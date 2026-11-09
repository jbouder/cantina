//! Jams: what plays along while you practise, as a file of its own,
//! `<name>.jam.json`. A jam is a drum groove, a backing, a tempo, a key
//! and the rig you play through; it is not a project and jamming never
//! edits one. `jam.schema.json` describes the file.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Blues in A",
//!   "tempo": 96,
//!   "timeSignature": { "numerator": 4, "denominator": 4 },
//!   "key": { "tonic": "A", "mode": "major" },
//!   "drums": { "kit": "studio", "groove": "Blues shuffle" },
//!   "rig": "blues",
//!   "backing": {
//!     "kind": "progression",
//!     "chords": "I7 | IV7 | I7 | V7",
//!     "instrument": "bass",
//!     "rhythm": "quarter",
//!     "rig": "bass-clean"
//!   }
//! }
//! ```
//!
//! The drums play a stock groove's sections, named by its genre, or
//! patterns of the jam's own, by section, written as pattern files are,
//! without an id; a section the jam has its own pattern for plays that
//! one. The rig is named by its file's name less
//! `.rig.json`, stock or the user's.
//!
//! The backing is a chord progression (see [`crate::parse_progression`])
//! played by a built-in guitar or bass through a rig, which follows the
//! jam's tempo; or audio, a file or a project's track, which plays at its
//! own tempo with the drums following it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Pattern, PPQ};
use crate::pattern_file::{Meter, Section};
use crate::tab::Instrument;

/// The version a jam is written at.
pub const JAM_VERSION: u32 = 1;

/// What a jam file's name ends with.
pub const JAM_SUFFIX: &str = ".jam.json";

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
    /// What plays with the drums.
    #[serde(default, skip_serializing_if = "Backing::is_none")]
    pub backing: Backing,
}

/// What plays along with the drums.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Backing {
    /// The drums alone.
    #[default]
    None,
    /// A chord progression in the jam's key, played by a built-in guitar
    /// or bass through a rig. It follows the jam's tempo.
    Progression {
        /// Roman numerals or chord names, `|` between bars.
        chords: String,
        /// Which voice plays it.
        instrument: Instrument,
        /// How often each chord is played.
        #[serde(default)]
        rhythm: Rhythm,
        /// The rig the voice plays through, by its file's name less
        /// `.rig.json`; none is the voice dry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rig: Option<String>,
        /// Its level in decibels, −60 to 12.
        #[serde(default)]
        level_db: f32,
    },
    /// A WAV file, played from the jam's first bar at its own tempo,
    /// round and round; the drums play at its tempo.
    Audio {
        /// Where the file is.
        file: String,
        /// The file's tempo, which the jam takes.
        bpm: f64,
        /// Its level in decibels, −60 to 12.
        #[serde(default)]
        level_db: f32,
    },
    /// An audio track of a project, its clips through its chain, round and
    /// round from the start to the end of its last clip; the drums play
    /// at the project's tempo.
    Track {
        /// The project's name.
        project: String,
        /// The track's name.
        track: String,
        /// Its level in decibels, −60 to 12.
        #[serde(default)]
        level_db: f32,
    },
}

impl Backing {
    /// True for the drums alone.
    pub fn is_none(&self) -> bool {
        matches!(self, Backing::None)
    }

    /// True when the jam plays at the backing's tempo, not its own.
    pub fn sets_tempo(&self) -> bool {
        matches!(self, Backing::Audio { .. } | Backing::Track { .. })
    }
}

/// How often a progression's chords are played.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rhythm {
    /// Once, ringing for as long as the chord lasts.
    Whole,
    /// On every other beat.
    #[default]
    Half,
    /// On every beat.
    Quarter,
    /// Twice a beat.
    Eighth,
}

/// A jam's own patterns as pattern files write them: an id means
/// nothing outside a project, so none is written and any read is 0.
mod patterns_without_ids {
    use super::*;
    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        patterns: &BTreeMap<Section, Pattern>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut out = BTreeMap::new();
        for (section, p) in patterns {
            let mut v = serde_json::to_value(p).map_err(S::Error::custom)?;
            if let Some(o) = v.as_object_mut() {
                o.remove("id");
            }
            out.insert(*section, v);
        }
        out.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<Section, Pattern>, D::Error> {
        let raw: BTreeMap<Section, Value> = BTreeMap::deserialize(d)?;
        raw.into_iter()
            .map(|(section, mut v)| {
                if let Some(o) = v.as_object_mut() {
                    o.insert("id".into(), Value::from(0));
                }
                serde_json::from_value(v)
                    .map(|p| (section, p))
                    .map_err(D::Error::custom)
            })
            .collect()
    }
}

/// Reads and checks a jam file's text. A `"$schema"` field is ignored.
pub fn jam_from_json(text: &str) -> Result<Jam, Vec<String>> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| vec![format!("not a jam: {e}")])?;
    if let Some(o) = value.as_object_mut() {
        o.remove("$schema");
    }
    let jam: Jam = serde_json::from_value(value).map_err(|e| vec![format!("not a jam: {e}")])?;
    jam.validate()?;
    Ok(jam)
}

/// A jam as the text of its file, pointing at its schema.
pub fn jam_to_json(jam: &Jam) -> String {
    let mut value = serde_json::to_value(jam).expect("a jam serializes");
    if let Some(o) = value.as_object_mut() {
        let mut with = serde_json::Map::new();
        with.insert(
            "$schema".into(),
            Value::from("https://cantina-docs.vibeproject.workers.dev/schemas/jam.schema.json"),
        );
        with.extend(std::mem::take(o));
        *o = with;
    }
    let mut text = serde_json::to_string_pretty(&value).expect("a jam serializes");
    text.push('\n');
    text
}

/// The file name for a jam id.
pub fn jam_file_name(id: &str) -> String {
    format!("{id}{JAM_SUFFIX}")
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
    /// groove's: pattern files' objects, without an id.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "patterns_without_ids"
    )]
    #[cfg_attr(
        any(test, feature = "schema"),
        schemars(with = "BTreeMap<Section, Pattern>")
    )]
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
            backing: Backing::None,
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
        let level = |db: f32| db.is_finite() && (-60.0..=12.0).contains(&db);
        match &self.backing {
            Backing::None => {}
            Backing::Progression {
                chords, level_db, ..
            } => {
                if let Some(tonic) = pitch_class(&self.key.tonic) {
                    if let Err(p) = crate::chords::parse_progression(chords, tonic) {
                        problems.extend(p.into_iter().map(|p| format!("the backing: {p}")));
                    }
                }
                if !level(*level_db) {
                    problems.push(format!(
                        "the backing's level {level_db} dB is outside −60 to 12"
                    ));
                }
            }
            Backing::Audio {
                file,
                bpm,
                level_db,
            } => {
                if file.trim().is_empty() {
                    problems.push("the backing names no file".into());
                }
                if !(bpm.is_finite() && (slow..=fast).contains(bpm)) {
                    problems.push(format!(
                        "the backing's tempo {bpm} is outside {slow} to {fast} bpm"
                    ));
                }
                if !level(*level_db) {
                    problems.push(format!(
                        "the backing's level {level_db} dB is outside −60 to 12"
                    ));
                }
            }
            Backing::Track {
                project,
                track,
                level_db,
            } => {
                if project.trim().is_empty() || track.trim().is_empty() {
                    problems.push("the backing names no project and track".into());
                }
                if !level(*level_db) {
                    problems.push(format!(
                        "the backing's level {level_db} dB is outside −60 to 12"
                    ));
                }
            }
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
    fn a_backing_reads_and_its_chords_are_checked() {
        let text = r#"{"version":1,"name":"B","tempo":90,"drums":{"kit":"k","groove":"Rock"},
            "backing":{"kind":"progression","chords":"I | IV | V","instrument":"bass","levelDb":-3}}"#;
        let j: Jam = serde_json::from_str(text).unwrap();
        assert_eq!(
            j.backing,
            Backing::Progression {
                chords: "I | IV | V".into(),
                instrument: Instrument::Bass,
                rhythm: Rhythm::Half,
                rig: None,
                level_db: -3.0,
            }
        );
        assert_eq!(j.validate(), Ok(()));
        assert!(!j.backing.sets_tempo());
        let mut bad = j.clone();
        bad.backing = Backing::Progression {
            chords: "I | Q".into(),
            instrument: Instrument::Guitar,
            rhythm: Rhythm::Whole,
            rig: None,
            level_db: 0.0,
        };
        assert_eq!(
            bad.validate().unwrap_err(),
            vec!["the backing: \"Q\" is not a chord"]
        );
        let audio: Jam = serde_json::from_str(&text.replace(
            r#""kind":"progression","chords":"I | IV | V","instrument":"bass""#,
            r#""kind":"audio","file":"/x.wav","bpm":500"#,
        ))
        .unwrap();
        assert!(audio.backing.sets_tempo());
        assert!(audio.validate().unwrap_err()[0].contains("500"));
        // None is left out of the file.
        assert!(!serde_json::to_string(&jam()).unwrap().contains("backing"));
    }

    #[test]
    fn a_file_round_trips_without_pattern_ids() {
        let mut j = jam();
        let mut p = Pattern::new(PatternId(42), "Mine");
        p.length = 16;
        p.rows.push(PatternRow::new("kick", vec![1.0; 16]));
        j.drums.patterns.insert(Section::Chorus, p);
        let text = jam_to_json(&j);
        assert!(text.starts_with("{\n  \"$schema\""), "{text}");
        assert!(!text.contains("\"id\""), "{text}");
        let back = jam_from_json(&text).unwrap();
        assert_eq!(back.drums.patterns[&Section::Chorus].id, PatternId(0));
        assert_eq!(
            back.drums.patterns[&Section::Chorus].rows,
            j.drums.patterns[&Section::Chorus].rows
        );
        assert_eq!(jam_file_name("blues"), "blues.jam.json");
        assert!(jam_from_json("{}").unwrap_err()[0].starts_with("not a jam"));
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
