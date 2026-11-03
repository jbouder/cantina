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

use serde_json::Value;

use crate::model::{Pattern, PatternId};

/// The suffix of a pattern file.
pub const PATTERN_SUFFIX: &str = ".pattern.json";

/// Reads and validates a pattern file. The pattern comes back with id 0,
/// for the project to replace with a fresh one.
pub fn pattern_from_json(text: &str) -> Result<Pattern, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("a pattern file holds one JSON object".into());
    };
    object.remove("$schema");
    object.insert("id".into(), Value::from(0));
    let pattern: Pattern = serde_json::from_value(value)
        .map_err(|e| format!("does not match the pattern format: {e}"))?;
    pattern.validate()?;
    Ok(pattern)
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
