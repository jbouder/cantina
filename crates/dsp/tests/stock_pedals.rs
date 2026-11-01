//! The stock pedals that ship as pedal JSON (`pedals/`), checked for
//! the behaviour that matters: these are the tests their native
//! implementations had, run against the definitions through the
//! interpreter.

use std::f32::consts::TAU;

use cantina_dsp::oversample::LATENCY;
use cantina_dsp::{compile, stock, stock_definition, Buffer, Processor, Renderer, Transport, Unit};

const RATE: f32 = 48_000.0;

fn sine(hz: f32, n: usize, amplitude: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amplitude * (TAU * hz * i as f32 / RATE).sin())
        .collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

/// Power at `hz` in `signal`, in dB, Hann-windowed.
fn goertzel_db(signal: &[f32], hz: f32) -> f32 {
    let w = TAU * hz / RATE;
    let coeff = 2.0 * w.cos() as f64;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for (i, x) in signal.iter().enumerate() {
        let window = 0.5 - 0.5 * (TAU * i as f32 / signal.len() as f32).cos();
        let s0 = (x * window) as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    10.0 * (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(1e-30).log10() as f32
}

fn white_noise(n: usize) -> Vec<f32> {
    let mut seed = 7u32;
    (0..n)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

fn render(pedal: &mut dyn Processor, input: &[f32]) -> Vec<f32> {
    Renderer::default().render(pedal, std::slice::from_ref(&input.to_vec()))[0].clone()
}

#[test]
fn every_json_stock_pedal_is_shipped_valid_and_builds_the_same_way() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../pedals");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".pedal.json").map(str::to_string)
        })
        .collect();
    files.sort();
    let mut embedded: Vec<&str> = cantina_dsp::STOCK_PEDAL_JSON
        .iter()
        .map(|(n, _)| *n)
        .collect();
    embedded.sort();
    assert_eq!(
        files, embedded,
        "every file in pedals/ is embedded, and only those"
    );
    for (name, text) in cantina_dsp::STOCK_PEDAL_JSON {
        let on_disk = std::fs::read_to_string(dir.join(format!("{name}.pedal.json"))).unwrap();
        assert_eq!(&on_disk, text, "{name}: the embedded text is the file");
        let def = stock_definition(name).unwrap();
        assert_eq!(
            &cantina_dsp::PedalDef::from_json(&def.to_json()).unwrap(),
            def,
            "{name} round-trips"
        );
        let from_def = compile(def).unwrap();
        let from_stock = stock(name).unwrap();
        assert_eq!(from_def.name(), from_stock.name());
        assert_eq!(from_def.params(), from_stock.params());
        assert!(
            cantina_dsp::STOCK_NAMES.contains(name),
            "{name} is a stock name"
        );
    }
    assert!(
        stock_definition("tuner").is_none(),
        "the tuner stays native"
    );
}

// --- drives ---

const DRIVE: usize = 0;
const TONE: usize = 1;
const LEVEL: usize = 2;
const CURVE: usize = 3;
const OVERSAMPLING: usize = 4;

fn drives() -> Vec<Box<dyn Processor>> {
    ["overdrive", "distortion", "fuzz"]
        .into_iter()
        .map(|n| stock(n).unwrap())
        .collect()
}

#[test]
fn drives_are_silent_on_silence_and_report_the_oversamplers_latency() {
    for mut pedal in drives() {
        let out = render(pedal.as_mut(), &vec![0.0; 2048]);
        assert!(
            out.iter().all(|s| s.abs() < 1e-4),
            "{} on silence",
            pedal.name()
        );
        assert_eq!(pedal.latency(), LATENCY);
        let names: Vec<&str> = pedal.params().iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(names, ["Drive", "Tone", "Level", "Curve", "Oversampling"]);
    }
}

#[test]
fn drives_are_bounded_by_their_level_and_driven_hard() {
    for mut pedal in drives() {
        pedal.set_param(LEVEL, 0.0);
        let out = render(pedal.as_mut(), &sine(220.0, 9600, 10.0));
        let peak = out[4800..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak <= 1.3, "{} peak {peak}", pedal.name());
        assert!(peak > 0.7, "{} peak {peak}", pedal.name());
    }
}

#[test]
fn more_drive_means_more_compression() {
    let mut quiet = stock("overdrive").unwrap();
    quiet.set_param(DRIVE, 0.0);
    quiet.set_param(LEVEL, 0.0);
    let mut hot = stock("overdrive").unwrap();
    hot.set_param(DRIVE, 40.0);
    hot.set_param(LEVEL, 0.0);
    let input = sine(220.0, 9600, 0.05);
    let a = render(quiet.as_mut(), &input);
    let b = render(hot.as_mut(), &input);
    let (a, b) = (&a[4800..], &b[4800..]);
    assert!(rms(b) > rms(a) * 5.0, "quiet {} hot {}", rms(a), rms(b));
    assert!(rms(b) < 1.0);
}

#[test]
fn the_three_drives_and_their_curves_sound_different() {
    let input = sine(110.0, 9600, 0.2);
    let mut renders: Vec<(String, Vec<f32>)> = Vec::new();
    for mut pedal in drives() {
        pedal.set_param(LEVEL, 0.0);
        for curve in 0..3 {
            pedal.set_param(CURVE, curve as f32);
            let out = render(pedal.as_mut(), &input);
            renders.push((
                format!("{} curve {curve}", pedal.name()),
                out[4800..].to_vec(),
            ));
        }
    }
    for (i, (a, x)) in renders.iter().enumerate() {
        for (b, y) in &renders[i + 1..] {
            let diff = x.iter().zip(y).map(|(p, q)| (p - q).abs()).sum::<f32>() / x.len() as f32;
            assert!(diff > 0.01, "{a} and {b} render the same ({diff})");
        }
    }
}

#[test]
fn oversampling_choice_changes_the_factor_but_not_the_latency() {
    let mut d = stock("distortion").unwrap();
    assert_eq!(d.param(OVERSAMPLING), 2.0);
    assert_eq!(
        d.params()[OVERSAMPLING].unit,
        Unit::Choice(&["Off", "2×", "4×", "8×"])
    );
    d.prepare(48_000.0, 64);
    for index in [0.0, 1.0, 3.0] {
        d.set_param(OVERSAMPLING, index);
        assert_eq!(d.param(OVERSAMPLING), index);
        assert_eq!(d.latency(), LATENCY);
    }
    let input = sine(5000.0, 48_000, 0.5);
    let render_at = |index: f32| {
        let mut d = stock("distortion").unwrap();
        d.set_param(DRIVE, 40.0);
        d.set_param(TONE, 8000.0);
        d.set_param(OVERSAMPLING, index);
        render(d.as_mut(), &input)[4800..].to_vec()
    };
    let plain = render_at(0.0);
    let eight = render_at(3.0);
    let alias_plain = goertzel_db(&plain, 3000.0) - goertzel_db(&plain, 5000.0);
    let alias_eight = goertzel_db(&eight, 3000.0) - goertzel_db(&eight, 5000.0);
    assert!(
        alias_eight < alias_plain - 20.0,
        "3 kHz alias: {alias_plain} dB off, {alias_eight} dB at 8x"
    );
}

#[test]
fn tone_sweep_does_not_jump() {
    let mut od = stock("overdrive").unwrap();
    od.prepare(48_000.0, 64);
    let mut buffer = sine(440.0, 48_000, 0.3);
    od.set_param(TONE, 500.0);
    for (i, block) in buffer.chunks_mut(64).enumerate() {
        if i == 100 {
            od.set_param(TONE, 8000.0);
        }
        let mut channels = [block];
        od.process(&mut Buffer::new(&mut channels));
    }
    let max_step = buffer[6400..]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(max_step < 0.1, "largest step {max_step}");
}

#[test]
fn saved_m0_overdrive_values_still_apply() {
    let mut od = stock("overdrive").unwrap();
    for (id, value) in [(0usize, 33.0f32), (1, 1234.0), (2, -3.0)] {
        od.set_param(id, value);
    }
    assert_eq!(od.param(0), 33.0);
    assert_eq!(od.param(1), 1234.0);
    assert_eq!(od.param(2), -3.0);
    assert_eq!(od.param(CURVE), 0.0);
    assert_eq!(od.param(OVERSAMPLING), 2.0);
}

// --- amp ---

#[test]
fn amp_has_the_knobs_and_the_oversamplers_latency() {
    let amp = stock("amp").unwrap();
    let names: Vec<&str> = amp.params().iter().map(|p| p.name.as_ref()).collect();
    assert_eq!(
        names,
        ["Gain", "Bass", "Mid", "Treble", "Presence", "Level"]
    );
    assert_eq!(amp.latency(), LATENCY);
}

#[test]
fn amp_tone_knobs_act_in_their_bands_and_gain_drives_the_clipper() {
    let tone = |hz: f32, amplitude: f32| sine(hz, 24_000, amplitude);
    let measure = |knob: usize, db: f32, hz: f32| {
        let mut amp = stock("amp").unwrap();
        amp.set_param(0, 0.0);
        amp.set_param(5, 0.0);
        amp.set_param(knob, db);
        let out = render(amp.as_mut(), &tone(hz, 0.01));
        let mut flat = stock("amp").unwrap();
        flat.set_param(0, 0.0);
        flat.set_param(5, 0.0);
        let reference = render(flat.as_mut(), &tone(hz, 0.01));
        20.0 * (rms(&out[12_000..]) / rms(&reference[12_000..])).log10()
    };
    assert!((measure(1, 9.0, 60.0) - 9.0).abs() < 1.0);
    assert!(measure(1, 9.0, 3000.0).abs() < 0.5);
    assert!((measure(2, -9.0, 700.0) + 9.0).abs() < 1.0);
    assert!((measure(3, 6.0, 10_000.0) - 6.0).abs() < 1.0);
    assert!(measure(3, 6.0, 100.0).abs() < 0.5);
    assert!(measure(4, 6.0, 12_000.0) > 4.0);
    assert!(measure(4, 6.0, 2000.0) < 2.5);

    let mut clean = stock("amp").unwrap();
    clean.set_param(0, 0.0);
    clean.set_param(5, 0.0);
    let mut hot = stock("amp").unwrap();
    hot.set_param(0, 40.0);
    hot.set_param(5, 0.0);
    let input = tone(220.0, 0.05);
    let a = rms(&render(clean.as_mut(), &input)[12_000..]);
    let b = rms(&render(hot.as_mut(), &input)[12_000..]);
    assert!(b > a * 5.0 && b < 1.0, "clean {a} hot {b}");
}

// --- eq ---

fn level_at(eq: &mut dyn Processor, hz: f32) -> f32 {
    let out = render(eq, &sine(hz, 24_000, 0.5));
    let tail = &out[12_000..];
    20.0 * (rms(tail) / (0.5 / 2f32.sqrt())).log10()
}

#[test]
fn eq_has_four_bands_and_is_flat_by_default() {
    let mut eq = stock("eq").unwrap();
    assert_eq!(eq.params().len(), 16);
    for b in 0..4 {
        let names: Vec<&str> = eq.params()[b * 4..b * 4 + 4]
            .iter()
            .map(|p| p.name.as_ref())
            .collect();
        assert_eq!(
            names,
            [format!("Band {}", b + 1).as_str(), "Freq", "Gain", "Q"]
        );
        assert!(matches!(eq.params()[b * 4].unit, Unit::Choice(_)));
    }
    for hz in [50.0, 400.0, 3000.0, 12_000.0] {
        assert!(level_at(eq.as_mut(), hz).abs() < 0.05, "{hz} Hz");
    }
}

#[test]
fn each_eq_type_acts_where_it_says() {
    let fresh = |settings: &[(usize, f32)]| {
        let mut eq = stock("eq").unwrap();
        for (id, v) in settings {
            eq.set_param(*id, *v);
        }
        eq
    };
    let mut peak = fresh(&[(5, 1000.0), (6, 9.0)]);
    assert!((level_at(peak.as_mut(), 1000.0) - 9.0).abs() < 0.3);
    assert!(level_at(peak.as_mut(), 100.0).abs() < 0.3);
    let mut low = fresh(&[(2, -12.0)]);
    assert!((level_at(low.as_mut(), 30.0) + 12.0).abs() < 0.5);
    assert!(level_at(low.as_mut(), 2000.0).abs() < 0.3);
    let mut high = fresh(&[(14, 6.0)]);
    assert!((level_at(high.as_mut(), 15_000.0) - 6.0).abs() < 0.5);
    assert!(level_at(high.as_mut(), 300.0).abs() < 0.3);
    let mut cut = fresh(&[(0, 3.0), (1, 200.0)]);
    assert!(level_at(cut.as_mut(), 40.0) < -20.0);
    assert!(level_at(cut.as_mut(), 2000.0).abs() < 0.3);
    let mut top = fresh(&[(8, 4.0), (9, 2000.0)]);
    assert!(level_at(top.as_mut(), 12_000.0) < -25.0);
    assert!(level_at(top.as_mut(), 200.0).abs() < 0.3);
    let mut notch = fresh(&[(4, 5.0), (5, 1000.0), (7, 5.0)]);
    assert!(level_at(notch.as_mut(), 1000.0) < -30.0);
    assert!(level_at(notch.as_mut(), 500.0).abs() < 1.0);
}

#[test]
fn an_eq_knob_move_does_not_click() {
    let mut eq = stock("eq").unwrap();
    eq.prepare(48_000.0, 64);
    let mut buffer = sine(300.0, 24_000, 0.3);
    for (i, block) in buffer.chunks_mut(64).enumerate() {
        eq.set_param(5, 300.0);
        eq.set_param(6, (i as f32 / 20.0).min(12.0));
        let mut lanes = [block];
        eq.process(&mut Buffer::new(&mut lanes));
    }
    let max_step = buffer
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(max_step < 0.1, "{max_step}");
}

// --- modulation ---

const MOD_RATE: usize = 0;
const MOD_DEPTH: usize = 1;
const MOD_THIRD: usize = 2;
const MOD_MIX: usize = 3;

#[test]
fn every_modulation_pedal_has_its_knobs() {
    for name in ["chorus", "flanger", "phaser", "tremolo"] {
        let pedal = stock(name).unwrap();
        let names: Vec<&str> = pedal.params().iter().map(|p| p.name.as_ref()).collect();
        for wanted in ["Rate", "Depth", "Sync", "Note"] {
            assert!(names.contains(&wanted), "{name} has no {wanted}");
        }
        if name != "tremolo" {
            assert!(
                names.contains(&"Feedback") && names.contains(&"Mix"),
                "{name}"
            );
        }
        assert_eq!(pedal.latency(), 0);
    }
}

#[test]
fn chorus_with_no_depth_is_a_fixed_delay_and_no_mix_is_dry() {
    let input = sine(440.0, 9600, 0.5);
    let mut dry = stock("chorus").unwrap();
    dry.set_param(MOD_MIX, 0.0);
    assert_eq!(render(dry.as_mut(), &input), input);

    let mut fixed = stock("chorus").unwrap();
    fixed.set_param(MOD_DEPTH, 0.0);
    fixed.set_param(MOD_MIX, 100.0);
    let out = render(fixed.as_mut(), &input);
    let delay = (12.0 * RATE / 1000.0) as usize;
    let worst = (delay + 100..9600)
        .map(|i| (out[i] - input[i - delay]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-3, "{worst}");
}

#[test]
fn chorus_spreads_a_tone_and_the_channels_differ() {
    let input = sine(440.0, 48_000, 0.5);
    let mut chorus = stock("chorus").unwrap();
    chorus.set_param(MOD_RATE, 2.0);
    let out = Renderer::default().render(chorus.as_mut(), &[input.clone(), input.clone()]);
    let centre = goertzel_db(&out[0], 440.0);
    let side = goertzel_db(&out[0], 446.0);
    let side_in = goertzel_db(&input, 446.0);
    assert!(side - centre > side_in - goertzel_db(&input, 440.0) + 10.0);
    let diff: f32 = out[0].iter().zip(&out[1]).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 100.0, "channels are the same: {diff}");
}

#[test]
fn flanger_cuts_a_comb_into_noise() {
    let mut flanger = stock("flanger").unwrap();
    flanger.set_param(MOD_DEPTH, 0.0);
    flanger.set_param(MOD_THIRD, 0.0);
    let out = render(flanger.as_mut(), &white_noise(96_000));
    let notch = goertzel_db(&out, 500.0);
    let peak = goertzel_db(&out, 1000.0);
    assert!(peak - notch > 15.0, "peak {peak} notch {notch}");
}

#[test]
fn phaser_notches_and_feedback_sharpens() {
    let mut phaser = stock("phaser").unwrap();
    phaser.set_param(MOD_DEPTH, 0.0);
    phaser.set_param(MOD_THIRD, 0.0);
    let noise = white_noise(96_000);
    let out = render(phaser.as_mut(), &noise);
    let notch = goertzel_db(&out, 724.0);
    let corner = goertzel_db(&out, 300.0);
    let away = goertzel_db(&out, 5000.0);
    assert!(corner - notch > 15.0, "notch {notch}, corner {corner}");
    assert!(away - notch > 15.0, "notch {notch}, away {away}");
    let mut sharp = stock("phaser").unwrap();
    sharp.set_param(MOD_DEPTH, 0.0);
    sharp.set_param(MOD_THIRD, 90.0);
    let out = render(sharp.as_mut(), &noise);
    assert!(out.iter().all(|s| s.is_finite() && s.abs() < 10.0));
}

#[test]
fn tremolo_pumps_at_its_rate_and_follows_the_tempo() {
    let flat = vec![1.0f32; 48_000];
    let mut trem = stock("tremolo").unwrap();
    trem.set_param(MOD_RATE, 4.0);
    trem.set_param(MOD_DEPTH, 100.0);
    let out = render(trem.as_mut(), &flat);
    let dips = out
        .windows(3)
        .filter(|w| w[1] < w[0] && w[1] <= w[2] && w[1] < 0.01)
        .count();
    assert_eq!(dips, 4, "{dips} dips");
    let top = out.iter().fold(0.0f32, |m, s| m.max(*s));
    assert!((top - 1.0).abs() < 1e-3);

    // Synced to an eighth note at 90 bpm: 3 Hz, six dips in two
    // seconds, the first half a cycle after the beat (a triangle).
    let mut synced = stock("tremolo").unwrap();
    synced.set_param(3, 1.0);
    synced.set_param(4, 6.0);
    synced.set_param(MOD_DEPTH, 100.0);
    synced.set_param(MOD_THIRD, 1.0);
    synced.prepare(48_000.0, 64);
    let mut lanes = vec![vec![1.0f32; 96_000]];
    let mut position = 0u64;
    Renderer::default().run(&mut lanes, |buffer| {
        synced.set_transport(&Transport {
            bpm: 90.0,
            beats: position as f64 / 48_000.0 * 1.5,
            playing: true,
        });
        synced.process(buffer);
        position += buffer.frames() as u64;
    });
    let dips: Vec<usize> = lanes[0]
        .windows(3)
        .enumerate()
        .filter(|(_, w)| w[1] < w[0] && w[1] <= w[2] && w[1] < 0.01)
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(dips.len(), 6, "{dips:?}");
    assert!((dips[0] as i64 - 8000).abs() < 3, "{dips:?}");
    assert!((dips[1] as i64 - 24_000).abs() < 3, "{dips:?}");
    assert!(
        lanes[0][0] > 0.999 && lanes[0][16_000] > 0.999,
        "not loudest on the beat"
    );
}
