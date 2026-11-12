//! Course files: the stock courses in the bundle's `courses/` and the
//! user's in `~/Music/Cantina/Courses`, a library folder watched like the
//! others. A course's progress comes from the practice log.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cantina_project::{course_from_json, Course, PracticeLog, COURSE_SUFFIX};
use serde::Serialize;

use crate::exercises::ExerciseInfo;
use crate::library::{BrokenFile, Kind};

/// A course as read, with where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseFile {
    pub course: Course,
    pub path: PathBuf,
    pub stock: bool,
}

/// A course and how far through it you are. Mirrors `CourseInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseInfo {
    pub name: String,
    pub description: String,
    /// `guitar` or `bass`.
    pub instrument: &'static str,
    pub style: String,
    pub stock: bool,
    pub path: String,
    pub steps: Vec<StepInfo>,
    /// The first step not done, by index; none when the course is done.
    pub next: Option<usize>,
}

/// One step of a course. Mirrors `CourseStepInfo` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    /// The exercise's id, its file less `.exercise.json`.
    pub exercise: String,
    /// The exercise's name, or the id when it is not installed.
    pub name: String,
    /// Its file, when it is installed.
    pub path: Option<String>,
    pub goal: f64,
    pub note: String,
    /// The fastest a whole pass has been played, from the log.
    pub best: Option<f64>,
    /// The best has reached the goal.
    pub done: bool,
}

/// Reads and checks a course file.
pub fn read(path: &Path) -> Result<Course> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    course_from_json(&text).map_err(|p| anyhow::anyhow!("{}", p.join("; ")))
}

fn course_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(COURSE_SUFFIX))
        })
        .collect();
    out.sort();
    out
}

/// The stock courses under `roots`; the first root to have a file of a
/// name wins. Files that do not load are left out: they are checked in CI.
pub fn scan_stock(roots: &[PathBuf]) -> Vec<CourseFile> {
    let mut found: Vec<CourseFile> = Vec::new();
    for root in roots {
        for path in course_files(root) {
            if found.iter().any(|c| c.path.file_name() == path.file_name()) {
                continue;
            }
            if let Ok(course) = read(&path) {
                found.push(CourseFile {
                    course,
                    path,
                    stock: true,
                });
            }
        }
    }
    found
}

/// The user's courses in `dir`, with the files that do not load listed
/// in `broken` with the reason.
pub fn scan_user(dir: &Path, broken: &mut Vec<BrokenFile>) -> Vec<CourseFile> {
    let mut found = Vec::new();
    for path in course_files(dir) {
        match read(&path) {
            Ok(course) => found.push(CourseFile {
                course,
                path,
                stock: false,
            }),
            Err(e) => broken.push(BrokenFile {
                kind: Kind::Course,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.display().to_string(),
                reason: format!("{e:#}"),
            }),
        }
    }
    found
}

/// An exercise's id: its file's name less `.exercise.json`.
pub fn exercise_id(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(cantina_project::EXERCISE_SUFFIX)
        .unwrap_or(&name)
        .to_string()
}

/// A course with its steps named from the exercises installed (yours
/// before the stock ones) and its progress from the log.
pub fn info(file: &CourseFile, exercises: &[ExerciseInfo], log: &PracticeLog) -> CourseInfo {
    let find = |id: &str| {
        exercises
            .iter()
            .filter(|e| exercise_id(&e.path) == id)
            .min_by_key(|e| e.stock)
    };
    let steps: Vec<StepInfo> = file
        .course
        .steps
        .iter()
        .map(|s| {
            let best = log.best(&s.exercise);
            let installed = find(&s.exercise);
            StepInfo {
                exercise: s.exercise.clone(),
                name: installed.map_or_else(|| s.exercise.clone(), |e| e.name.clone()),
                path: installed.map(|e| e.path.clone()),
                goal: s.goal,
                note: s.note.clone(),
                best,
                done: best.is_some_and(|b| b >= s.goal),
            }
        })
        .collect();
    CourseInfo {
        name: file.course.name.clone(),
        description: file.course.description.clone(),
        instrument: match file.course.instrument {
            cantina_project::Instrument::Guitar => "guitar",
            cantina_project::Instrument::Bass => "bass",
        },
        style: file.course.style.clone(),
        stock: file.stock,
        path: file.path.display().to_string(),
        next: file.course.next(|id| log.best(id)),
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(dir: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(dir)
    }

    #[test]
    fn the_stock_courses_load_and_follow_the_log() {
        let courses = scan_stock(&[root("courses"), root("courses")]);
        assert_eq!(courses.len(), 5);
        let exercises = crate::exercises::scan_stock(&[root("exercises")]);
        let metal = courses
            .iter()
            .find(|c| c.course.name == "Metal rhythm")
            .unwrap();
        let mut log = PracticeLog::default();
        let first = info(metal, &exercises, &log);
        assert_eq!(first.next, Some(0));
        assert!(
            first.steps.iter().all(|s| s.path.is_some()),
            "every step installed"
        );
        assert_eq!(first.steps[0].name, "Downpicking endurance");
        let goal = first.steps[0].goal;
        log.record("downpicking-endurance", "2026-11-12", goal, 5.0);
        let after = info(metal, &exercises, &log);
        assert!(after.steps[0].done);
        assert_eq!(after.next, Some(1));
    }

    #[test]
    fn a_broken_course_says_why() {
        let dir = std::env::temp_dir().join(format!(
            "cantina-courses-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("bad.course.json"),
            r#"{"version":1,"name":"Bad","instrument":"guitar","style":"Metal","steps":[]}"#,
        )
        .unwrap();
        let mut broken = Vec::new();
        assert!(scan_user(&dir, &mut broken).is_empty());
        assert_eq!(broken[0].kind, Kind::Course);
        assert!(
            broken[0].reason.contains("no steps"),
            "{}",
            broken[0].reason
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
