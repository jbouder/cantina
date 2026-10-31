//! The project on disk.
//!
//! A project is a directory, `Name.cantina/`:
//!
//! ```text
//! Name.cantina/
//!   project.json    the model, see model.rs
//!   audio/          recorded and imported WAV files, never modified
//!   plugins/        third-party plugin state blobs
//!   pedals/         user pedal definitions used by this project
//! ```
//!
//! `project.json` carries a `version`. [`load`] upgrades an older file in
//! memory through the migrations below, one version step at a time, and
//! the next [`save`] writes the current version. A file from a newer
//! Cantina is refused rather than guessed at. Saves go through a
//! temporary file and a rename, so a crash mid-save leaves the old file
//! intact.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use crate::model::{Project, FORMAT_VERSION};

/// Extension of a project directory.
pub const EXTENSION: &str = "cantina";
pub const PROJECT_FILE: &str = "project.json";
pub const AUDIO_DIR: &str = "audio";
pub const PLUGINS_DIR: &str = "plugins";
pub const PEDALS_DIR: &str = "pedals";

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("{path} is not a project: no {PROJECT_FILE} inside it")]
    NotAProject { path: PathBuf },
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not valid JSON: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{path} has no numeric \"version\" field")]
    NoVersion { path: PathBuf },
    #[error("{path} is a version {found} project; this Cantina reads up to version {supported}")]
    TooNew {
        path: PathBuf,
        found: u64,
        supported: u32,
    },
    #[error("{path} does not match the project format: {source}")]
    Shape {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{path} is not a usable project: {why}")]
    Invalid { path: PathBuf, why: String },
}

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("creating {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("encoding the project: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("not saving an unusable project: {0}")]
    Invalid(String),
}

/// `Name.cantina` under `parent`.
pub fn dir_for(parent: &Path, name: &str) -> PathBuf {
    parent.join(format!("{name}.{EXTENSION}"))
}

pub fn audio_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(AUDIO_DIR)
}

/// Creates the directory layout. Fine to call on an existing project.
pub fn create_layout(project_dir: &Path) -> Result<(), SaveError> {
    for sub in ["", AUDIO_DIR, PLUGINS_DIR, PEDALS_DIR] {
        let path = project_dir.join(sub);
        fs::create_dir_all(&path).map_err(|source| SaveError::Create { path, source })?;
    }
    Ok(())
}

/// Writes `project.json` at the current format version, creating the
/// layout if needed.
pub fn save(project: &Project, project_dir: &Path) -> Result<(), SaveError> {
    project.validate().map_err(SaveError::Invalid)?;
    create_layout(project_dir)?;
    let mut current = project.clone();
    current.version = FORMAT_VERSION;
    let text = serde_json::to_string_pretty(&current)?;
    let path = project_dir.join(PROJECT_FILE);
    let temp = project_dir.join(format!("{PROJECT_FILE}.tmp"));
    fs::write(&temp, text).map_err(|source| SaveError::Write {
        path: temp.clone(),
        source,
    })?;
    fs::rename(&temp, &path).map_err(|source| SaveError::Write { path, source })
}

/// Reads and upgrades `project.json`. The returned project is at the
/// current version whatever the file held.
pub fn load(project_dir: &Path) -> Result<Project, LoadError> {
    let path = project_dir.join(PROJECT_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(LoadError::NotAProject {
                path: project_dir.to_path_buf(),
            })
        }
        Err(source) => return Err(LoadError::Io { path, source }),
    };
    load_str(&text, &path)
}

/// [`load`] on text already read; `path` is for error messages.
pub fn load_str(text: &str, path: &Path) -> Result<Project, LoadError> {
    let mut value: Value = serde_json::from_str(text).map_err(|source| LoadError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    let found = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| LoadError::NoVersion {
            path: path.to_path_buf(),
        })?;
    if found > u64::from(FORMAT_VERSION) {
        return Err(LoadError::TooNew {
            path: path.to_path_buf(),
            found,
            supported: FORMAT_VERSION,
        });
    }
    let mut version = found as u32;
    while version < FORMAT_VERSION {
        value = migrate(version, value);
        version += 1;
        value["version"] = Value::from(version);
    }
    let project: Project = serde_json::from_value(value).map_err(|source| LoadError::Shape {
        path: path.to_path_buf(),
        source,
    })?;
    project.validate().map_err(|why| LoadError::Invalid {
        path: path.to_path_buf(),
        why,
    })?;
    Ok(project)
}

/// Upgrades a `project.json` value from `from` to `from + 1`. Each step
/// is written when the format changes and never edited afterwards, so a
/// file from any earlier version walks the same path.
fn migrate(from: u32, mut value: Value) -> Value {
    match from {
        // Version 0 never existed; a file claiming it is read as version 1.
        0 => value,
        // 1 -> 2: the mixer. Every track gets a strip at unity, centred,
        // not muted or soloed, and the project a master strip the same.
        1 => {
            let unity = serde_json::json!({
                "gainDb": 0.0, "pan": 0.0, "mute": false, "solo": false
            });
            if let Some(tracks) = value.get_mut("tracks").and_then(Value::as_array_mut) {
                for track in tracks {
                    if let Some(t) = track.as_object_mut() {
                        t.entry("strip").or_insert_with(|| unity.clone());
                    }
                }
            }
            if let Some(p) = value.as_object_mut() {
                p.entry("master").or_insert(unity);
            }
            value
        }
        _ => value,
    }
}

/// Project directories under `parent`, by name, sorted.
pub fn list(parent: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(names),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
            continue;
        }
        if !path.join(PROJECT_FILE).is_file() {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(stem.to_string());
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, ClipId, Pedal, PedalId, Track, TrackId};

    fn project() -> Project {
        let mut p = Project::new("Song", 48_000.0);
        let mut t = Track::new(TrackId(1), "Guitar");
        t.armed = true;
        t.chain.push(Pedal {
            id: PedalId(2),
            spec: "overdrive".into(),
            values: vec![20.0, 3000.0, -6.0],
        });
        t.clips.push(Clip {
            id: ClipId(3),
            name: "take 1".into(),
            file: "take-1.wav".into(),
            start: 4800,
            offset: 100,
            length: 48_000,
        });
        p.tracks.push(t);
        p.tempo.bpm = 100.0;
        p
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir_for(dir.path(), "Song");
        let p = project();
        save(&p, &project_dir).unwrap();
        for sub in [AUDIO_DIR, PLUGINS_DIR, PEDALS_DIR] {
            assert!(project_dir.join(sub).is_dir(), "{sub} was not created");
        }
        assert!(!project_dir.join("project.json.tmp").exists());
        assert_eq!(load(&project_dir).unwrap(), p);
        assert_eq!(list(dir.path()).unwrap(), vec!["Song".to_string()]);
        assert_eq!(
            list(&dir.path().join("nowhere")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_missing_project_is_reported_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&dir.path().join("Nope.cantina")).unwrap_err();
        assert!(matches!(err, LoadError::NotAProject { .. }), "{err}");
        assert!(err.to_string().contains("not a project"));
    }

    #[test]
    fn malformed_files_give_clear_errors() {
        let path = Path::new("x/project.json");
        let json = load_str("{not json", path).unwrap_err();
        assert!(matches!(json, LoadError::Json { .. }), "{json}");
        assert!(json
            .to_string()
            .starts_with("x/project.json is not valid JSON"));

        let no_version = load_str(r#"{"name":"x"}"#, path).unwrap_err();
        assert!(
            matches!(no_version, LoadError::NoVersion { .. }),
            "{no_version}"
        );

        let shape = load_str(r#"{"version":2,"name":"x","sampleRate":"fast"}"#, path).unwrap_err();
        assert!(matches!(shape, LoadError::Shape { .. }), "{shape}");
        assert!(shape
            .to_string()
            .contains("does not match the project format"));

        let invalid = load_str(
            r#"{"version":2,"name":"x","sampleRate":48000,"tracks":[{"id":1,"name":"a"},{"id":1,"name":"b"}]}"#,
            path,
        )
        .unwrap_err();
        assert!(matches!(invalid, LoadError::Invalid { .. }), "{invalid}");
        assert!(invalid.to_string().contains("used more than once"));
    }

    #[test]
    fn a_newer_file_is_refused() {
        let err = load_str(
            &format!(
                r#"{{"version":{},"name":"x","sampleRate":48000}}"#,
                FORMAT_VERSION + 1
            ),
            Path::new("p/project.json"),
        )
        .unwrap_err();
        assert!(matches!(err, LoadError::TooNew { .. }), "{err}");
        assert!(err.to_string().contains("this Cantina reads up to version"));
    }

    #[test]
    fn save_refuses_an_unusable_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project();
        p.tracks[0].clips[0].length = 0;
        let err = save(&p, &dir_for(dir.path(), "Bad")).unwrap_err();
        assert!(matches!(err, SaveError::Invalid(_)), "{err}");
    }

    #[test]
    fn a_version_1_file_gains_strips() {
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1.json"),
        )
        .unwrap();
        let project = load_str(&text, Path::new("v1/project.json")).unwrap();
        assert_eq!(project.version, FORMAT_VERSION);
        assert_eq!(project.tracks[0].strip, crate::model::Strip::default());
        assert_eq!(project.master, crate::model::Strip::default());
        // Everything the old file held survives.
        assert_eq!(project.tracks[0].chain.len(), 2);
        assert_eq!(project.tracks[0].clips[1].offset, 4800);
        // Saving writes the new version.
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir_for(dir.path(), "Old");
        save(&project, &project_dir).unwrap();
        let text = fs::read_to_string(project_dir.join(PROJECT_FILE)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], FORMAT_VERSION);
        assert_eq!(value["master"]["gainDb"], 0.0);
    }

    #[test]
    fn every_fixture_loads_at_the_current_version() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut seen = 0;
        for entry in fs::read_dir(&fixtures).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            let project = load_str(&text, &path).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(project.version, FORMAT_VERSION, "{}", path.display());
            assert_eq!(project.name, "Fixture", "{}", path.display());
            assert_eq!(project.tracks.len(), 2, "{}", path.display());
            seen += 1;
        }
        assert!(seen >= 1, "no fixtures under {}", fixtures.display());
    }
}
