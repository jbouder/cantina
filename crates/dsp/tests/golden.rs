//! Golden-file tests: every stock processor over a set of fixtures,
//! compared sample by sample against reference renders committed in
//! `tests/golden/`.
//!
//! Fixtures are generated here, not stored, so only the references live
//! in the repository. A change that alters a processor's output fails
//! here with the worst deviation and where it was. To accept a change on
//! purpose, regenerate the references and commit them:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden
//! ```
//!
//! `tests/golden/summary.txt` is rewritten at the same time with a few
//! numbers per file, so a review can see what moved without decoding WAV.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cantina_dsp::{stock, Processor, Renderer, STOCK_NAMES};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

const SAMPLE_RATE: f64 = 48_000.0;
const BLOCK: usize = 64;
/// Fixture length: a quarter second.
const FRAMES: usize = 12_000;
/// Largest sample difference a render may show against its reference.
/// Float summation order can shift the last bits; a real DSP change is
/// orders of magnitude larger.
const TOLERANCE: f32 = 1e-5;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn updating() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some_and(|v| !v.is_empty() && v != "0")
}

// --- fixtures ---

fn impulse() -> Vec<f32> {
    let mut v = vec![0.0; FRAMES];
    v[0] = 1.0;
    v
}

fn sine() -> Vec<f32> {
    (0..FRAMES)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin() * 0.5)
        .collect()
}

/// A logarithmic sweep from 50 Hz to 10 kHz at half scale.
fn sweep() -> Vec<f32> {
    let (f0, f1) = (50.0f64, 10_000.0f64);
    let seconds = FRAMES as f64 / SAMPLE_RATE;
    let k = (f1 / f0).ln();
    (0..FRAMES)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE;
            let phase = std::f64::consts::TAU * f0 * seconds / k * ((k * t / seconds).exp() - 1.0);
            (phase.sin() * 0.5) as f32
        })
        .collect()
}

/// A full-scale 100 Hz square for 100 ms, then silence: hard edges and a
/// tail for anything with memory.
fn square_burst() -> Vec<f32> {
    let period = (SAMPLE_RATE / 100.0) as usize;
    (0..FRAMES)
        .map(|i| {
            if i >= (SAMPLE_RATE * 0.1) as usize {
                0.0
            } else if (i % period) < period / 2 {
                1.0
            } else {
                -1.0
            }
        })
        .collect()
}

fn fixtures() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        ("impulse", impulse()),
        ("sine", sine()),
        ("sweep", sweep()),
        ("square", square_burst()),
    ]
}

// --- cases ---

/// A processor configuration to render every fixture through.
struct Case {
    name: String,
    processor: Box<dyn Processor>,
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for name in STOCK_NAMES {
        cases.push(Case {
            name: name.to_string(),
            processor: stock(name).unwrap(),
        });
    }
    let mut quiet = stock("gain").unwrap();
    quiet.set_param(0, -12.0);
    cases.push(Case {
        name: "gain-12db".into(),
        processor: quiet,
    });
    let mut hot = stock("overdrive").unwrap();
    hot.set_param(0, 40.0);
    hot.set_param(1, 800.0);
    hot.set_param(2, 0.0);
    cases.push(Case {
        name: "overdrive-hot".into(),
        processor: hot,
    });
    // Each drive on its second curve, without oversampling, so the
    // references also cover the plain path.
    for name in ["overdrive", "distortion", "fuzz"] {
        let mut p = stock(name).unwrap();
        p.set_param(3, 1.0);
        p.set_param(4, 0.0);
        cases.push(Case {
            name: format!("{name}-alt"),
            processor: p,
        });
    }
    // A gate that the fixtures' quiet tails fall below.
    let mut gate = stock("gate").unwrap();
    gate.set_param(0, -20.0);
    gate.set_param(2, 10.0);
    gate.set_param(3, 30.0);
    cases.push(Case {
        name: "gate-tight".into(),
        processor: gate,
    });
    // A compressor squashing hard with a fast attack and a lift.
    let mut squash = stock("compressor").unwrap();
    squash.set_param(0, -30.0);
    squash.set_param(1, 10.0);
    squash.set_param(2, 1.0);
    squash.set_param(4, 0.0);
    squash.set_param(5, 6.0);
    cases.push(Case {
        name: "compressor-squash".into(),
        processor: squash,
    });
    // The EQ with every band doing something.
    let mut eq = stock("eq").unwrap();
    eq.set_param(2, -6.0);
    eq.set_param(6, 4.0);
    eq.set_param(7, 2.0);
    eq.set_param(10, -3.0);
    eq.set_param(14, 5.0);
    cases.push(Case {
        name: "eq-shaped".into(),
        processor: eq,
    });
    // Modulation at a brisk rate, so a quarter second shows a cycle.
    for name in ["chorus", "flanger", "phaser", "tremolo"] {
        let mut p = stock(name).unwrap();
        p.set_param(0, 6.0);
        cases.push(Case {
            name: format!("{name}-fast"),
            processor: p,
        });
    }
    // A short delay so the quarter-second fixtures show repeats, and
    // a reverb at full wet with a quick decay.
    let mut delay = stock("delay").unwrap();
    delay.set_param(0, 50.0);
    delay.set_param(3, 60.0);
    cases.push(Case {
        name: "delay-short".into(),
        processor: delay,
    });
    let mut room = stock("reverb").unwrap();
    room.set_param(1, 0.5);
    room.set_param(4, 100.0);
    cases.push(Case {
        name: "reverb-wet".into(),
        processor: room,
    });
    cases
}

// --- files ---

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    for s in samples {
        writer.write_sample(*s).unwrap();
    }
    writer.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    let mut reader = WavReader::open(path).unwrap();
    assert_eq!(reader.spec().channels, 1, "{}", path.display());
    reader.samples::<f32>().map(|s| s.unwrap()).collect()
}

fn summary_line(name: &str, samples: &[f32]) -> String {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let first = samples.iter().position(|s| s.abs() > 1e-6);
    let last = samples.iter().rposition(|s| s.abs() > 1e-6);
    format!(
        "{name:<28} peak {peak:>8.5}  rms {rms:>8.5}  nonzero {}..{}",
        first.map_or("-".to_string(), |i| i.to_string()),
        last.map_or("-".to_string(), |i| (i + 1).to_string()),
    )
}

/// Compares `actual` to the reference, or writes it when updating.
/// Returns an error line on mismatch.
fn check(name: &str, actual: &[f32], summary: &mut Vec<String>) -> Option<String> {
    let path = golden_dir().join(format!("{name}.wav"));
    summary.push(summary_line(name, actual));
    if updating() {
        write_wav(&path, actual);
        return None;
    }
    if !path.exists() {
        return Some(format!(
            "{name}: no reference at {}; run with UPDATE_GOLDEN=1 to create it",
            path.display()
        ));
    }
    let expected = read_wav(&path);
    if expected.len() != actual.len() {
        return Some(format!(
            "{name}: reference has {} samples, render has {}",
            expected.len(),
            actual.len()
        ));
    }
    let (mut worst, mut at) = (0.0f32, 0usize);
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        let d = (e - a).abs();
        if d > worst {
            worst = d;
            at = i;
        }
    }
    (worst > TOLERANCE).then(|| {
        format!(
            "{name}: differs from the reference by {worst} at sample {at} \
             (reference {}, render {}); tolerance {TOLERANCE}",
            expected[at], actual[at]
        )
    })
}

#[test]
fn stock_processors_match_their_golden_files() {
    let dir = golden_dir();
    fs::create_dir_all(&dir).unwrap();
    let renderer = Renderer::new(SAMPLE_RATE, BLOCK);
    let fixtures = fixtures();
    let mut summary = Vec::new();
    let mut failures = Vec::new();
    for case in cases() {
        let mut processor = case.processor;
        for (fixture, input) in &fixtures {
            processor.reset();
            let output = renderer.render(processor.as_mut(), std::slice::from_ref(input));
            assert!(
                output[0].iter().all(|s| s.is_finite()),
                "{} over {fixture} produced a non-finite sample",
                case.name
            );
            if let Some(failure) = check(
                &format!("{}_{fixture}", case.name),
                &output[0],
                &mut summary,
            ) {
                failures.push(failure);
            }
        }
    }
    if updating() {
        let mut text = String::from(
            "Golden renders: 48 kHz, 64-frame blocks, 12 000 frames per fixture.\n\
             Regenerate with UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden\n\n",
        );
        for line in summary {
            text.push_str(&line);
            text.push('\n');
        }
        fs::write(dir.join("summary.txt"), text).unwrap();
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

#[test]
fn fixtures_are_deterministic_and_bounded() {
    for (name, samples) in fixtures() {
        assert_eq!(samples.len(), FRAMES, "{name}");
        assert!(samples.iter().all(|s| s.abs() <= 1.0), "{name} clips");
        assert!(samples.iter().any(|s| *s != 0.0), "{name} is silent");
    }
    assert_eq!(fixtures()[2].1, sweep());
}

/// Times each drive over a minute of audio at every oversampling factor
/// and prints the cost as a share of real time. The numbers are
/// recorded in `docs/pedals.md`; the assertion only guards against a
/// drive becoming unusable.
#[test]
fn drives_cost_is_measured_at_every_factor() {
    let seconds = 10;
    let input = vec![sine()
        .into_iter()
        .cycle()
        .take(seconds * SAMPLE_RATE as usize)
        .collect::<Vec<f32>>()];
    let renderer = Renderer::new(SAMPLE_RATE, BLOCK);
    for name in ["overdrive", "distortion", "fuzz"] {
        for (index, label) in ["off", "2x", "4x", "8x"].iter().enumerate() {
            let mut processor = stock(name).unwrap();
            processor.set_param(4, index as f32);
            let start = Instant::now();
            renderer.render(processor.as_mut(), &input);
            let took = start.elapsed().as_secs_f64();
            let share = took / seconds as f64;
            eprintln!(
                "{name:<11} {label:<4} {:.2} ms per second of audio ({:.2}% of real time)",
                share * 1000.0,
                share * 100.0
            );
            assert!(
                share < 0.25,
                "{name} at {label} takes {share:.2} of real time"
            );
        }
    }
}

#[test]
fn renders_a_minute_well_under_a_second() {
    let minute = vec![sine()
        .into_iter()
        .cycle()
        .take(60 * SAMPLE_RATE as usize)
        .collect::<Vec<f32>>()];
    let mut processor = stock("overdrive").unwrap();
    let renderer = Renderer::new(SAMPLE_RATE, BLOCK);
    let start = Instant::now();
    let output = renderer.render(processor.as_mut(), &minute);
    let took = start.elapsed();
    assert_eq!(output[0].len(), minute[0].len());
    eprintln!("rendered one minute of overdrive in {took:?}");
    assert!(
        took.as_secs_f64() < 2.0,
        "a minute took {took:?}; the renderer should be far faster than real time"
    );
}
