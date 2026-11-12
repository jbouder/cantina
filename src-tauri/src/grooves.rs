//! Groove files of the user's: whole drum parts the drum designer saved
//! to `~/Music/Cantina/Grooves`, a library folder watched like the others.
//! The stock grooves stay pattern files, one per section, read by
//! [`crate::library::scan_grooves`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cantina_project::{
    groove_file_from_json, groove_file_to_json, GrooveFile, Meter, Section, GROOVE_SUFFIX,
};
use serde::Serialize;

use crate::library::{BrokenFile, Kind};

/// A section of a groove, as the palette lists it. Mirrors
/// `GrooveSectionInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveSectionInfo {
    pub name: String,
    pub kind: Option<Section>,
}

/// A groove of the user's. Mirrors `GrooveInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveInfo {
    /// What jams name it by.
    pub name: String,
    pub genre: String,
    pub description: String,
    /// The file.
    pub path: String,
    pub tempo: f64,
    pub time_signature: Meter,
    pub kit: String,
    pub sections: Vec<GrooveSectionInfo>,
    /// Bars in its order.
    pub bars: u64,
}

fn info(g: &GrooveFile, path: &Path) -> GrooveInfo {
    GrooveInfo {
        name: g.name.clone(),
        genre: g.genre.clone(),
        description: g.description.clone(),
        path: path.display().to_string(),
        tempo: g.tempo,
        time_signature: g.time_signature,
        kit: g.part.kit.clone(),
        sections: g
            .part
            .sections
            .iter()
            .map(|s| GrooveSectionInfo {
                name: s.name.clone(),
                kind: s.kind,
            })
            .collect(),
        bars: g.part.order_bars(g.bar_ticks()),
    }
}

/// Reads and checks a groove file.
pub fn read(path: &Path) -> Result<GrooveFile> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    groove_file_from_json(&text).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Writes a groove file, whole or not at all.
pub fn write(path: &Path, groove: &GrooveFile) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, groove_file_to_json(groove))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The file a groove called `name` is saved as in `dir`.
pub fn path_for(dir: &Path, name: &str) -> PathBuf {
    let mut id = crate::pedals::slug(name);
    if id == "pedal" {
        id = "groove".into();
    }
    dir.join(format!("{id}{GROOVE_SUFFIX}"))
}

fn groove_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(GROOVE_SUFFIX))
        })
        .collect();
    out.sort();
    out
}

/// The user's grooves in `dir`, by name, with the files that do not load
/// listed in `broken` with the reason.
pub fn scan_user(dir: &Path, broken: &mut Vec<BrokenFile>) -> Vec<GrooveInfo> {
    let mut found = Vec::new();
    for path in groove_files(dir) {
        match read(&path) {
            Ok(g) => found.push(info(&g, &path)),
            Err(e) => broken.push(BrokenFile {
                kind: Kind::Groove,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.display().to_string(),
                reason: format!("{e:#}"),
            }),
        }
    }
    found.sort_by_key(|g| g.name.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder of its own under the system's temporary one.
    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cantina-grooves-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn example() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../.claude/skills/cantina-author/examples/late-train.groove.json"),
        )
        .unwrap()
    }

    #[test]
    fn the_users_grooves_are_listed_and_a_broken_one_says_why() {
        let dir = temp("scan");
        std::fs::write(dir.join("late-train.groove.json"), example()).unwrap();
        std::fs::write(dir.join("bad.groove.json"), "{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let mut broken = Vec::new();
        let found = scan_user(&dir, &mut broken);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Late train");
        assert_eq!(found[0].sections.len(), 5);
        assert_eq!(found[0].bars, 35);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].kind, Kind::Groove);
    }

    #[test]
    fn a_groove_written_reads_back() {
        let dir = temp("write");
        let g = groove_file_from_json(&example()).unwrap();
        let path = path_for(&dir, "Late train!");
        assert!(
            path.ends_with("late-train.groove.json"),
            "{}",
            path.display()
        );
        write(&path, &g).unwrap();
        assert_eq!(read(&path).unwrap(), g);
    }
}
