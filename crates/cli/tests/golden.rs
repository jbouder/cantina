//! Rendering each stock pedal from its JSON through `render pedal`'s path
//! matches the dsp crate's golden files: the CLI hears what the app plays.
//!
//! The fixtures are the ones `crates/dsp/tests/golden.rs` generates, made
//! the same way here; the references are its files.

use std::path::{Path, PathBuf};

use cantina_cli::render::render_pedal;
use cantina_dsp::PedalDef;

const RATE: f64 = 48_000.0;
const FRAMES: usize = 12_000;
const TOLERANCE: f32 = 1e-5;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> Vec<(&'static str, Vec<f32>)> {
    let mut impulse = vec![0.0; FRAMES];
    impulse[0] = 1.0;
    let sine = (0..FRAMES)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.5)
        .collect();
    let (f0, f1) = (50.0f64, 10_000.0f64);
    let seconds = FRAMES as f64 / RATE;
    let k = (f1 / f0).ln();
    let sweep = (0..FRAMES)
        .map(|i| {
            let t = i as f64 / RATE;
            let phase = std::f64::consts::TAU * f0 * seconds / k * ((k * t / seconds).exp() - 1.0);
            (phase.sin() * 0.5) as f32
        })
        .collect();
    let period = (RATE / 100.0) as usize;
    let square = (0..FRAMES)
        .map(|i| {
            if i >= (RATE * 0.1) as usize {
                0.0
            } else if (i % period) < period / 2 {
                1.0
            } else {
                -1.0
            }
        })
        .collect();
    vec![
        ("impulse", impulse),
        ("sine", sine),
        ("sweep", sweep),
        ("square", square),
    ]
}

#[test]
fn every_stock_pedal_json_renders_its_golden_files() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for entry in std::fs::read_dir(root().join("pedals")).unwrap() {
        let path = entry.unwrap().path();
        let name = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_suffix(".pedal.json")
            .unwrap()
            .to_string();
        let def = PedalDef::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (fixture, input) in fixtures() {
            let golden = root().join(format!("crates/dsp/tests/golden/{name}_{fixture}.wav"));
            let expected: Vec<f32> = hound::WavReader::open(&golden)
                .unwrap_or_else(|e| panic!("{}: {e}", golden.display()))
                .samples::<f32>()
                .map(|s| s.unwrap())
                .collect();
            let (out, _) = render_pedal(&def, &[], RATE, &[input], 0, 120.0).unwrap();
            assert_eq!(out[0].len(), expected.len(), "{name}_{fixture}");
            let worst = out[0]
                .iter()
                .zip(&expected)
                .map(|(a, e)| (a - e).abs())
                .fold(0.0f32, f32::max);
            if worst > TOLERANCE {
                failures.push(format!("{name}_{fixture}: off by {worst}"));
            }
            checked += 1;
        }
    }
    assert!(checked >= 9 * 4, "only {checked} renders checked");
    assert!(failures.is_empty(), "{failures:#?}");
}
