//! Rigs on disk: the stock rigs that ship with the app (`rigs/` in the
//! bundle's resources, read-only) and the user's, `~/Music/Cantina/Rigs`,
//! one `<id>.rig.json` per rig, a watched library folder like the others.
//!
//! The picker lists both. A rig is named by its file's path, so a user
//! rig with a stock rig's name is a rig of its own. Only the user's can
//! be renamed or deleted.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_project::{rig_from_json, rig_to_json, Instrument, Rig, RIG_SUFFIX};
use serde::Serialize;

use crate::library::{BrokenFile, Kind};
use crate::pedals::slug;

/// A rig the picker offers. Mirrors `RigInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RigInfo {
    pub name: String,
    pub genre: String,
    pub description: String,
    /// `guitar` or `bass`.
    pub instrument: &'static str,
    /// Ships with Cantina: read-only.
    pub stock: bool,
    /// The file, which names the rig.
    pub path: String,
    /// The pedals, by spec, first to last.
    pub chain: Vec<String>,
}

/// The name the UI shows for an instrument.
pub fn instrument_name(i: Instrument) -> &'static str {
    match i {
        Instrument::Guitar => "guitar",
        Instrument::Bass => "bass",
    }
}

/// Reads and checks a rig file.
pub fn read(path: &Path) -> Result<Rig> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    rig_from_json(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// What is wrong with a rig's stock pedals: a name that is not a stock
/// pedal, or a parameter name or value it does not take. Cabinets,
/// designed pedals and plugins depend on what is installed and are
/// checked when the rig is heard or applied.
pub fn stock_problems(rig: &Rig) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, p) in rig.chain.iter().enumerate() {
        let spec = p.pedal.as_str();
        if spec.starts_with("cab:") || spec.starts_with("pedal:") || p.is_plugin() {
            continue;
        }
        let at = format!("chain {i} ({spec})");
        let Some(processor) = cantina_dsp::stock(spec) else {
            problems.push(format!("{at}: {spec:?} is not a stock pedal"));
            continue;
        };
        let slots =
            cantina_cli::rig::slots_for(processor.params(), cantina_dsp::stock_definition(spec));
        if let Err(p) = cantina_project::resolve_values(p, &slots) {
            problems.extend(p.into_iter().map(|p| format!("{at}: {p}")));
        }
    }
    problems
}

/// Writes a rig file, whole or not at all.
pub fn write(path: &Path, rig: &Rig) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, rig_to_json(rig)).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The file a rig called `name` is saved as in `dir`.
pub fn path_for(dir: &Path, name: &str) -> PathBuf {
    let mut id = slug(name);
    if id == "pedal" {
        id = "rig".into();
    }
    dir.join(format!("{id}{RIG_SUFFIX}"))
}

/// A path in `dir` for `name` that no file has yet: `name-2`, `name-3`…
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = path_for(dir, name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| path_for(dir, &format!("{name} {n}")))
        .find(|p| !p.exists())
        .expect("some number is free")
}

fn info(rig: &Rig, path: &Path, stock: bool) -> RigInfo {
    RigInfo {
        name: rig.name.clone(),
        genre: rig.genre.clone(),
        description: rig.description.clone(),
        instrument: instrument_name(rig.instrument),
        stock,
        path: path.display().to_string(),
        chain: rig.chain.iter().map(|p| p.pedal.clone()).collect(),
    }
}

fn rig_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(RIG_SUFFIX))
        })
        .collect();
    out.sort();
    out
}

/// The stock rigs under `roots`; the first root to have a file of a name
/// wins. Files that do not load are left out: they are checked in CI.
pub fn scan_stock(roots: &[PathBuf]) -> Vec<RigInfo> {
    let mut found: Vec<RigInfo> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for root in roots {
        for path in rig_files(root) {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if file.as_ref().is_some_and(|f| names.contains(f)) {
                continue;
            }
            if let Ok(rig) = read(&path) {
                found.push(info(&rig, &path, true));
                names.extend(file);
            }
        }
    }
    sort(&mut found);
    found
}

/// The user's rigs in `dir`, with the files that do not load listed in
/// `broken` with the reason.
pub fn scan_user(dir: &Path, broken: &mut Vec<BrokenFile>) -> Vec<RigInfo> {
    let mut found = Vec::new();
    for path in rig_files(dir) {
        let checked = read(&path).and_then(|rig| match stock_problems(&rig).as_slice() {
            [] => Ok(rig),
            problems => bail!("{}", problems.join("; ")),
        });
        match checked {
            Ok(rig) => found.push(info(&rig, &path, false)),
            Err(e) => broken.push(BrokenFile {
                kind: Kind::Rig,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.display().to_string(),
                reason: format!("{e:#}")
                    .trim_start_matches(&format!("{}: ", path.display()))
                    .to_string(),
            }),
        }
    }
    sort(&mut found);
    found
}

/// Guitar before bass, then by genre and name.
pub fn sort(rigs: &mut [RigInfo]) {
    rigs.sort_by(|a, b| {
        (
            a.instrument != "guitar",
            a.genre.to_lowercase(),
            a.name.to_lowercase(),
        )
            .cmp(&(
                b.instrument != "guitar",
                b.genre.to_lowercase(),
                b.name.to_lowercase(),
            ))
    });
}

/// A user rig's path, checked to be one: in `dir` and a rig file.
pub fn user_path(dir: &Path, path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    let inside = path
        .parent()
        .zip(dir.canonicalize().ok())
        .and_then(|(p, d)| p.canonicalize().ok().map(|p| p == d))
        .unwrap_or(false);
    if !inside {
        bail!("only your own rigs, in {}, can be changed", dir.display());
    }
    if !path.is_file() {
        bail!("there is no rig at {}", path.display());
    }
    Ok(path)
}

/// Renames a user rig: its name, and its file to match. Refuses a name
/// another rig's file already has.
pub fn rename(dir: &Path, path: &str, name: &str) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        bail!("a rig needs a name");
    }
    let from = user_path(dir, path)?;
    let mut rig = read(&from)?;
    rig.name = name.to_string();
    let to = path_for(dir, name);
    if to != from && to.exists() {
        bail!(
            "there is already a rig file called {}",
            to.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    write(&to, &rig)?;
    if to != from {
        std::fs::remove_file(&from).with_context(|| format!("removing {}", from.display()))?;
    }
    Ok(to)
}

/// Deletes a user rig.
pub fn delete(dir: &Path, path: &str) -> Result<()> {
    let path = user_path(dir, path)?;
    std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))
}

/// Copies a rig file into `dir` after checking it reads, under its name,
/// numbered when that is taken. Returns where it went.
pub fn import(dir: &Path, from: &Path) -> Result<PathBuf> {
    let rig = read(from)?;
    let to = free_path(dir, &rig.name);
    write(&to, &rig)?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cantina-rigs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn repo_rigs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rigs")
    }

    #[test]
    fn the_stock_rigs_are_found_guitar_first() {
        let rigs = scan_stock(&[repo_rigs(), repo_rigs()]);
        assert!(rigs.len() >= 19, "{}", rigs.len());
        assert!(rigs.iter().filter(|r| r.instrument == "bass").count() >= 4);
        let first_bass = rigs.iter().position(|r| r.instrument == "bass").unwrap();
        assert!(rigs[first_bass..].iter().all(|r| r.instrument == "bass"));
        assert!(rigs.iter().all(|r| r.stock && !r.description.is_empty()));
        for r in &rigs {
            let rig = read(Path::new(&r.path)).unwrap();
            assert_eq!(stock_problems(&rig), Vec::<String>::new(), "{}", r.name);
        }
        let names: std::collections::HashSet<&str> = rigs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), rigs.len(), "a second root adds nothing new");
    }

    #[test]
    fn user_rigs_are_listed_and_broken_ones_say_why() {
        let dir = tempdir("scan");
        std::fs::copy(
            repo_rigs().join("blues.rig.json"),
            dir.join("blues.rig.json"),
        )
        .unwrap();
        std::fs::write(
            dir.join("bad.rig.json"),
            r#"{ "version": 1, "name": "", "instrument": "guitar", "chain": [] }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("loud.rig.json"),
            r#"{ "version": 1, "name": "Loud", "instrument": "guitar",
                "chain": [ { "pedal": "cab:anything" }, { "pedal": "amp", "params": { "volume": 11 } } ] }"#,
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a rig").unwrap();
        let mut broken = Vec::new();
        let rigs = scan_user(&dir, &mut broken);
        assert_eq!(rigs.len(), 1);
        assert!(!rigs[0].stock);
        assert_eq!(broken.len(), 2);
        assert_eq!(broken[0].name, "bad.rig.json");
        assert!(
            broken[0].reason.contains("the name is empty"),
            "{}",
            broken[0].reason
        );
        assert_eq!(broken[1].name, "loud.rig.json");
        assert!(
            broken[1]
                .reason
                .starts_with("chain 1 (amp): amp has no parameter \"volume\""),
            "{}",
            broken[1].reason
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_delete_and_import_touch_only_user_rigs() {
        let dir = tempdir("edit");
        let imported = import(&dir, &repo_rigs().join("punk.rig.json")).unwrap();
        assert_eq!(imported, dir.join("punk.rig.json"));
        let again = import(&dir, &repo_rigs().join("punk.rig.json")).unwrap();
        assert_eq!(again, dir.join("punk-2.rig.json"));

        let renamed = rename(&dir, &imported.display().to_string(), "My Punk").unwrap();
        assert_eq!(renamed, dir.join("my-punk.rig.json"));
        assert!(!imported.exists());
        assert_eq!(read(&renamed).unwrap().name, "My Punk");
        let clash = rename(&dir, &again.display().to_string(), "my punk");
        assert!(clash.unwrap_err().to_string().contains("already"));

        let stock = repo_rigs().join("punk.rig.json").display().to_string();
        assert!(delete(&dir, &stock).is_err());
        assert!(rename(&dir, &stock, "Mine").is_err());
        assert!(repo_rigs().join("punk.rig.json").exists());

        delete(&dir, &renamed.display().to_string()).unwrap();
        assert!(!renamed.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
