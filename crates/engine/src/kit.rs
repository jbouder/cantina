//! Loads drum kits from disk: the kit's JSON and every sample it names,
//! into a [`LoadedKit`] the sampler plays.
//!
//! A kit is a folder holding `kit.json` and its samples:
//!
//! ```text
//! kits/
//!   acoustic/
//!     kit.json
//!     kick-soft.wav
//!     kick-hard.wav
//!     ...
//! ```
//!
//! The folder's name is the kit's id, which is what a project records.
//! [`discover`] lists the kits under a set of folders; [`load`] reads
//! one. Samples are WAV, mono or stereo, at any rate: the sampler
//! resamples to the engine's rate as it plays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use cantina_dsp::{Kit, LoadedKit, Sample};

use crate::wav;

/// The file a kit folder must hold.
pub const KIT_FILE: &str = "kit.json";

/// A kit found by [`discover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitEntry {
    /// The folder name.
    pub id: String,
    /// The name inside the file.
    pub name: String,
    /// The folder.
    pub path: PathBuf,
}

/// Reads a kit folder: the JSON and every sample, with each file read
/// once however many layers use it.
pub fn load(dir: &Path) -> Result<(Kit, LoadedKit)> {
    let json_path = dir.join(KIT_FILE);
    let text = std::fs::read_to_string(&json_path)
        .with_context(|| format!("reading {}", json_path.display()))?;
    let kit = Kit::from_json(&text).with_context(|| format!("in {}", json_path.display()))?;
    let mut cache: HashMap<String, Arc<Sample>> = HashMap::new();
    let loaded = LoadedKit::build(&kit, |file| {
        if let Some(sample) = cache.get(file) {
            return Ok(Arc::clone(sample));
        }
        let path = dir.join(file);
        let (info, channels) = wav::read_all(&path).map_err(|e| format!("{e:#}"))?;
        let sample = Arc::new(Sample {
            sample_rate: info.sample_rate as f64,
            channels,
        });
        cache.insert(file.to_string(), Arc::clone(&sample));
        Ok(sample)
    })
    .with_context(|| format!("loading {}", json_path.display()))?;
    Ok((kit, loaded))
}

/// Every kit folder directly under any of `roots`, by id. A folder whose
/// `kit.json` does not read is skipped with its error in `problems`;
/// a root that does not exist is simply empty. When two roots hold the
/// same id the first root wins, so user kits can be listed before stock
/// ones to override them.
pub fn discover(roots: &[PathBuf], problems: &mut Vec<String>) -> Vec<KitEntry> {
    let mut found: Vec<KitEntry> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let json = path.join(KIT_FILE);
            if !json.is_file() {
                continue;
            }
            let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if found.iter().any(|k| k.id == id) {
                continue;
            }
            let name = match std::fs::read_to_string(&json)
                .map_err(|e| e.to_string())
                .and_then(|text| Kit::from_json(&text).map_err(|e| e.to_string()))
            {
                Ok(kit) => kit.name,
                Err(e) => {
                    problems.push(format!("{}: {e}", json.display()));
                    continue;
                }
            };
            found.push(KitEntry {
                id: id.to_string(),
                name,
                path,
            });
        }
    }
    found.sort_by_key(|k| k.name.to_lowercase());
    found
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::wav::tests::tempdir;

    /// Writes a small kit with two pads and a shared sample into `dir`
    /// and returns its folder.
    pub(crate) fn write_test_kit(dir: &Path, id: &str) -> PathBuf {
        let kit_dir = dir.join(id);
        std::fs::create_dir_all(&kit_dir).unwrap();
        let kick: Vec<f32> = (0..100).map(|i| 1.0 - i as f32 / 100.0).collect();
        wav::write(&kit_dir.join("kick.wav"), 48_000.0, &[kick]).unwrap();
        let hat: Vec<f32> = (0..50)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        wav::write(&kit_dir.join("hat.wav"), 44_100.0, &[hat.clone(), hat]).unwrap();
        std::fs::write(
            kit_dir.join(KIT_FILE),
            format!(
                r#"{{"version":1,"name":"{id} kit","pads":[
                  {{"id":"kick","layers":[{{"file":"kick.wav"}}]}},
                  {{"id":"hh-closed","choke":"hat","layers":[{{"file":"hat.wav"}}]}},
                  {{"id":"hh-open","choke":"hat","layers":[{{"file":"hat.wav"}}]}}
                ]}}"#
            ),
        )
        .unwrap();
        kit_dir
    }

    #[test]
    fn loads_a_kit_folder_reading_each_file_once() {
        let dir = tempdir();
        let kit_dir = write_test_kit(&dir, "test");
        let (kit, loaded) = load(&kit_dir).unwrap();
        assert_eq!(kit.name, "test kit");
        assert_eq!(loaded.pads().len(), 3);
        assert_eq!(loaded.pads()[0].layers[0].sample.frames(), 100);
        assert_eq!(loaded.pads()[1].layers[0].sample.sample_rate, 44_100.0);
        assert_eq!(loaded.pads()[1].layers[0].sample.channels.len(), 2);
        assert!(
            Arc::ptr_eq(
                &loaded.pads()[1].layers[0].sample,
                &loaded.pads()[2].layers[0].sample
            ),
            "the shared sample is one allocation"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_samples_and_bad_json_are_reported() {
        let dir = tempdir();
        let kit_dir = write_test_kit(&dir, "broken");
        std::fs::remove_file(kit_dir.join("hat.wav")).unwrap();
        let err = load(&kit_dir).unwrap_err();
        assert!(format!("{err:#}").contains("hat.wav"), "{err:#}");
        std::fs::write(kit_dir.join(KIT_FILE), "{").unwrap();
        let err = load(&kit_dir).unwrap_err();
        assert!(format!("{err:#}").contains("not a kit file"), "{err:#}");
        assert!(load(&dir.join("nowhere")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn discovers_kits_across_roots_with_the_first_root_winning() {
        let dir = tempdir();
        let stock = dir.join("stock");
        let user = dir.join("user");
        write_test_kit(&stock, "acoustic");
        write_test_kit(&stock, "rock");
        write_test_kit(&user, "acoustic");
        write_test_kit(&user, "junk");
        std::fs::write(user.join("junk").join(KIT_FILE), "nope").unwrap();
        std::fs::create_dir_all(stock.join("not-a-kit")).unwrap();
        let mut problems = Vec::new();
        let kits = discover(
            &[user.clone(), stock.clone(), dir.join("missing")],
            &mut problems,
        );
        let ids: Vec<&str> = kits.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, ["acoustic", "rock"]);
        assert_eq!(kits[0].path, user.join("acoustic"), "the user kit wins");
        assert_eq!(kits[0].name, "acoustic kit");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("junk"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
