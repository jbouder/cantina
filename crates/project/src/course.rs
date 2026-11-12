//! Courses: exercises in an order, each with the tempo to reach before
//! the next, as a file of its own, `<id>.course.json`.
//! `course.schema.json` describes the file.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Metal rhythm",
//!   "description": "From the first downstroke to a breakdown.",
//!   "instrument": "guitar",
//!   "style": "Metal",
//!   "steps": [
//!     { "exercise": "downpicking-endurance", "goal": 160 },
//!     { "exercise": "gallops", "goal": 150, "note": "Gallops are downpicking with a turn." }
//!   ]
//! }
//! ```
//!
//! An exercise is named by its file's name less `.exercise.json`, the
//! user's in `~/Music/Cantina/Exercises` first, then the stock ones. A
//! step is done when the practice log's best for its exercise reaches the
//! step's goal.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::jam::JAM_TEMPO;
use crate::tab::Instrument;

/// The version a course is written at.
pub const COURSE_VERSION: u32 = 1;

/// What a course file's name ends with.
pub const COURSE_SUFFIX: &str = ".course.json";

/// Exercises in an order, each to a tempo.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    /// The format version, 1.
    pub version: u32,
    /// Shown in the course list.
    pub name: String,
    /// One line on what it takes you through.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Guitar or bass.
    pub instrument: Instrument,
    /// The style it is for: `Metal`, `Beginner`…
    pub style: String,
    /// The exercises, in the order to practice them.
    pub steps: Vec<CourseStep>,
}

/// One exercise of a course, and the tempo to reach on it.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseStep {
    /// The exercise, by its file's name less `.exercise.json`.
    pub exercise: String,
    /// The tempo a whole pass must be played at before the next step,
    /// in beats per minute, 30 to 300.
    pub goal: f64,
    /// A line on why it comes here, shown with the step.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Course {
    /// The first step whose goal `best` has not reached, given each
    /// exercise's best tempo; none when every step is done.
    pub fn next(&self, best: impl Fn(&str) -> Option<f64>) -> Option<usize> {
        self.steps
            .iter()
            .position(|s| best(&s.exercise).is_none_or(|b| b < s.goal))
    }

    /// The rules beyond the shape: a version this build reads, a name and
    /// a style, steps, each naming an exercise once, with a goal Cantina
    /// plays.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version == 0 || self.version > COURSE_VERSION {
            problems.push(format!(
                "version {} is not one this Cantina reads (1 to {COURSE_VERSION})",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            problems.push("the name is empty".into());
        }
        if self.style.trim().is_empty() {
            problems.push("the style is empty".into());
        }
        if self.steps.is_empty() {
            problems.push("the course has no steps".into());
        }
        let (slow, fast) = JAM_TEMPO;
        for (i, step) in self.steps.iter().enumerate() {
            let at = format!("step {} ({})", i + 1, step.exercise);
            if step.exercise.trim().is_empty() {
                problems.push(format!("step {} names no exercise", i + 1));
            } else if self.steps[..i].iter().any(|s| s.exercise == step.exercise) {
                problems.push(format!("{at} is in the course twice"));
            }
            if !(step.goal.is_finite() && (slow..=fast).contains(&step.goal)) {
                problems.push(format!(
                    "{at}: the goal {} is outside {slow} to {fast} bpm",
                    step.goal
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// Reads and checks a course file's text. A `"$schema"` field is ignored.
pub fn course_from_json(text: &str) -> Result<Course, Vec<String>> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| vec![format!("not a course: {e}")])?;
    if let Some(o) = value.as_object_mut() {
        o.remove("$schema");
    }
    let course: Course =
        serde_json::from_value(value).map_err(|e| vec![format!("not a course: {e}")])?;
    course.validate()?;
    Ok(course)
}

/// The file name for a course id.
pub fn course_file_name(id: &str) -> String {
    format!("{id}{COURSE_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course() -> Course {
        course_from_json(
            r#"{ "$schema": "x", "version": 1, "name": "Metal rhythm", "instrument": "guitar",
                 "style": "Metal", "steps": [
                   { "exercise": "downpicking-endurance", "goal": 160 },
                   { "exercise": "gallops", "goal": 150, "note": "Downpicking with a turn." } ] }"#,
        )
        .unwrap()
    }

    #[test]
    fn the_next_step_is_the_first_not_reached() {
        let c = course();
        assert_eq!(c.next(|_| None), Some(0));
        assert_eq!(
            c.next(|e| (e == "downpicking-endurance").then_some(160.0)),
            Some(1)
        );
        assert_eq!(
            c.next(|e| (e == "downpicking-endurance").then_some(155.0)),
            Some(0)
        );
        assert_eq!(c.next(|_| Some(200.0)), None, "every step done");
        assert_eq!(course_file_name("metal-rhythm"), "metal-rhythm.course.json");
    }

    #[test]
    fn reports_every_problem() {
        let mut c = course();
        c.version = 3;
        c.name = String::new();
        c.steps.push(CourseStep {
            exercise: "gallops".into(),
            goal: 500.0,
            note: String::new(),
        });
        let p = c.validate().unwrap_err();
        assert_eq!(p.len(), 4, "{p:#?}");
        assert!(p[2].contains("twice"), "{p:#?}");
        c.steps.clear();
        assert!(c
            .validate()
            .unwrap_err()
            .iter()
            .any(|p| p.contains("no steps")));
        assert!(course_from_json("{}").unwrap_err()[0].starts_with("not a course"));
    }
}
