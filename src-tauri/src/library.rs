//! The library folders under `~/Music/Cantina`: `Pedals`, `Kits`,
//! `Patterns`, `Rigs`, `Jams` and `Exercises`, the documented places for files written outside the app,
//! by hand or by a coding agent.
//!
//! A watcher thread looks at the folders twice a second and, when what is
//! in them changes, the engine scans them again and pushes a status, so a
//! pedal written to `Pedals` shows up in the picker within a second, a kit
//! in the kit list, a pattern in the pattern menu and a rig in the rig
//! picker, without a restart.
//! A file that does not load is not dropped: it is listed as broken with
//! the reason, the same message `cantina-cli check` gives, and cannot be
//! picked.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cantina_dsp::{Kit, PedalDef};
use cantina_project::{pattern_from_json, Pattern, PatternId, Project, PATTERN_SUFFIX};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::engine::EngineState;
use crate::pedals;

/// How often the watcher looks.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(400);

/// Which kind of library file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Pedal,
    Kit,
    Pattern,
    Rig,
    Jam,
    Exercise,
}

/// A library file that does not load. Mirrors `BrokenFile` in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenFile {
    pub kind: Kind,
    /// The file or folder name, as the list shows it.
    pub name: String,
    pub path: String,
    /// What is wrong, in words.
    pub reason: String,
}

/// A pattern file in the library. Mirrors `LibraryPattern` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPattern {
    /// The pattern's own name.
    pub name: String,
    pub path: String,
    pub steps_per_beat: u8,
    pub length: u16,
}

/// What the library folders hold beyond what the pedal picker and kit
/// list already show. Mirrors `LibraryStatus` in the UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    pub pedals_dir: String,
    pub kits_dir: String,
    pub patterns_dir: String,
    pub rigs_dir: String,
    pub jams_dir: String,
    pub exercises_dir: String,
    /// The patterns in `Patterns`, by name.
    pub patterns: Vec<LibraryPattern>,
    /// The rigs in `Rigs`; the status lists them with the stock ones.
    #[serde(skip)]
    pub rigs: Vec<crate::rigs::RigInfo>,
    /// The jams in `Jams`; the status lists them with the stock ones.
    #[serde(skip)]
    pub jams: Vec<crate::jams::JamInfo>,
    /// The exercises in `Exercises`; the status lists them with the
    /// stock ones.
    #[serde(skip)]
    pub exercises: Vec<crate::exercises::ExerciseInfo>,
    /// Files in any of the folders that do not load.
    pub broken: Vec<BrokenFile>,
}

/// The library folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    pub pedals: PathBuf,
    pub kits: PathBuf,
    pub patterns: PathBuf,
    pub rigs: PathBuf,
    pub jams: PathBuf,
    pub exercises: PathBuf,
}

impl Folders {
    pub fn all(&self) -> [&Path; 6] {
        [
            &self.pedals,
            &self.kits,
            &self.patterns,
            &self.rigs,
            &self.jams,
            &self.exercises,
        ]
    }

    /// Reads the folders: the pattern files and every broken file.
    pub fn scan(&self) -> LibraryStatus {
        let mut broken = Vec::new();
        broken_pedals(&self.pedals, &mut broken);
        broken_kits(&self.kits, &mut broken);
        let patterns = scan_patterns(&self.patterns, &mut broken);
        let rigs = crate::rigs::scan_user(&self.rigs, &mut broken);
        let jams = crate::jams::scan_user(&self.jams, &mut broken);
        let exercises = crate::exercises::scan_user(&self.exercises, &mut broken);
        broken.sort_by_key(|b| b.name.to_lowercase());
        LibraryStatus {
            pedals_dir: self.pedals.display().to_string(),
            kits_dir: self.kits.display().to_string(),
            patterns_dir: self.patterns.display().to_string(),
            rigs_dir: self.rigs.display().to_string(),
            jams_dir: self.jams.display().to_string(),
            exercises_dir: self.exercises.display().to_string(),
            patterns,
            rigs,
            jams,
            exercises,
            broken,
        }
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn broken(kind: Kind, path: &Path, reason: String) -> BrokenFile {
    BrokenFile {
        kind,
        name: file_name(path),
        path: path.display().to_string(),
        reason,
    }
}

fn broken_pedals(dir: &Path, out: &mut Vec<BrokenFile>) {
    for path in entries(dir) {
        if !path.is_file() || pedals::id_of(&path).is_none() {
            continue;
        }
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| PedalDef::from_json(&text).map_err(|e| e.to_string()));
        if let Err(reason) = result {
            out.push(broken(Kind::Pedal, &path, reason));
        }
    }
}

/// A kit is broken when its `kit.json` does not read or a sample it
/// names is not in the folder.
pub fn kit_problem(dir: &Path) -> Option<String> {
    let json = dir.join(cantina_engine::kit::KIT_FILE);
    let text = match std::fs::read_to_string(&json) {
        Ok(t) => t,
        Err(e) => return Some(format!("reading kit.json: {e}")),
    };
    let kit = match Kit::from_json(&text) {
        Ok(k) => k,
        Err(e) => return Some(e.to_string()),
    };
    for pad in &kit.pads {
        for layer in &pad.layers {
            if !dir.join(&layer.file).is_file() {
                return Some(format!(
                    "pad {:?} names {:?}, which is not in the folder",
                    pad.id, layer.file
                ));
            }
        }
    }
    None
}

fn broken_kits(dir: &Path, out: &mut Vec<BrokenFile>) {
    for path in entries(dir) {
        if !path.join(cantina_engine::kit::KIT_FILE).is_file() {
            continue;
        }
        if let Some(reason) = kit_problem(&path) {
            out.push(broken(Kind::Kit, &path, reason));
        }
    }
}

fn scan_patterns(dir: &Path, out: &mut Vec<BrokenFile>) -> Vec<LibraryPattern> {
    let mut found = Vec::new();
    for path in entries(dir) {
        if !path.is_file() || !file_name(&path).ends_with(PATTERN_SUFFIX) {
            continue;
        }
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| pattern_from_json(&text));
        match result {
            Ok(p) => found.push(LibraryPattern {
                name: p.name,
                path: path.display().to_string(),
                steps_per_beat: p.steps_per_beat,
                length: p.length,
            }),
            Err(reason) => out.push(broken(Kind::Pattern, &path, reason)),
        }
    }
    found.sort_by_key(|p| p.name.to_lowercase());
    found
}

/// A stock groove: a pattern file that ships with the app, read-only,
/// with what it is for. Mirrors `StockGroove` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockGroove {
    pub name: String,
    pub path: String,
    pub steps_per_beat: u8,
    pub length: u16,
    #[serde(flatten)]
    pub groove: cantina_project::Groove,
}

/// The stock grooves under `roots`, one folder per genre, by genre and
/// then by section. The first root to have a groove of a name wins.
/// Files that do not load are left out: they are checked in CI.
pub fn scan_grooves(roots: &[PathBuf]) -> Vec<StockGroove> {
    let mut found: Vec<StockGroove> = Vec::new();
    for root in roots {
        for genre in entries(root).into_iter().filter(|p| p.is_dir()) {
            for path in entries(&genre) {
                if !file_name(&path).ends_with(PATTERN_SUFFIX) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok((pattern, Some(groove))) = cantina_project::groove_from_json(&text) else {
                    continue;
                };
                if found.iter().any(|g| g.name == pattern.name) {
                    continue;
                }
                found.push(StockGroove {
                    name: pattern.name,
                    path: path.display().to_string(),
                    steps_per_beat: pattern.steps_per_beat,
                    length: pattern.length,
                    groove,
                });
            }
        }
    }
    found.sort_by(|a, b| {
        (a.groove.genre.to_lowercase(), a.groove.section, &a.name).cmp(&(
            b.groove.genre.to_lowercase(),
            b.groove.section,
            &b.name,
        ))
    });
    found
}

/// A fingerprint of what the folders hold: every entry's name, size and
/// modification time, and one level into each kit folder. Equal
/// fingerprints mean nothing worth a rescan has happened.
pub fn fingerprint(folders: &[&Path]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut add = |path: &Path| {
        path.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(path) {
            meta.len().hash(&mut h);
            if let Ok(t) = meta.modified() {
                t.hash(&mut h);
            }
        }
    };
    for folder in folders {
        for path in entries(folder) {
            add(&path);
            if path.is_dir() {
                for inner in entries(&path) {
                    add(&inner);
                }
            }
        }
    }
    h.finish()
}

/// Watches the folders on a thread of its own for as long as the app
/// runs, asking the engine to rescan when they change.
pub fn spawn_watcher(app: AppHandle, folders: Folders) {
    std::thread::Builder::new()
        .name("library-watch".into())
        .spawn(move || {
            let mut last = fingerprint(&folders.all());
            loop {
                std::thread::sleep(WATCH_INTERVAL);
                let now = fingerprint(&folders.all());
                if now != last {
                    last = now;
                    app.state::<EngineState>().lock().library_rescan();
                }
            }
        })
        .expect("the library watcher thread starts");
}

/// A pattern file read for import into `project`: a fresh id, renamed on
/// a clash, and the pads its rows name that `kit_pads` lacks (kept, so
/// they play on another kit, and named in a notice).
pub fn import_pattern(
    text: &str,
    project: &Project,
    kit_pads: Option<&[String]>,
) -> Result<(Pattern, Vec<String>), String> {
    let pattern = pattern_from_json(text)?;
    let taken: Vec<&str> = project.patterns.iter().map(|p| p.name.as_str()).collect();
    let pattern = cantina_project::imported(pattern, PatternId(project.fresh_id()), &taken);
    let missing = match kit_pads {
        Some(pads) => pattern
            .rows
            .iter()
            .filter(|r| !pads.contains(&r.pad))
            .map(|r| r.pad.clone())
            .collect(),
        None => Vec::new(),
    };
    Ok((pattern, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cantina-library-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn folders(root: &Path) -> Folders {
        let f = Folders {
            pedals: root.join("Pedals"),
            kits: root.join("Kits"),
            patterns: root.join("Patterns"),
            rigs: root.join("Rigs"),
            jams: root.join("Jams"),
            exercises: root.join("Exercises"),
        };
        for d in f.all() {
            std::fs::create_dir_all(d).unwrap();
        }
        f
    }

    const BEAT: &str = r#"{"name":"Beat","stepsPerBeat":4,"length":2,"swing":0,
        "rows":[{"pad":"kick","steps":[1,0]},{"pad":"cowbell","steps":[0,1]}]}"#;

    #[test]
    fn broken_files_are_listed_with_the_reason() {
        let root = tempdir("broken");
        let f = folders(&root);
        std::fs::write(f.pedals.join("bad.pedal.json"), "{").unwrap();
        std::fs::write(
            f.pedals.join("ok.pedal.json"),
            cantina_dsp::stock_definition("fuzz").unwrap().to_json(),
        )
        .unwrap();
        std::fs::write(f.pedals.join("notes.txt"), "ignored").unwrap();
        let kit = f.kits.join("thin");
        std::fs::create_dir(&kit).unwrap();
        std::fs::write(
            kit.join("kit.json"),
            r#"{"version":1,"name":"Thin","pads":[{"id":"kick","layers":[{"file":"k.wav"}]}]}"#,
        )
        .unwrap();
        std::fs::write(f.patterns.join("beat.pattern.json"), BEAT).unwrap();
        std::fs::write(
            f.patterns.join("short.pattern.json"),
            BEAT.replace("[1,0]", "[1]"),
        )
        .unwrap();
        let status = f.scan();
        let summary: Vec<(Kind, &str)> = status
            .broken
            .iter()
            .map(|b| (b.kind, b.name.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (Kind::Pedal, "bad.pedal.json"),
                (Kind::Pattern, "short.pattern.json"),
                (Kind::Kit, "thin"),
            ]
        );
        assert!(
            status.broken[1].reason.contains("has 1 steps"),
            "{status:?}"
        );
        assert!(status.broken[2].reason.contains("\"k.wav\""), "{status:?}");
        assert_eq!(status.patterns.len(), 1);
        assert_eq!(status.patterns[0].name, "Beat");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_stock_grooves_are_found_by_genre_and_section() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../patterns");
        let grooves = scan_grooves(&[repo.clone(), repo]);
        assert!(grooves.len() >= 60, "{}", grooves.len());
        let names: std::collections::HashSet<&str> =
            grooves.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names.len(), grooves.len(), "a second root adds nothing new");
        let rock: Vec<&StockGroove> = grooves
            .iter()
            .filter(|g| g.groove.genre == "Rock")
            .collect();
        assert_eq!(rock[0].groove.section, cantina_project::Section::Intro);
        assert!(rock.iter().any(|g| g.name == "Rock verse"));
        let json = serde_json::to_value(rock[0]).unwrap();
        assert_eq!(json["genre"], "Rock");
        assert_eq!(json["timeSignature"]["numerator"], 4);
    }

    #[test]
    fn the_fingerprint_moves_when_a_file_is_written() {
        let root = tempdir("print");
        let f = folders(&root);
        let a = fingerprint(&f.all());
        assert_eq!(a, fingerprint(&f.all()));
        std::fs::write(f.patterns.join("beat.pattern.json"), BEAT).unwrap();
        let b = fingerprint(&f.all());
        assert_ne!(a, b);
        std::fs::write(
            f.patterns.join("beat.pattern.json"),
            BEAT.replace("Beat", "Beats"),
        )
        .unwrap();
        assert_ne!(b, fingerprint(&f.all()), "a size change shows");
        // A sample added inside a kit folder shows too.
        std::fs::create_dir(f.kits.join("k")).unwrap();
        let c = fingerprint(&f.all());
        std::fs::write(f.kits.join("k/k.wav"), b"x").unwrap();
        assert_ne!(c, fingerprint(&f.all()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_take_a_fresh_id_rename_and_name_missing_pads() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns.push(Pattern::new(PatternId(4), "Beat"));
        let pads = vec!["kick".to_string(), "snare".to_string()];
        let (p, missing) = import_pattern(BEAT, &project, Some(&pads)).unwrap();
        assert_eq!(p.id, PatternId(5));
        assert_eq!(p.name, "Beat 2");
        assert_eq!(p.rows.len(), 2, "rows for missing pads are kept");
        assert_eq!(missing, ["cowbell"]);
        let (_, none) = import_pattern(BEAT, &project, None).unwrap();
        assert!(none.is_empty());
        assert!(import_pattern("{}", &project, None).is_err());
    }
}
