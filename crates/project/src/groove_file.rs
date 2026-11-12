//! Grooves as files: `<name>.groove.json`, a whole drum part as the drum
//! designer saves it: its sections with their patterns, fills and
//! variations, the order they play in, the kit, and the tempo and time
//! signature it was made at.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Late train",
//!   "genre": "Rock",
//!   "tempo": 118,
//!   "timeSignature": { "numerator": 4, "denominator": 4 },
//!   "kit": "acoustic",
//!   "sections": [
//!     { "name": "Verse", "kind": "verse",
//!       "pattern": { "name": "Verse", "stepsPerBeat": 4, "length": 16, "swing": 0,
//!                    "rows": [ { "pad": "kick", "steps": [1,0,0,0, 0,0,0,0, 1,0,0,0, 0,0,0,0] } ] } }
//!   ],
//!   "order": [ { "section": "Verse", "times": 8 } ]
//! }
//! ```
//!
//! Patterns are written as pattern files are, without an id. The stock
//! grooves stay what they were, one `*.pattern.json` per section, and are
//! read as they are; this is the format for a part of your own.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drum_part::DrumPart;
use crate::pattern_file::{Meter, Section};

/// The suffix of a groove file.
pub const GROOVE_SUFFIX: &str = ".groove.json";

/// The groove format's version. Bump it, with a migration in
/// [`groove_file_from_json`], when the format changes.
pub const GROOVE_VERSION: u32 = 1;

/// A drum part as a file.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrooveFile {
    /// The format's version, 1.
    pub version: u32,
    /// Shown in the palette and the jam's groove picker.
    pub name: String,
    /// The genre it belongs to, if any: `Rock`, `Funk`…
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub genre: String,
    /// A line on it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The tempo it was made at, in beats per minute, 20 to 400.
    pub tempo: f64,
    /// The time signature it was made in.
    pub time_signature: Meter,
    /// The kit, sections and order.
    #[serde(flatten)]
    pub part: DrumPart,
}

impl GrooveFile {
    /// Checks the file: a version this build reads, a name, a tempo and a
    /// time signature Cantina plays, and a part that adds up.
    pub fn validate(&self) -> Result<(), String> {
        if self.version == 0 || self.version > GROOVE_VERSION {
            return Err(format!(
                "the groove is version {}; this Cantina reads 1 to {GROOVE_VERSION}",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            return Err("the groove has no name".into());
        }
        if !(self.tempo.is_finite() && (20.0..=400.0).contains(&self.tempo)) {
            return Err(format!(
                "the groove's tempo {} is not 20 to 400 bpm",
                self.tempo
            ));
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
        self.part.validate()
    }

    /// Ticks in a bar of its time signature.
    pub fn bar_ticks(&self) -> u64 {
        u64::from(crate::PPQ) * 4 / u64::from(self.time_signature.denominator.max(1))
            * u64::from(self.time_signature.numerator.max(1))
    }

    /// What a jam plays from it: for each kind of section, the first
    /// section of that kind, round a whole pass of its variations; a part
    /// with no verse gives its first section that loops (no intro, fill or
    /// ending) as the verse. Patterns that are not whole bars are left
    /// out, as a jam needs.
    pub fn jam_patterns(&self) -> Vec<(Section, crate::Pattern)> {
        let bar = self.bar_ticks();
        let mut out: Vec<(Section, crate::Pattern)> = Vec::new();
        for s in &self.part.sections {
            let Some(kind) = s.kind else { continue };
            if out.iter().any(|(k, _)| *k == kind) {
                continue;
            }
            let played = s.played();
            if played.ticks() % bar == 0 {
                out.push((kind, played));
            }
        }
        if !out.iter().any(|(k, _)| *k == Section::Verse) {
            let loops = |k: Option<Section>| {
                !matches!(k, Some(Section::Intro | Section::Fill | Section::Ending))
            };
            if let Some(s) = self
                .part
                .sections
                .iter()
                .filter(|s| loops(s.kind))
                .find(|s| s.played().ticks() % bar == 0)
            {
                out.push((Section::Verse, s.played()));
            }
        }
        out
    }
}

/// Every pattern in a groove file's JSON, so an id can go in on the way
/// in and come out on the way out.
fn patterns_mut(value: &mut Value) -> Vec<&mut serde_json::Map<String, Value>> {
    let mut out = Vec::new();
    if let Some(sections) = value.get_mut("sections").and_then(Value::as_array_mut) {
        for s in sections {
            let Some(s) = s.as_object_mut() else { continue };
            for (key, v) in s.iter_mut() {
                match key.as_str() {
                    "pattern" => {
                        if let Some(p) = v.as_object_mut() {
                            out.push(p);
                        }
                    }
                    "variations" => {
                        if let Some(list) = v.as_array_mut() {
                            out.extend(list.iter_mut().filter_map(Value::as_object_mut));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Reads and validates a groove file. Its patterns come back with id 0.
pub fn groove_file_from_json(text: &str) -> Result<GrooveFile, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("a groove file holds one JSON object".into());
    };
    object.remove("$schema");
    for p in patterns_mut(&mut value) {
        p.insert("id".into(), Value::from(0));
    }
    let groove: GrooveFile = serde_json::from_value(value)
        .map_err(|e| format!("does not match the groove format: {e}"))?;
    groove.validate()?;
    Ok(groove)
}

/// A groove as a file: pretty JSON, its patterns without ids.
pub fn groove_file_to_json(groove: &GrooveFile) -> String {
    let mut value = serde_json::to_value(groove).expect("a groove serializes");
    for p in patterns_mut(&mut value) {
        p.remove("id");
    }
    let mut text = serde_json::to_string_pretty(&value).expect("a groove serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drum_part::{OrderStep, PartSection, SectionFill, Vary};
    use crate::model::{Pattern, PatternId, PatternRow};

    fn pattern(name: &str, length: u16) -> Pattern {
        let mut p = Pattern::new(PatternId(0), name);
        p.length = length;
        let mut steps = vec![0.0; length as usize];
        steps[0] = 1.0;
        p.rows.push(PatternRow::new("kick", steps));
        p
    }

    fn section(name: &str, kind: Option<Section>, length: u16) -> PartSection {
        PartSection {
            name: name.into(),
            kind,
            pattern: pattern(name, length),
            fill: None,
            variations: Vec::new(),
            vary: Vary::Alternate,
        }
    }

    fn groove() -> GrooveFile {
        let mut verse = section("Verse", Some(Section::Verse), 16);
        verse.variations = vec![pattern("Verse B", 16)];
        verse.vary = Vary::Chain;
        verse.fill = Some(SectionFill {
            section: "Fill".into(),
            every: 4,
            at_end: false,
        });
        GrooveFile {
            version: 1,
            name: "Late train".into(),
            genre: "Rock".into(),
            description: String::new(),
            tempo: 118.0,
            time_signature: Meter {
                numerator: 4,
                denominator: 4,
            },
            part: DrumPart {
                kit: "acoustic".into(),
                sections: vec![
                    verse,
                    section("Fill", Some(Section::Fill), 16),
                    section("Breakdown", None, 8),
                ],
                order: vec![OrderStep {
                    section: "Verse".into(),
                    times: 8,
                }],
            },
        }
    }

    #[test]
    fn round_trips_without_pattern_ids() {
        let text = groove_file_to_json(&groove());
        assert!(!text.contains("\"id\""), "{text}");
        assert!(text.contains("\"kit\": \"acoustic\""), "{text}");
        assert_eq!(groove_file_from_json(&text).unwrap(), groove());
    }

    #[test]
    fn refuses_what_does_not_add_up() {
        let mut g = groove();
        g.version = 2;
        assert!(groove_file_from_json(&groove_file_to_json(&g))
            .unwrap_err()
            .contains("version"));
        let mut g = groove();
        g.tempo = 10.0;
        assert!(groove_file_from_json(&groove_file_to_json(&g)).is_err());
        let mut g = groove();
        g.part.order[0].section = "Nope".into();
        assert!(groove_file_from_json(&groove_file_to_json(&g))
            .unwrap_err()
            .contains("Nope"));
        assert!(groove_file_from_json("[]").is_err());
    }

    #[test]
    fn a_jam_plays_its_sections_by_kind() {
        let g = groove();
        let patterns = g.jam_patterns();
        let kinds: Vec<_> = patterns.iter().map(|(k, _)| *k).collect();
        // The breakdown is of no kind and half a bar: left out.
        assert_eq!(kinds, [Section::Verse, Section::Fill]);
        // The verse plays a round of its variations: A then B.
        assert_eq!(patterns[0].1.length, 32);

        let mut no_verse = groove();
        no_verse.part.sections[0].kind = Some(Section::Chorus);
        let kinds: Vec<_> = no_verse.jam_patterns().iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [Section::Chorus, Section::Fill, Section::Verse]);
    }
}
