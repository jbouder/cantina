//! Designed pedals on disk: the user's library (`~/Music/Cantina/Pedals`,
//! one `<id>.pedal.json` per pedal), the copies a project keeps in its
//! own `pedals/` directory, and single files for sharing.
//!
//! A pedal's id is its file stem, so a project that refers to
//! `pedal:warm-drive` finds it on another machine with the same file;
//! the project-local copy is what plays when the library has no such
//! pedal, or a different one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_dsp::PedalDef;

/// What a pedal file's name ends with.
pub const EXTENSION: &str = ".pedal.json";

/// A pedal in the library, not yet read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedalEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// The file name for an id.
pub fn file_name(id: &str) -> String {
    format!("{id}{EXTENSION}")
}

/// The id a file holds, if it is a pedal file.
pub fn id_of(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()?.strip_suffix(EXTENSION)
}

/// An id made from a name: lower case, letters, digits and dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "pedal".to_string()
    } else {
        out
    }
}

/// Reads and validates a pedal file.
pub fn read(path: &Path) -> Result<PedalDef> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    PedalDef::from_json(&text).with_context(|| format!("{}", path.display()))
}

/// Writes a pedal file, whole or not at all.
pub fn write(path: &Path, def: &PedalDef) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, def.to_json()).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Every pedal file directly under any of `roots`, by id. The first
/// root wins on a duplicate id. Files that do not load are reported
/// in `problems` and skipped.
pub fn discover(roots: &[PathBuf], problems: &mut Vec<String>) -> Vec<PedalEntry> {
    let mut found: Vec<PedalEntry> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = id_of(&path) else {
                continue;
            };
            if !path.is_file() || found.iter().any(|e| e.id == id) {
                continue;
            }
            match read(&path) {
                Ok(def) => found.push(PedalEntry {
                    id: id.to_string(),
                    name: def.name,
                    path,
                }),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }
    }
    found.sort_by_key(|e| e.name.to_lowercase());
    found
}

/// Every pedal file in a directory, by id: a project's own copies.
pub fn load_dir(dir: &Path, problems: &mut Vec<String>) -> HashMap<String, PedalDef> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = id_of(&path) else {
            continue;
        };
        match read(&path) {
            Ok(def) => {
                out.insert(id.to_string(), def);
            }
            Err(e) => problems.push(format!("{e:#}")),
        }
    }
    out
}

/// Copies a pedal file into the library, validated, under its own id
/// or the next free one when a different pedal already holds it.
/// Returns the entry it became.
pub fn import(path: &Path, library: &Path) -> Result<PedalEntry> {
    let def = read(path)?;
    let base = match id_of(path) {
        Some(id) => slug(id),
        None => {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("pedal");
            slug(stem.trim_end_matches(".pedal"))
        }
    };
    let mut id = base.clone();
    let mut n = 2;
    loop {
        let target = library.join(file_name(&id));
        if !target.exists() {
            write(&target, &def)?;
            break;
        }
        if read(&target).ok().as_ref() == Some(&def) {
            // The same pedal is already there.
            break;
        }
        id = format!("{base}-{n}");
        n += 1;
        if n > 1000 {
            bail!("too many pedals called {base}");
        }
    }
    Ok(PedalEntry {
        id: id.clone(),
        name: def.name,
        path: library.join(file_name(&id)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cantina-pedals-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(slug("Warm Drive"), "warm-drive");
        assert_eq!(slug("  Über  Fuzz!! 2 "), "ber-fuzz-2");
        assert_eq!(slug("---"), "pedal");
        assert_eq!(slug(""), "pedal");
        assert_eq!(file_name("warm-drive"), "warm-drive.pedal.json");
        assert_eq!(
            id_of(Path::new("/x/warm-drive.pedal.json")),
            Some("warm-drive")
        );
        assert_eq!(id_of(Path::new("/x/notes.json")), None);
    }

    #[test]
    fn files_round_trip_and_bad_ones_are_reported() {
        let dir = tempdir("rw");
        let def = cantina_dsp::stock_definition("overdrive").unwrap();
        let path = dir.join(file_name("od"));
        write(&path, def).unwrap();
        assert_eq!(&read(&path).unwrap(), def);
        std::fs::write(dir.join("broken.pedal.json"), "{ not json").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let mut problems = Vec::new();
        let found = discover(std::slice::from_ref(&dir), &mut problems);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "od");
        assert_eq!(found[0].name, "Overdrive");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("broken.pedal.json"), "{problems:?}");
        let loaded = load_dir(&dir, &mut Vec::new());
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key("od"));
        assert!(load_dir(&dir.join("nowhere"), &mut Vec::new()).is_empty());
    }

    #[test]
    fn importing_keeps_ids_free_and_skips_an_identical_copy() {
        let library = tempdir("lib");
        let elsewhere = tempdir("share");
        let od = cantina_dsp::stock_definition("overdrive").unwrap();
        let fuzz = cantina_dsp::stock_definition("fuzz").unwrap();
        let shared = elsewhere.join("My Drive.pedal.json");
        write(&shared, od).unwrap();
        let first = import(&shared, &library).unwrap();
        assert_eq!(first.id, "my-drive");
        assert!(library.join("my-drive.pedal.json").is_file());
        // The same file again is already there.
        assert_eq!(import(&shared, &library).unwrap().id, "my-drive");
        // A different pedal under the same name gets the next id.
        write(&shared, fuzz).unwrap();
        let second = import(&shared, &library).unwrap();
        assert_eq!(second.id, "my-drive-2");
        assert_eq!(read(&second.path).unwrap().name, "Fuzz");
        // Listing both folders: ids are file stems, sorted by name, the
        // library's copies first.
        let mut problems = Vec::new();
        let found = discover(&[library.clone(), elsewhere.clone()], &mut problems);
        assert_eq!(
            found.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["my-drive-2", "My Drive", "my-drive"]
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert!(import(&elsewhere.join("missing.pedal.json"), &library).is_err());
    }
}
