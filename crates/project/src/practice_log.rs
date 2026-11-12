//! The practice log: what was practiced, on which day, how fast and for
//! how long, as `~/Music/Cantina/Practice/log.json`. It is never part of a
//! project. One entry per exercise per day: practicing an exercise again
//! the same day adds to its minutes and keeps the faster tempo.
//! `practice-log.schema.json` describes the file.
//!
//! ```json
//! {
//!   "version": 1,
//!   "days": [
//!     { "exercise": "pentatonic-box-1", "date": "2026-11-12", "tempo": 85, "minutes": 12.5 }
//!   ]
//! }
//! ```
//!
//! An exercise is named by its file's name less `.exercise.json`. The
//! tempo is the fastest a whole pass of the loop was played at that day;
//! 0 when no pass was finished.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::jam::JAM_TEMPO;

/// The version a practice log is written at.
pub const PRACTICE_LOG_VERSION: u32 = 1;

/// The log's file name, in the Practice folder.
pub const PRACTICE_LOG_FILE: &str = "log.json";

/// What was practiced, day by day.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeLog {
    /// The format version, 1.
    pub version: u32,
    /// One entry per exercise per day, oldest first.
    #[serde(default)]
    pub days: Vec<PracticeDay>,
}

/// An exercise practiced on a day.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeDay {
    /// The exercise, by its file's name less `.exercise.json`.
    pub exercise: String,
    /// The day, `YYYY-MM-DD`, in local time.
    pub date: String,
    /// The fastest a whole pass was played at, in beats per minute; 0
    /// when no pass was finished.
    pub tempo: f64,
    /// Minutes played.
    pub minutes: f64,
}

impl Default for PracticeLog {
    fn default() -> Self {
        Self {
            version: PRACTICE_LOG_VERSION,
            days: Vec::new(),
        }
    }
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` date, or `None` when it is
/// not one.
pub fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[(m - 1) as usize] {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

impl PracticeLog {
    /// Adds a session: to the exercise's entry for that day, its minutes
    /// added and the faster tempo kept, or as a new entry.
    pub fn record(&mut self, exercise: &str, date: &str, tempo: f64, minutes: f64) {
        let tempo = if tempo.is_finite() {
            tempo.max(0.0)
        } else {
            0.0
        };
        let minutes = if minutes.is_finite() {
            minutes.max(0.0)
        } else {
            0.0
        };
        if let Some(day) = self
            .days
            .iter_mut()
            .find(|d| d.exercise == exercise && d.date == date)
        {
            day.tempo = day.tempo.max(tempo);
            day.minutes += minutes;
        } else {
            self.days.push(PracticeDay {
                exercise: exercise.to_string(),
                date: date.to_string(),
                tempo,
                minutes,
            });
            self.days.sort_by(|a, b| a.date.cmp(&b.date));
        }
    }

    /// The fastest an exercise has been played, ever.
    pub fn best(&self, exercise: &str) -> Option<f64> {
        self.days
            .iter()
            .filter(|d| d.exercise == exercise && d.tempo > 0.0)
            .map(|d| d.tempo)
            .reduce(f64::max)
    }

    /// The last day an exercise was practiced.
    pub fn last(&self, exercise: &str) -> Option<&PracticeDay> {
        self.days.iter().rev().find(|d| d.exercise == exercise)
    }

    /// Days in a row with any practice, up to `today`, or up to
    /// yesterday when today has none yet.
    pub fn streak(&self, today: &str) -> u32 {
        let Some(today) = day_number(today) else {
            return 0;
        };
        let mut days: Vec<i64> = self
            .days
            .iter()
            .filter_map(|d| day_number(&d.date))
            .collect();
        days.sort_unstable();
        days.dedup();
        let mut want = if days.last() == Some(&today) {
            today
        } else {
            today - 1
        };
        let mut count = 0;
        for day in days.iter().rev() {
            if *day == want {
                count += 1;
                want -= 1;
            } else if *day < want {
                break;
            }
        }
        count
    }

    /// Minutes practiced on a day, every exercise together.
    pub fn minutes_on(&self, date: &str) -> f64 {
        self.days
            .iter()
            .filter(|d| d.date == date)
            .map(|d| d.minutes)
            .sum()
    }

    /// The rules beyond the shape: a version this build reads, dates that
    /// are dates, a tempo that is 0 or one Cantina plays, and minutes not
    /// below 0.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version == 0 || self.version > PRACTICE_LOG_VERSION {
            problems.push(format!(
                "version {} is not one this Cantina reads (1 to {PRACTICE_LOG_VERSION})",
                self.version
            ));
        }
        let (slow, fast) = JAM_TEMPO;
        for (i, d) in self.days.iter().enumerate() {
            let at = format!("day {} ({} on {})", i + 1, d.exercise, d.date);
            if d.exercise.trim().is_empty() {
                problems.push(format!("day {} names no exercise", i + 1));
            }
            if day_number(&d.date).is_none() {
                problems.push(format!("{at}: {:?} is not a YYYY-MM-DD date", d.date));
            }
            if !(d.tempo == 0.0 || (slow..=fast).contains(&d.tempo)) {
                problems.push(format!(
                    "{at}: the tempo {} is neither 0 nor {slow} to {fast}",
                    d.tempo
                ));
            }
            if !(d.minutes.is_finite() && d.minutes >= 0.0) {
                problems.push(format!("{at}: {} minutes is below 0", d.minutes));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// Reads and checks a practice log's text. A `"$schema"` field is ignored.
pub fn practice_log_from_json(text: &str) -> Result<PracticeLog, Vec<String>> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| vec![format!("not a practice log: {e}")])?;
    if let Some(o) = value.as_object_mut() {
        o.remove("$schema");
    }
    let log: PracticeLog =
        serde_json::from_value(value).map_err(|e| vec![format!("not a practice log: {e}")])?;
    log.validate()?;
    Ok(log)
}

/// A practice log as the text of its file, pointing at its schema.
pub fn practice_log_to_json(log: &PracticeLog) -> String {
    let mut value = serde_json::to_value(log).expect("a practice log serializes");
    if let Some(o) = value.as_object_mut() {
        let mut with = serde_json::Map::new();
        with.insert(
            "$schema".into(),
            Value::from(
                "https://cantina-docs.vibeproject.workers.dev/schemas/practice-log.schema.json",
            ),
        );
        with.extend(std::mem::take(o));
        *o = with;
    }
    let mut text = serde_json::to_string_pretty(&value).expect("a practice log serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_count_days_from_1970() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2000-03-01"), Some(11_017));
        assert_eq!(day_number("2026-11-12"), Some(20_769));
        assert_eq!(
            day_number("2024-03-01").unwrap() - day_number("2024-02-28").unwrap(),
            2
        );
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "26-11-12",
            "2026-1-12",
            "today",
            "",
        ] {
            assert_eq!(day_number(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_day_adds_up_and_keeps_the_faster_tempo() {
        let mut log = PracticeLog::default();
        log.record("box-1", "2026-11-12", 80.0, 5.0);
        log.record("box-1", "2026-11-12", 75.0, 2.5);
        log.record("box-1", "2026-11-10", 70.0, 10.0);
        log.record("open-e", "2026-11-12", 0.0, 0.5);
        assert_eq!(log.days.len(), 3);
        assert_eq!(log.days[0].date, "2026-11-10", "oldest first");
        assert_eq!(log.best("box-1"), Some(80.0));
        assert_eq!(log.best("open-e"), None, "no pass finished");
        assert_eq!(log.last("box-1").unwrap().minutes, 7.5);
        assert_eq!(log.minutes_on("2026-11-12"), 8.0);
        let text = practice_log_to_json(&log);
        assert!(text.starts_with("{\n  \"$schema\""), "{text}");
        assert_eq!(practice_log_from_json(&text).unwrap(), log);
    }

    #[test]
    fn a_streak_counts_days_in_a_row_to_today_or_yesterday() {
        let mut log = PracticeLog::default();
        for date in ["2026-11-08", "2026-11-10", "2026-11-11"] {
            log.record("a", date, 60.0, 1.0);
        }
        assert_eq!(log.streak("2026-11-11"), 2);
        assert_eq!(log.streak("2026-11-12"), 2, "today not yet practiced");
        assert_eq!(log.streak("2026-11-13"), 0, "a day missed");
        log.record("b", "2026-11-12", 60.0, 1.0);
        assert_eq!(log.streak("2026-11-12"), 3);
        assert_eq!(PracticeLog::default().streak("2026-11-12"), 0);
    }

    #[test]
    fn rejects_what_is_not_a_log() {
        let bad =
            r#"{"version":2,"days":[{"exercise":"","date":"2026-02-30","tempo":12,"minutes":-1}]}"#;
        let p = practice_log_from_json(bad).unwrap_err();
        assert_eq!(p.len(), 5, "{p:#?}");
        assert!(practice_log_from_json("[]").unwrap_err()[0].starts_with("not a practice log"));
    }
}
