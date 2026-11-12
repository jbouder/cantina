//! Exercise files: the stock exercises in the bundle's `exercises/` and
//! the user's in `~/Music/Cantina/Exercises`, a library folder watched
//! like the others.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cantina_project::{exercise_from_json, Exercise, Instrument, Mode, EXERCISE_SUFFIX};
use serde::Serialize;

use crate::library::{BrokenFile, Kind};

/// An exercise the practice view offers. Mirrors `ExerciseInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseInfo {
    pub name: String,
    /// `guitar` or `bass`.
    pub instrument: &'static str,
    /// `chord`, `change`, `scale`, `technique`, `riff` or `rhythm`.
    pub teaches: &'static str,
    pub style: String,
    /// Ships with Cantina: read-only.
    pub stock: bool,
    /// The file, which names the exercise.
    pub path: String,
    /// `A minor`.
    pub key: String,
    /// The tempo ladder's start, goal and step, in beats per minute.
    pub start: f64,
    pub goal: f64,
    pub step: f64,
    /// The fastest a whole pass has been played, from the practice log.
    pub best: Option<f64>,
    /// The last day it was practiced, `YYYY-MM-DD`.
    pub last: Option<String>,
}

fn info(exercise: &Exercise, path: &Path, stock: bool) -> ExerciseInfo {
    ExerciseInfo {
        name: exercise.name.clone(),
        instrument: match exercise.instrument {
            Instrument::Guitar => "guitar",
            Instrument::Bass => "bass",
        },
        teaches: exercise.teaches.name(),
        style: exercise.style.clone(),
        stock,
        path: path.display().to_string(),
        key: format!(
            "{} {}",
            exercise.key.tonic,
            match exercise.key.mode {
                Mode::Major => "major",
                Mode::Minor => "minor",
            }
        ),
        start: exercise.tempo.start,
        goal: exercise.tempo.goal,
        step: exercise.tempo.step,
        best: None,
        last: None,
    }
}

/// Reads and checks an exercise file.
pub fn read(path: &Path) -> Result<Exercise> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    exercise_from_json(&text).map_err(|p| anyhow::anyhow!("{}", p.join("; ")))
}

fn exercise_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(EXERCISE_SUFFIX))
        })
        .collect();
    out.sort();
    out
}

/// By style, then what it teaches, then name.
fn sort(exercises: &mut [ExerciseInfo]) {
    exercises.sort_by(|a, b| {
        (a.style.to_lowercase(), a.teaches, a.name.to_lowercase()).cmp(&(
            b.style.to_lowercase(),
            b.teaches,
            b.name.to_lowercase(),
        ))
    });
}

/// The stock exercises under `roots`; the first root to have a file of a
/// name wins. Files that do not load are left out: they are checked in CI.
pub fn scan_stock(roots: &[PathBuf]) -> Vec<ExerciseInfo> {
    let mut found: Vec<ExerciseInfo> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for root in roots {
        for path in exercise_files(root) {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if file.as_ref().is_some_and(|f| names.contains(f)) {
                continue;
            }
            if let Ok(exercise) = read(&path) {
                found.push(info(&exercise, &path, true));
                names.extend(file);
            }
        }
    }
    sort(&mut found);
    found
}

/// The user's exercises in `dir`, with the files that do not load listed
/// in `broken` with the reason.
pub fn scan_user(dir: &Path, broken: &mut Vec<BrokenFile>) -> Vec<ExerciseInfo> {
    let mut found = Vec::new();
    for path in exercise_files(dir) {
        match read(&path) {
            Ok(exercise) => found.push(info(&exercise, &path, false)),
            Err(e) => broken.push(BrokenFile {
                kind: Kind::Exercise,
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../exercises")
    }

    #[test]
    fn the_stock_exercises_load_once_each_by_style() {
        let found = scan_stock(&[stock(), stock()]);
        assert!(found.len() >= 30, "{}", found.len());
        assert!(found.iter().all(|e| e.stock));
        let mut paths: Vec<&str> = found.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), found.len());
        assert!(found
            .windows(2)
            .all(|w| w[0].style.to_lowercase() <= w[1].style.to_lowercase()));
        let e = found.iter().find(|e| e.name == "Open E").unwrap();
        assert_eq!(
            (e.instrument, e.teaches, e.key.as_str()),
            ("guitar", "chord", "E major")
        );
        assert!(found.iter().any(|e| e.instrument == "bass"));
    }

    #[test]
    fn user_exercises_are_read_and_broken_ones_say_why() {
        let dir = std::env::temp_dir().join(format!(
            "cantina-exercises-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            stock().join("open-e.exercise.json"),
            dir.join("mine.exercise.json"),
        )
        .unwrap();
        let text = std::fs::read_to_string(stock().join("open-e.exercise.json")).unwrap();
        std::fs::write(
            dir.join("bad.exercise.json"),
            text.replace("\"goal\": 100", "\"goal\": 40"),
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not an exercise").unwrap();
        let mut broken = Vec::new();
        let found = scan_user(&dir, &mut broken);
        assert_eq!(found.len(), 1);
        assert!(!found[0].stock);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].kind, Kind::Exercise);
        assert_eq!(broken[0].name, "bad.exercise.json");
        assert!(
            broken[0]
                .reason
                .contains("the goal tempo 40 is below the start tempo 60"),
            "{}",
            broken[0].reason
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
