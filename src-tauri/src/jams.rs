//! Jam files: the stock jams in the bundle's `jams/` and the user's in
//! `~/Music/Cantina/Jams`, a library folder watched like the others.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cantina_project::{jam_from_json, jam_to_json, Backing, Jam, JAM_SUFFIX};
use serde::Serialize;

use crate::library::{BrokenFile, Kind};

/// A jam the Jams list offers. Mirrors `JamInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamInfo {
    pub name: String,
    pub genre: String,
    pub description: String,
    /// Ships with Cantina: read-only.
    pub stock: bool,
    /// The file, which names the jam.
    pub path: String,
    pub tempo: f64,
    /// `A minor`.
    pub key: String,
}

fn info(jam: &Jam, path: &Path, stock: bool) -> JamInfo {
    JamInfo {
        name: jam.name.clone(),
        genre: jam.genre.clone(),
        description: jam.description.clone(),
        stock,
        path: path.display().to_string(),
        tempo: match &jam.backing {
            Backing::Audio { bpm, .. } => *bpm,
            _ => jam.tempo,
        },
        key: format!(
            "{} {}",
            jam.key.tonic,
            match jam.key.mode {
                cantina_project::Mode::Major => "major",
                cantina_project::Mode::Minor => "minor",
            }
        ),
    }
}

/// Reads and checks a jam file.
pub fn read(path: &Path) -> Result<Jam> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    jam_from_json(&text).map_err(|p| anyhow::anyhow!("{}", p.join("; ")))
}

/// Writes a jam file, whole or not at all.
pub fn write(path: &Path, jam: &Jam) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, jam_to_json(jam)).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The file a jam called `name` is saved as in `dir`.
pub fn path_for(dir: &Path, name: &str) -> PathBuf {
    let mut id = crate::pedals::slug(name);
    if id == "pedal" {
        id = "jam".into();
    }
    dir.join(format!("{id}{JAM_SUFFIX}"))
}

fn jam_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(JAM_SUFFIX))
        })
        .collect();
    out.sort();
    out
}

/// By genre, then name.
fn sort(jams: &mut [JamInfo]) {
    jams.sort_by(|a, b| {
        (a.genre.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.genre.to_lowercase(), b.name.to_lowercase()))
    });
}

/// The stock jams under `roots`; the first root to have a file of a name
/// wins. Files that do not load are left out: they are checked in CI.
pub fn scan_stock(roots: &[PathBuf]) -> Vec<JamInfo> {
    let mut found: Vec<JamInfo> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for root in roots {
        for path in jam_files(root) {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if file.as_ref().is_some_and(|f| names.contains(f)) {
                continue;
            }
            if let Ok(jam) = read(&path) {
                found.push(info(&jam, &path, true));
                names.extend(file);
            }
        }
    }
    sort(&mut found);
    found
}

/// The user's jams in `dir`, with the files that do not load listed in
/// `broken` with the reason. `Takes`, beside them, is not looked in.
pub fn scan_user(dir: &Path, broken: &mut Vec<BrokenFile>) -> Vec<JamInfo> {
    let mut found = Vec::new();
    for path in jam_files(dir) {
        match read(&path) {
            Ok(jam) => found.push(info(&jam, &path, false)),
            Err(e) => broken.push(BrokenFile {
                kind: Kind::Jam,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.display().to_string(),
                reason: format!("{e:#}"),
            }),
        }
    }
    sort(&mut found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../jams")
    }

    #[test]
    fn the_stock_jams_load_once_each_by_genre() {
        let jams = scan_stock(&[stock(), stock()]);
        assert!(jams.len() >= 15, "{}", jams.len());
        assert!(jams.iter().all(|j| j.stock));
        let mut names: Vec<&str> = jams.iter().map(|j| j.name.as_str()).collect();
        names.dedup();
        assert_eq!(names.len(), jams.len());
        assert!(jams
            .windows(2)
            .all(|w| w[0].genre.to_lowercase() <= w[1].genre.to_lowercase()));
        let blues = jams.iter().find(|j| j.name == "Blues in A").unwrap();
        assert_eq!((blues.key.as_str(), blues.tempo), ("A major", 96.0));
    }

    #[test]
    fn user_jams_round_trip_and_broken_ones_are_listed() {
        let dir = std::env::temp_dir().join(format!(
            "cantina-jams-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut jam = Jam::new("Late Night", 70.0, "jazz");
        jam.drums.groove = Some("Jazz".into());
        let path = path_for(&dir, &jam.name);
        assert!(path.ends_with("late-night.jam.json"));
        write(&path, &jam).unwrap();
        std::fs::create_dir_all(dir.join("Takes")).unwrap();
        std::fs::write(
            dir.join("bad.jam.json"),
            r#"{"version":1,"name":"","tempo":9}"#,
        )
        .unwrap();
        let mut broken = Vec::new();
        let found = scan_user(&dir, &mut broken);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].name.as_str(), found[0].stock),
            ("Late Night", false)
        );
        assert_eq!(read(&path).unwrap(), jam);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].kind, Kind::Jam);
        assert!(
            broken[0].reason.contains("not a jam"),
            "{}",
            broken[0].reason
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
