//! Jams: what plays along while you practise. A jam is a drum groove, a
//! tempo, a key and the rig you play through; it is not a project and
//! jamming never edits one.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Blues in A",
//!   "tempo": 96,
//!   "timeSignature": { "numerator": 4, "denominator": 4 },
//!   "key": { "tonic": "A", "mode": "major" },
//!   "drums": { "kit": "studio", "groove": "Blues shuffle" },
//!   "rig": "blues"
//! }
//! ```
//!
//! The drums play a stock groove's sections, named by its genre, or
//! patterns of the jam's own, by section; a section the jam has its own
//! pattern for plays that one. The rig is named by its file's name less
//! `.rig.json`, stock or the user's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Pattern, PPQ};
use crate::pattern_file::{Meter, Section};

/// The version a jam is written at.
pub const JAM_VERSION: u32 = 1;

/// The slowest and fastest a jam plays, in beats per minute.
pub const JAM_TEMPO: (f64, f64) = (30.0, 300.0);

/// What plays along while you practise.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Jam {
    /// The format version, 1.
    pub version: u32,
    /// Shown in the jam list.
    pub name: String,
    /// The style it is for: `Blues`, `Rock`…
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub genre: String,
    /// One line on what it is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Beats per minute, 30 to 300.
    pub tempo: f64,
    /// The meter every section plays in.
    #[serde(default = "common_time")]
    pub time_signature: Meter,
    /// The key the backing plays in.
    #[serde(default)]
    pub key: Key,
    /// The drums.
    pub drums: JamDrums,
    /// The rig the input plays through, by its file's name less
    /// `.rig.json`; none plays the input dry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<String>,
}

fn common_time() -> Meter {
    Meter {
        numerator: 4,
        denominator: 4,
    }
}

/// The drums of a jam.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JamDrums {
    /// The kit, by id.
    pub kit: String,
    /// A stock groove, by its genre (`Rock`, `Blues shuffle`): each
    /// section plays that groove's pattern for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove: Option<String>,
    /// Patterns of the jam's own, by section, played in place of the
    /// groove's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub patterns: BTreeMap<Section, Pattern>,
    /// The drums' level in decibels, −60 to 12.
    #[serde(default)]
    pub level_db: f32,
}

/// A key: its tonic and whether it is major or minor.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    /// A note name: `A`, `F#`, `Bb`.
    pub tonic: String,
    /// Major or minor.
    pub mode: Mode,
}

impl Default for Key {
    fn default() -> Self {
        Self {
            tonic: "E".into(),
            mode: Mode::Minor,
        }
    }
}

/// Major or minor.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Ionian.
    Major,
    /// Natural minor.
    Minor,
}

/// The pitch class of a note name, 0 for C to 11 for B: a letter, then
/// any number of `#` or `b`.
pub fn pitch_class(name: &str) -> Option<u8> {
    let mut chars = name.trim().chars();
    let base: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut pc = base;
    for c in chars {
        pc += match c {
            '#' | '♯' => 1,
            'b' | '♭' => -1,
            _ => return None,
        };
    }
    Some(pc.rem_euclid(12) as u8)
}

impl Jam {
    /// A jam at a tempo on a kit, with nothing else chosen yet.
    pub fn new(name: impl Into<String>, tempo: f64, kit: impl Into<String>) -> Self {
        Self {
            version: JAM_VERSION,
            name: name.into(),
            genre: String::new(),
            description: String::new(),
            tempo,
            time_signature: common_time(),
            key: Key::default(),
            drums: JamDrums {
                kit: kit.into(),
                groove: None,
                patterns: BTreeMap::new(),
                level_db: 0.0,
            },
            rig: None,
        }
    }

    /// Ticks in one bar of the jam's meter.
    pub fn bar_ticks(&self) -> u64 {
        u64::from(PPQ) * 4 / u64::from(self.time_signature.denominator.max(1))
            * u64::from(self.time_signature.numerator.max(1))
    }

    /// The rules beyond the shape: a version this build reads, a name, a
    /// tempo in range, a meter Cantina plays, a key that names a note,
    /// a kit, and patterns that are valid and whole bars long.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version == 0 || self.version > JAM_VERSION {
            problems.push(format!(
                "version {} is not one this Cantina reads (1 to {JAM_VERSION})",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            problems.push("the name is empty".into());
        }
        let (slow, fast) = JAM_TEMPO;
        if !(self.tempo.is_finite() && (slow..=fast).contains(&self.tempo)) {
            problems.push(format!(
                "the tempo {} is outside {slow} to {fast} bpm",
                self.tempo
            ));
        }
        let Meter {
            numerator,
            denominator,
        } = self.time_signature;
        if !(1..=16).contains(&numerator) || !matches!(denominator, 2 | 4 | 8 | 16) {
            problems.push(format!(
                "the time signature {numerator}/{denominator} is not one Cantina plays"
            ));
        }
        if pitch_class(&self.key.tonic).is_none() {
            problems.push(format!(
                "the key's tonic {:?} is not a note",
                self.key.tonic
            ));
        }
        if self.drums.kit.trim().is_empty() {
            problems.push("the drums have no kit".into());
        }
        if !(self.drums.level_db.is_finite() && (-60.0..=12.0).contains(&self.drums.level_db)) {
            problems.push(format!(
                "the drums' level {} dB is outside −60 to 12",
                self.drums.level_db
            ));
        }
        if self.drums.groove.is_none() && self.drums.patterns.is_empty() {
            problems.push("the drums have neither a groove nor patterns".into());
        }
        let bar = self.bar_ticks();
        for (section, pattern) in &self.drums.patterns {
            let at = format!("the {} pattern", section_name(*section));
            if let Err(e) = pattern.validate() {
                problems.push(format!("{at}: {e}"));
            } else if bar > 0 && pattern.ticks() % bar != 0 {
                problems.push(format!(
                    "{at} is not a whole number of {numerator}/{denominator} bars"
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

/// A section's name as files and the UI write it.
pub fn section_name(section: Section) -> &'static str {
    match section {
        Section::Intro => "intro",
        Section::Verse => "verse",
        Section::Chorus => "chorus",
        Section::Bridge => "bridge",
        Section::Fill => "fill",
        Section::Ending => "ending",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PatternId, PatternRow};

    fn jam() -> Jam {
        let mut j = Jam::new("Blues in A", 96.0, "studio");
        j.drums.groove = Some("Blues shuffle".into());
        j.key = Key {
            tonic: "A".into(),
            mode: Mode::Major,
        };
        j.rig = Some("blues".into());
        j
    }

    #[test]
    fn note_names_are_pitch_classes() {
        assert_eq!(pitch_class("C"), Some(0));
        assert_eq!(pitch_class("a"), Some(9));
        assert_eq!(pitch_class("F#"), Some(6));
        assert_eq!(pitch_class("Bb"), Some(10));
        assert_eq!(pitch_class("Cb"), Some(11));
        assert_eq!(pitch_class("E♭"), Some(3));
        assert_eq!(pitch_class("H"), None);
        assert_eq!(pitch_class(""), None);
        assert_eq!(pitch_class("C+"), None);
    }

    #[test]
    fn round_trips_through_json() {
        let j = jam();
        let text = serde_json::to_string(&j).unwrap();
        assert!(text.contains(r#""groove":"Blues shuffle""#), "{text}");
        assert!(!text.contains("patterns"), "{text}");
        let back: Jam = serde_json::from_str(&text).unwrap();
        assert_eq!(back, j);
        assert_eq!(back.validate(), Ok(()));
        assert_eq!(back.bar_ticks(), u64::from(PPQ) * 4);
    }

    #[test]
    fn reports_every_problem() {
        let mut j = jam();
        j.version = 9;
        j.name = " ".into();
        j.tempo = 500.0;
        j.time_signature = Meter {
            numerator: 4,
            denominator: 3,
        };
        j.key.tonic = "X".into();
        j.drums.groove = None;
        let problems = j.validate().unwrap_err();
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert!(problems[5].contains("neither a groove"), "{problems:?}");
    }

    #[test]
    fn its_own_patterns_are_whole_bars() {
        let mut j = jam();
        let mut p = Pattern::new(PatternId(0), "Half bar");
        p.steps_per_beat = 4;
        p.length = 8;
        p.rows.push(PatternRow::new("kick", vec![1.0; 8]));
        j.drums.patterns.insert(Section::Verse, p.clone());
        assert!(j.validate().unwrap_err()[0].contains("whole number of 4/4 bars"));
        p.length = 16;
        p.rows[0].steps = vec![1.0; 16];
        j.drums.patterns.insert(Section::Verse, p);
        assert_eq!(j.validate(), Ok(()));
    }
}
