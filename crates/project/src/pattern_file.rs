//! Patterns as files of their own: `<name>.pattern.json`, the object a
//! project holds in `patterns` without its `id`. The project assigns an
//! id when a file is imported, so the same file can be imported twice.
//!
//! ```json
//! {
//!   "name": "Half-time",
//!   "stepsPerBeat": 4,
//!   "length": 16,
//!   "swing": 0,
//!   "rows": [
//!     { "pad": "kick", "steps": [1,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0] },
//!     { "pad": "snare", "steps": [0,0,0,0, 0,0,0,0, 1,0,0,0, 0,0,0,0] }
//!   ]
//! }
//! ```
//!
//! `pattern.schema.json` describes it. A `"$schema"` field is ignored, and
//! so is an `"id"`, so a pattern copied out of a project file reads too.
//!
//! A stock groove also carries a [`Groove`] in its file: its genre, the
//! section of a song it is for, its time signature, the tempos it suits
//! and its feel. That is for finding the groove; a project keeps the
//! pattern alone once the groove is added to it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Pattern, PatternId, PPQ};

/// The part of a song a groove is for.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    /// Before the song gets going.
    Intro,
    /// The main groove.
    Verse,
    /// Bigger: the crash or ride opens up.
    Chorus,
    /// Somewhere else for a while.
    Bridge,
    /// A bar that leads into the next section.
    Fill,
    /// The last hit.
    Ending,
}

/// How a groove's beat feels.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feel {
    /// Even subdivisions.
    Straight,
    /// Long-short eighths: blues, swing rock.
    Shuffle,
    /// Swung, lighter than a shuffle: jazz.
    Swing,
    /// The backbeat on 3 rather than 2 and 4: twice as heavy.
    HalfTime,
    /// The snare on every upbeat: twice as urgent.
    DoubleTime,
}

/// A time signature, as a groove states it.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Meter {
    /// Beats in a bar.
    pub numerator: u8,
    /// The note value of a beat: 4 for quarter notes, 8 for eighths.
    pub denominator: u8,
}

/// The tempos a groove suits, in beats per minute.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoRange {
    /// Slowest.
    pub min: f64,
    /// Fastest.
    pub max: f64,
}

/// What a stock groove says about itself.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Groove {
    /// The genre it belongs to, as the browser groups it: `Rock`,
    /// `Heavy metal`, `Blues shuffle`…
    pub genre: String,
    /// The part of a song it is for.
    pub section: Section,
    /// Its time signature; the pattern is a whole number of bars of it.
    pub time_signature: Meter,
    /// The tempos it suits.
    pub tempo: TempoRange,
    /// How it feels.
    pub feel: Feel,
}

impl Groove {
    /// Checks the groove against itself and its pattern: a genre, a time
    /// signature that exists, a sensible tempo range, and a pattern that
    /// is whole bars long.
    pub fn validate(&self, pattern: &Pattern) -> Result<(), String> {
        if self.genre.trim().is_empty() {
            return Err("the groove has no genre".into());
        }
        let Meter {
            numerator,
            denominator,
        } = self.time_signature;
        if !(1..=16).contains(&numerator) || !matches!(denominator, 2 | 4 | 8 | 16) {
            return Err(format!(
                "the groove's time signature {numerator}/{denominator} is not one Cantina plays"
            ));
        }
        let TempoRange { min, max } = self.tempo;
        if !(min.is_finite() && max.is_finite() && 20.0 <= min && min <= max && max <= 400.0) {
            return Err(format!(
                "the groove's tempo range {min} to {max} bpm is not 20 to 400 with the slower first"
            ));
        }
        let bar = u64::from(PPQ) * 4 / u64::from(denominator) * u64::from(numerator);
        if pattern.ticks() % bar != 0 {
            return Err(format!(
                "the pattern is not a whole number of {numerator}/{denominator} bars"
            ));
        }
        Ok(())
    }
}

/// The suffix of a pattern file.
pub const PATTERN_SUFFIX: &str = ".pattern.json";

/// Reads and validates a pattern file. The pattern comes back with id 0,
/// for the project to replace with a fresh one; a groove's metadata is
/// checked and left behind.
pub fn pattern_from_json(text: &str) -> Result<Pattern, String> {
    groove_from_json(text).map(|(pattern, _)| pattern)
}

/// Reads and validates a pattern file and the [`Groove`] it carries, if
/// it is a groove.
pub fn groove_from_json(text: &str) -> Result<(Pattern, Option<Groove>), String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("a pattern file holds one JSON object".into());
    };
    object.remove("$schema");
    object.insert("id".into(), Value::from(0));
    let groove = match object.remove("groove") {
        Some(g) => Some(
            serde_json::from_value::<Groove>(g)
                .map_err(|e| format!("the groove does not match the pattern format: {e}"))?,
        ),
        None => None,
    };
    let pattern: Pattern = serde_json::from_value(value)
        .map_err(|e| format!("does not match the pattern format: {e}"))?;
    pattern.validate()?;
    if let Some(groove) = &groove {
        groove.validate(&pattern)?;
    }
    Ok((pattern, groove))
}

/// A pattern as a file: pretty JSON without its id.
pub fn pattern_to_json(pattern: &Pattern) -> String {
    let mut value = serde_json::to_value(pattern).expect("a pattern serializes");
    if let Some(object) = value.as_object_mut() {
        object.remove("id");
    }
    let mut text = serde_json::to_string_pretty(&value).expect("a pattern serializes");
    text.push('\n');
    text
}

/// A file name for a pattern: its name with anything that is not a
/// letter, digit, space, `-` or `_` replaced, plus [`PATTERN_SUFFIX`].
pub fn pattern_file_name(pattern: &Pattern) -> String {
    let stem: String = pattern
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim();
    format!(
        "{}{PATTERN_SUFFIX}",
        if stem.is_empty() { "Pattern" } else { stem }
    )
}

/// The pattern with `id`, renamed if `taken` already has its name: `Beat`
/// becomes `Beat 2`, then `Beat 3`.
pub fn imported(mut pattern: Pattern, id: PatternId, taken: &[&str]) -> Pattern {
    pattern.id = id;
    if taken.contains(&pattern.name.as_str()) {
        let base = pattern.name.clone();
        let mut n = 2;
        while taken.contains(&format!("{base} {n}").as_str()) {
            n += 1;
        }
        pattern.name = format!("{base} {n}");
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PatternRow;

    fn beat() -> Pattern {
        let mut p = Pattern::new(PatternId(7), "Beat");
        p.length = 4;
        p.rows.push(PatternRow {
            pad: "kick".into(),
            steps: vec![1.0, 0.0, 0.5, 0.0],
        });
        p
    }

    #[test]
    fn round_trips_without_the_id() {
        let text = pattern_to_json(&beat());
        assert!(!text.contains("\"id\""), "{text}");
        let back = pattern_from_json(&text).unwrap();
        assert_eq!(back.id, PatternId(0));
        assert_eq!(
            Pattern {
                id: PatternId(7),
                ..back
            },
            beat()
        );
    }

    #[test]
    fn ignores_schema_and_id_and_reports_problems() {
        let with = r#"{"$schema":"x","id":3,"name":"B","stepsPerBeat":4,"length":2,"swing":0,
            "rows":[{"pad":"kick","steps":[1,0]}]}"#;
        assert_eq!(pattern_from_json(with).unwrap().id, PatternId(0));
        let short = r#"{"name":"B","stepsPerBeat":4,"length":3,"swing":0,
            "rows":[{"pad":"kick","steps":[1,0]}]}"#;
        let err = pattern_from_json(short).unwrap_err();
        assert!(
            err.starts_with("the pattern row \"kick\" has 2 steps"),
            "{err}"
        );
        assert!(pattern_from_json("[]")
            .unwrap_err()
            .contains("one JSON object"));
        assert!(pattern_from_json(r#"{"name":"B"}"#)
            .unwrap_err()
            .contains("missing field"));
    }

    #[test]
    fn a_groove_carries_its_metadata_and_the_project_keeps_the_pattern() {
        let text = r#"{"name":"Rock verse","stepsPerBeat":4,"length":16,"swing":0,
            "rows":[{"pad":"kick","steps":[1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0]}],
            "groove":{"genre":"Rock","section":"verse","timeSignature":{"numerator":4,"denominator":4},
              "tempo":{"min":90,"max":140},"feel":"straight"}}"#;
        let (pattern, groove) = groove_from_json(text).unwrap();
        let groove = groove.unwrap();
        assert_eq!(
            (groove.genre.as_str(), groove.section, groove.feel),
            ("Rock", Section::Verse, Feel::Straight)
        );
        assert_eq!(pattern.name, "Rock verse");
        assert_eq!(pattern_from_json(text).unwrap(), pattern);
        assert!(!pattern_to_json(&pattern).contains("groove"));
        // A plain pattern has none.
        assert!(groove_from_json(&pattern_to_json(&beat()))
            .unwrap()
            .1
            .is_none());
        // Bad metadata is refused with the reason.
        let bars = text.replace(r#""numerator":4"#, r#""numerator":3"#);
        assert!(groove_from_json(&bars)
            .unwrap_err()
            .contains("whole number of 3/4 bars"));
        let slow = text.replace(r#""min":90"#, r#""min":190"#);
        assert!(groove_from_json(&slow)
            .unwrap_err()
            .contains("slower first"));
        let genre = text.replace(r#""genre":"Rock""#, r#""genre":" ""#);
        assert!(groove_from_json(&genre).unwrap_err().contains("no genre"));
    }

    #[test]
    fn imports_rename_on_a_clash() {
        let p = imported(beat(), PatternId(9), &["Beat", "Beat 2", "Other"]);
        assert_eq!((p.id, p.name.as_str()), (PatternId(9), "Beat 3"));
        let p = imported(beat(), PatternId(9), &["Other"]);
        assert_eq!(p.name, "Beat");
    }

    #[test]
    fn file_names_are_safe() {
        let mut p = beat();
        p.name = "Half/time: 140".into();
        assert_eq!(pattern_file_name(&p), "Half-time- 140.pattern.json");
        p.name = "  ".into();
        assert_eq!(pattern_file_name(&p), "Pattern.pattern.json");
    }
}
