//! The kits that ship in `kits/` at the repository root: every one
//! loads, has the pads a pattern written for another kit expects, names
//! only files that exist, and loads fast. `kits/CREDITS.md` must list
//! every sample file, so a sample never ships without its source.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cantina_engine::kit;

fn kits_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../kits")
}

/// Pads every stock kit has, so a pattern moves between them.
const SHARED_PADS: &[&str] = &["kick", "snare", "hh-closed", "hh-open", "tom-hi", "tom-lo"];

#[test]
fn stock_kits_load_fast_with_the_shared_pads() {
    let mut problems = Vec::new();
    let kits = kit::discover(&[kits_dir()], &mut problems);
    assert!(problems.is_empty(), "{problems:?}");
    let ids: Vec<&str> = kits.iter().map(|k| k.id.as_str()).collect();
    assert_eq!(ids, ["acoustic", "electronic", "rock"], "sorted by name");
    for entry in &kits {
        let start = Instant::now();
        let (kit, loaded) =
            kit::load(&entry.path).unwrap_or_else(|e| panic!("{}: {e:#}", entry.id));
        let took = start.elapsed();
        eprintln!(
            "{} loaded in {took:?}: {} frames",
            entry.id,
            loaded.frames()
        );
        assert!(
            took.as_millis() < 200,
            "{} took {took:?} to load; the budget is 200 ms",
            entry.id
        );
        assert_eq!(kit.name, entry.name);
        for pad in SHARED_PADS {
            assert!(
                loaded.pad_index(pad).is_some(),
                "{} has no {pad} pad",
                entry.id
            );
        }
        assert!(
            loaded.choke_groups().contains(&"hat".to_string()),
            "{} has no hat group",
            entry.id
        );
        assert!(
            loaded.pads().len() >= 9,
            "{} has only {} pads",
            entry.id,
            loaded.pads().len()
        );
        for pad in loaded.pads() {
            for layer in &pad.layers {
                assert!(
                    layer.sample.frames() > 0,
                    "{}: {} is empty",
                    entry.id,
                    pad.id
                );
                assert_eq!(layer.sample.sample_rate, 44_100.0);
            }
        }
    }
}

#[test]
fn every_sample_is_used_and_credited() {
    let credits = std::fs::read_to_string(kits_dir().join("CREDITS.md")).unwrap();
    let mut problems = Vec::new();
    for entry in kit::discover(&[kits_dir()], &mut problems) {
        let (kit, _) = kit::load(&entry.path).unwrap();
        let used: HashSet<String> = kit
            .pads
            .iter()
            .flat_map(|p| p.layers.iter().map(|l| l.file.clone()))
            .collect();
        for file in std::fs::read_dir(&entry.path).unwrap().flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if name == kit::KIT_FILE {
                continue;
            }
            assert!(
                name.ends_with(".wav"),
                "{}: {name} is not a WAV or the kit file",
                entry.id
            );
            assert!(
                used.contains(&name),
                "{}: {name} is not used by the kit",
                entry.id
            );
            assert!(
                credits.contains(&format!("| `{name}` |")),
                "{}: {name} is not in kits/CREDITS.md",
                entry.id
            );
        }
        assert!(
            credits.contains(&format!("`kits/{}`", entry.id)),
            "{} has no section in kits/CREDITS.md",
            entry.id
        );
    }
}
