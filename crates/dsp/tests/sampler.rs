//! Golden-file test for the sampler: a kit of synthesized drums played
//! through a bar of a beat (and a beat of tail) in 64-frame blocks,
//! compared sample by sample against `tests/golden/sampler_beat.wav`,
//! 16-bit stereo. The hits are at known frames, so the test also checks,
//! without the reference, that every hit starts on exactly its frame and
//! nowhere else.
//!
//! Regenerate the reference after an intended change with
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test sampler
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cantina_dsp::{Buffer, Kit, LoadedKit, Processor, Sample, Sampler};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

const SAMPLE_RATE: f64 = 48_000.0;
const BLOCK: usize = 64;
/// The reference is 16-bit, so half a step of that plus a little.
const TOLERANCE: f32 = 2.0 / 32_768.0;
/// A bar of 4/4 at 120 bpm and one beat of tail.
const FRAMES: usize = 5 * 24_000;
const SIXTEENTH: usize = 6_000;

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sampler_beat.wav")
}

fn updating() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some_and(|v| !v.is_empty() && v != "0")
}

// --- a kit made of arithmetic, so nothing binary is needed ---

fn decaying_sine(hz: f32, seconds: f32, sweep: f32) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let f = hz * (1.0 - sweep * t);
            phase += std::f32::consts::TAU * f / SAMPLE_RATE as f32;
            phase.sin() * (1.0 - t).powi(3) * 0.5
        })
        .collect()
}

/// Deterministic noise with an exponential tail.
fn noise_burst(seed: u32, seconds: f32, decay: f32) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    let mut state = seed;
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let r = (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            r * (-(i as f32) * decay / SAMPLE_RATE as f32).exp() * 0.5
        })
        .collect()
}

fn mono(values: Vec<f32>) -> Arc<Sample> {
    Arc::new(Sample {
        sample_rate: SAMPLE_RATE,
        channels: vec![values],
    })
}

fn stereo(left: Vec<f32>, right: Vec<f32>) -> Arc<Sample> {
    Arc::new(Sample {
        sample_rate: SAMPLE_RATE,
        channels: vec![left, right],
    })
}

const KIT_JSON: &str = r#"{
  "version": 1,
  "name": "Golden",
  "pads": [
    { "id": "kick", "layers": [
      { "file": "kick-soft", "velocity": [0, 0.7] },
      { "file": "kick-hard", "velocity": [0.7, 1] } ] },
    { "id": "snare", "pan": -0.2, "layers": [{ "file": "snare" }] },
    { "id": "hh-closed", "pan": 0.4, "choke": "hat", "layers": [{ "file": "hh-closed" }] },
    { "id": "hh-open", "pan": 0.4, "choke": "hat", "gainDb": -3, "layers": [{ "file": "hh-open" }] },
    { "id": "ride", "layers": [{ "file": "ride" }] }
  ]
}"#;

fn kit() -> Arc<LoadedKit> {
    let kit = Kit::from_json(KIT_JSON).unwrap();
    Arc::new(
        LoadedKit::build(&kit, |file| {
            Ok(match file {
                "kick-soft" => mono(decaying_sine(90.0, 0.25, 0.5)),
                "kick-hard" => mono(decaying_sine(110.0, 0.35, 0.6)),
                "snare" => mono(noise_burst(7, 0.2, 18.0)),
                "hh-closed" => mono(noise_burst(11, 0.06, 60.0)),
                "hh-open" => mono(noise_burst(13, 0.5, 6.0)),
                "ride" => stereo(noise_burst(17, 0.6, 4.0), noise_burst(19, 0.6, 4.0)),
                _ => return Err(format!("no sample {file}")),
            })
        })
        .unwrap(),
    )
}

/// The beat: (sixteenth, pad, velocity). A bar, then the downbeat of the
/// next, whose closed hat chokes the open hat on the "and" of 4.
fn hits() -> Vec<(usize, usize, f32)> {
    let mut hits = vec![
        (0, 0, 1.0),
        (4, 1, 0.9),
        (8, 0, 0.6),
        (10, 0, 0.5),
        (12, 1, 1.0),
        (14, 3, 0.7),
        (15, 1, 0.3),
        (16, 0, 1.0),
        (16, 2, 0.8),
        (16, 4, 0.5),
    ];
    for s in (0..14).step_by(2) {
        hits.push((s, 2, if s % 4 == 0 { 0.8 } else { 0.45 }));
    }
    hits.sort_by_key(|h| h.0);
    hits
}

fn render(sampler: &mut Sampler, hits: &[(usize, usize, f32)]) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0f32; FRAMES];
    let mut right = vec![0.0f32; FRAMES];
    let mut start = 0;
    while start < FRAMES {
        let end = (start + BLOCK).min(FRAMES);
        for &(sixteenth, pad, velocity) in hits {
            let at = sixteenth * SIXTEENTH;
            if (start..end).contains(&at) {
                assert!(sampler.trigger(pad, velocity, at - start));
            }
        }
        let mut lanes = [&mut left[start..end], &mut right[start..end]];
        sampler.process(&mut Buffer::new(&mut lanes));
        start = end;
    }
    (left, right)
}

fn write_wav(path: &Path, left: &[f32], right: &[f32]) {
    let spec = WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let to_int = |v: f32| (v.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
    let mut writer = WavWriter::create(path, spec).unwrap();
    for (l, r) in left.iter().zip(right) {
        writer.write_sample(to_int(*l)).unwrap();
        writer.write_sample(to_int(*r)).unwrap();
    }
    writer.finalize().unwrap();
}

fn read_wav(path: &Path) -> (Vec<f32>, Vec<f32>) {
    let mut reader = WavReader::open(path).unwrap();
    assert_eq!(reader.spec().channels, 2);
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32_767.0)
        .collect();
    let left = samples.iter().step_by(2).copied().collect();
    let right = samples.iter().skip(1).step_by(2).copied().collect();
    (left, right)
}

#[test]
fn beat_matches_the_golden_file() {
    let mut sampler = Sampler::new(kit());
    sampler.prepare(SAMPLE_RATE, BLOCK);
    let (left, right) = render(&mut sampler, &hits());
    assert!(left.iter().chain(&right).all(|s| s.is_finite()));
    let path = golden_path();
    if updating() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_wav(&path, &left, &right);
        return;
    }
    assert!(
        path.exists(),
        "no reference at {}; run with UPDATE_GOLDEN=1 to create it",
        path.display()
    );
    let (expected_left, expected_right) = read_wav(&path);
    for (lane, expected, actual) in [
        ("left", &expected_left, &left),
        ("right", &expected_right, &right),
    ] {
        assert_eq!(expected.len(), actual.len(), "{lane} length");
        let (mut worst, mut at) = (0.0f32, 0usize);
        for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
            let d = (e - a).abs();
            if d > worst {
                worst = d;
                at = i;
            }
        }
        assert!(
            worst <= TOLERANCE,
            "{lane} differs from the reference by {worst} at sample {at} \
             (reference {}, render {}); tolerance {TOLERANCE}",
            expected[at],
            actual[at]
        );
    }
}

#[test]
fn every_hit_starts_on_its_frame() {
    // One pad at a time, so each hit's onset is unmistakable: the output
    // at the hit's frame, less what the pad's earlier hits still
    // contribute there, is the sample's first value at the hit's gain.
    // The frame before the first hit is silent.
    let kit = kit();
    for pad in 0..kit.pads().len() {
        let hits: Vec<(usize, usize, f32)> = hits().into_iter().filter(|h| h.1 == pad).collect();
        if hits.is_empty() {
            continue;
        }
        let (left, right) = render_fresh(&kit, &hits);
        let first_at = hits[0].0 * SIXTEENTH;
        assert!(
            left[..first_at].iter().all(|v| *v == 0.0),
            "pad {pad} early"
        );
        for (i, &(sixteenth, _, velocity)) in hits.iter().enumerate() {
            let at = sixteenth * SIXTEENTH;
            let (ring_left, ring_right) = render_fresh(&kit, &hits[..i]);
            let p = &kit.pads()[pad];
            let layer = &p.layers[p.layer_for(velocity)];
            let gain = velocity * p.gain * layer.gain;
            let angle = (p.pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
            let stereo = layer.sample.channels.len() == 2;
            let width = if stereo {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let expected_left = layer.sample.channels[0][0] * gain * angle.cos() * width;
            let got_left = left[at] - ring_left[at];
            assert!(
                (got_left - expected_left).abs() < 1e-4,
                "pad {pad} at sixteenth {sixteenth}: {got_left}, expected {expected_left}"
            );
            let right_source = if stereo { 1 } else { 0 };
            let expected_right =
                layer.sample.channels[right_source][0] * gain * angle.sin() * width;
            let got_right = right[at] - ring_right[at];
            assert!(
                (got_right - expected_right).abs() < 1e-4,
                "pad {pad} at sixteenth {sixteenth} right: {got_right}, expected {expected_right}"
            );
        }
    }
}

fn render_fresh(kit: &Arc<LoadedKit>, hits: &[(usize, usize, f32)]) -> (Vec<f32>, Vec<f32>) {
    let mut sampler = Sampler::new(Arc::clone(kit));
    sampler.prepare(SAMPLE_RATE, BLOCK);
    render(&mut sampler, hits)
}
