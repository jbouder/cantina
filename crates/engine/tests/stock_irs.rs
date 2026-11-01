//! The impulse responses that ship in `irs/` at the repository root:
//! every one loads, is short, has unit energy, and is listed in
//! `irs/CREDITS.md`, so a file never ships unexplained.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cantina_engine::ir;

fn irs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../irs")
}

#[test]
fn stock_irs_load_fast_and_are_credited() {
    let found = ir::discover(&[irs_dir()]);
    let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        ["1x12-open-back", "2x12-closed", "4x10-bass", "4x12-modern"],
        "sorted by name"
    );
    let credits = std::fs::read_to_string(irs_dir().join("CREDITS.md")).unwrap();
    for entry in &found {
        let start = Instant::now();
        let imp = ir::load(&entry.path).unwrap_or_else(|e| panic!("{}: {e:#}", entry.id));
        let took = start.elapsed();
        assert!(took.as_millis() < 50, "{} took {took:?}", entry.id);
        assert_eq!(imp.sample_rate, 48_000.0);
        assert_eq!(imp.samples.len(), 4800, "{} is 100 ms", entry.id);
        let energy: f32 = imp.samples.iter().map(|s| s * s).sum();
        assert!((energy - 1.0).abs() < 0.01, "{} energy {energy}", entry.id);
        let file = entry.path.file_name().unwrap().to_str().unwrap();
        assert!(credits.contains(file), "{file} is not in CREDITS.md");
    }
    // Every WAV in the folder was found.
    let wavs = std::fs::read_dir(irs_dir())
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .count();
    assert_eq!(wavs, found.len());
}
