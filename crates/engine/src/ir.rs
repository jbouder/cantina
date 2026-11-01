//! Cabinet impulse responses on disk: WAV files found under a set of
//! roots, by file name.
//!
//! An impulse is any `.wav` directly under a root; its id is the file
//! stem and its name the stem with the separators turned into spaces.
//! Users drop their own files into `~/Music/Cantina/IRs`; the starter
//! set ships with the app.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::wav;

/// Longest impulse loaded; anything past this is cut.
pub const MAX_SECONDS: f32 = 3.0;

/// An impulse found on disk, not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// A loaded impulse, mono.
#[derive(Debug, Clone, PartialEq)]
pub struct Impulse {
    pub sample_rate: f32,
    pub samples: Vec<f32>,
}

fn name_for(stem: &str) -> String {
    stem.replace(['-', '_'], " ")
}

/// Every `.wav` directly under any of `roots`, by id. A root that does
/// not exist is simply empty. When two roots hold the same id the first
/// root wins, so user files can be listed before stock ones.
pub fn discover(roots: &[PathBuf]) -> Vec<IrEntry> {
    let mut found: Vec<IrEntry> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_wav = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            if !is_wav || !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|n| n.to_str()) else {
                continue;
            };
            if found.iter().any(|k| k.id == stem) {
                continue;
            }
            found.push(IrEntry {
                id: stem.to_string(),
                name: name_for(stem),
                path,
            });
        }
    }
    found.sort_by_key(|k| k.name.to_lowercase());
    found
}

/// Reads an impulse: the channels are averaged to mono and it is cut
/// at [`MAX_SECONDS`].
pub fn load(path: &Path) -> Result<Impulse> {
    let (info, channels) =
        wav::read_all(path).with_context(|| format!("reading {}", path.display()))?;
    if channels.is_empty() || channels[0].is_empty() {
        bail!("{} is empty", path.display());
    }
    let frames = channels[0]
        .len()
        .min((MAX_SECONDS * info.sample_rate as f32) as usize);
    let scale = 1.0 / channels.len() as f32;
    let samples = (0..frames)
        .map(|i| channels.iter().map(|c| c[i]).sum::<f32>() * scale)
        .collect();
    Ok(Impulse {
        sample_rate: info.sample_rate as f32,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::tests::tempdir;

    #[test]
    fn discovers_wavs_by_stem_first_root_winning() {
        let dir = tempdir();
        let stock = dir.join("stock");
        let user = dir.join("user");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        wav::write(&stock.join("4x12-modern.wav"), 48_000.0, &[vec![1.0, 0.5]]).unwrap();
        wav::write(&stock.join("1x12_open.WAV"), 48_000.0, &[vec![1.0]]).unwrap();
        wav::write(&user.join("4x12-modern.wav"), 44_100.0, &[vec![0.5]]).unwrap();
        std::fs::write(stock.join("notes.txt"), "x").unwrap();
        let found = discover(&[user.clone(), stock, dir.join("missing")]);
        let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["1x12_open", "4x12-modern"]);
        assert_eq!(found[0].name, "1x12 open");
        assert_eq!(found[1].path, user.join("4x12-modern.wav"));
        let imp = load(&found[1].path).unwrap();
        assert_eq!(imp.sample_rate, 44_100.0);
        assert_eq!(imp.samples, vec![0.5]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loads_mono_and_cuts_long_files() {
        let dir = tempdir();
        let path = dir.join("long.wav");
        let frames = (MAX_SECONDS * 1000.0) as usize + 500;
        wav::write(&path, 1000.0, &[vec![0.2; frames], vec![0.4; frames]]).unwrap();
        let imp = load(&path).unwrap();
        assert_eq!(imp.samples.len(), 3000);
        assert!((imp.samples[0] - 0.3).abs() < 1e-6);
        assert!(load(&dir.join("nope.wav")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
