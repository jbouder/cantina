//! Tabs: the notes of a guitar or bass part as strings and frets.
//!
//! A tab lives on the clip it was transcribed from, in
//! [`Clip::tab`](crate::Clip::tab), and as a file of its own, `<name>.tab.json`. Its
//! notes are in ticks ([`PPQ`] per quarter note) from the tab's start, at
//! the tempo it was written against, so it reads as bars and beats
//! whatever the project's tempo does later.
//!
//! On a clip, the tab is anchored to the audio, not to the timeline:
//! `origin` is the frame of the clip's file where tick 0 falls. Moving a
//! clip moves its file, and trimming changes which frames play, but
//! neither moves a frame of the file, so the tab stays on the notes it
//! describes. A note outside the trimmed region is kept, just not heard.
//!
//! Strings are numbered as tab is read: 1 is the highest-pitched string,
//! the top line. `tuning` lists the open strings from the lowest to the
//! highest, as players name a tuning (E A D G B E), so string `n` of `k`
//! is `tuning[k - n]`. Frets count from the capo: fret 0 with a capo at
//! 2 sounds the open string two semitones up.
//!
//! ```json
//! {
//!   "name": "Riff",
//!   "instrument": "guitar",
//!   "tuning": [40, 45, 50, 55, 59, 64],
//!   "tempo": { "bpm": 120, "numerator": 4, "denominator": 4 },
//!   "notes": [
//!     { "string": 6, "fret": 0, "start": 0, "length": 480, "palmMute": true },
//!     { "string": 5, "fret": 2, "start": 480, "length": 480 },
//!     { "string": 3, "fret": 7, "start": 960, "length": 1920,
//!       "bend": { "semitones": 1, "release": false }, "vibrato": true }
//!   ]
//! }
//! ```
//!
//! `tab.schema.json` describes the file. A `"$schema"` field is ignored,
//! and so is an `"origin"`, so a tab copied out of a project reads too.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Tempo, PPQ};

/// The suffix of a tab file.
pub const TAB_SUFFIX: &str = ".tab.json";

/// The highest fret a tab may name, counted from the capo.
pub const MAX_FRET: u8 = 36;

/// The largest bend, in semitones.
pub const MAX_BEND: f32 = 4.0;

/// What the tab is for. It sets how many strings it may have.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Instrument {
    /// A guitar: 6, 7 or 8 strings.
    Guitar,
    /// A bass: 4, 5 or 6 strings.
    Bass,
}

impl Instrument {
    /// The string counts this instrument may have.
    pub fn strings(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Instrument::Guitar => 6..=8,
            Instrument::Bass => 4..=6,
        }
    }

    /// Standard tuning for `strings` strings, lowest first, as MIDI
    /// notes: E A D G B E on a guitar, with a low B and then F♯ below for
    /// seven and eight; E A D G on a bass, with a low B and then a high C
    /// for five and six. `None` for a count the instrument does not have.
    pub fn standard_tuning(self, strings: usize) -> Option<Vec<u8>> {
        let tuning: &[u8] = match (self, strings) {
            (Instrument::Guitar, 6) => &[40, 45, 50, 55, 59, 64],
            (Instrument::Guitar, 7) => &[35, 40, 45, 50, 55, 59, 64],
            (Instrument::Guitar, 8) => &[30, 35, 40, 45, 50, 55, 59, 64],
            (Instrument::Bass, 4) => &[28, 33, 38, 43],
            (Instrument::Bass, 5) => &[23, 28, 33, 38, 43],
            (Instrument::Bass, 6) => &[23, 28, 33, 38, 43, 48],
            _ => return None,
        };
        Some(tuning.to_vec())
    }
}

/// A bend: the string pushed up by `semitones` after the note starts.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bend {
    /// How far up, in semitones: 1 is a half step, 2 a whole step.
    pub semitones: f32,
    /// Let back down to the fretted pitch before the note ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub release: bool,
}

/// One note: where it is played, when, and how.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabNote {
    /// The string, 1 for the highest-pitched.
    pub string: u8,
    /// The fret, counted from the capo; 0 is the open string.
    pub fret: u8,
    /// Ticks from the start of the tab, 960 per quarter note.
    pub start: u64,
    /// Ticks long.
    pub length: u64,
    /// How hard it was played, 0 to 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub velocity: f32,
    /// A bend up, and whether it comes back down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<Bend>,
    /// Slides into the next note on the same string.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slide: bool,
    /// Sounded by hammering onto the fret from the note before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hammer_on: bool,
    /// Sounded by pulling off to this fret from the note before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pull_off: bool,
    /// Damped with the picking hand at the bridge.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub palm_mute: bool,
    /// Left ringing past its length, into the notes after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub let_ring: bool,
    /// Vibrato with the fretting hand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vibrato: bool,
    /// A natural harmonic, touched over the fret rather than pressed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub harmonic: bool,
    /// A dead note: the string muted by the fretting hand, a click with
    /// no pitch. Written as an x.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dead: bool,
    /// Tapped with the picking hand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tap: bool,
}

fn full() -> f32 {
    1.0
}

fn is_full(v: &f32) -> bool {
    *v == 1.0
}

impl TabNote {
    /// A plain note at full velocity with no techniques.
    pub fn new(string: u8, fret: u8, start: u64, length: u64) -> Self {
        Self {
            string,
            fret,
            start,
            length,
            velocity: 1.0,
            bend: None,
            slide: false,
            hammer_on: false,
            pull_off: false,
            palm_mute: false,
            let_ring: false,
            vibrato: false,
            harmonic: false,
            dead: false,
            tap: false,
        }
    }

    /// The tick just after the note.
    pub fn end(&self) -> u64 {
        self.start + self.length
    }
}

/// A tab: the tuning, the capo, the tempo it was written against and the
/// notes.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    /// Shown above the tab and in exports.
    #[serde(default)]
    pub name: String,
    /// Guitar or bass, which sets how many strings it may have.
    pub instrument: Instrument,
    /// The open strings from the lowest to the highest, as MIDI notes
    /// (40 is the low E of a guitar). Its length is the string count.
    pub tuning: Vec<u8>,
    /// The fret the capo is at; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub capo: u8,
    /// The tempo and time signature the ticks were written against.
    #[serde(default)]
    pub tempo: Tempo,
    /// On a clip, the frame of the clip's audio file where tick 0 falls.
    /// Not part of a tab file.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub origin: u64,
    /// The notes, in any order.
    #[serde(default)]
    pub notes: Vec<TabNote>,
    /// For a transcribed tab, the notes as transcription heard them,
    /// before they were quantized to the grid and fitted to strings, so
    /// either can be redone without transcribing again. Empty for a tab
    /// written by hand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<HeardNote>,
}

/// A note as transcription heard it: a pitch and a time in seconds, not
/// yet on a beat or a string.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeardNote {
    /// The MIDI note it started on.
    pub pitch: u8,
    /// Seconds after the tab's tick 0 that it starts.
    pub onset: f64,
    /// Seconds long.
    pub length: f64,
    /// How loud it was, 0 to 1.
    pub velocity: f32,
    /// The furthest the pitch moved from `pitch` while it sounded, in
    /// semitones: a bend when positive. 0 when it held still.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bend: f32,
}

fn is_zero<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

impl Tab {
    /// An empty tab in standard tuning.
    pub fn new(name: impl Into<String>, instrument: Instrument, strings: usize) -> Option<Self> {
        Some(Self {
            name: name.into(),
            instrument,
            tuning: instrument.standard_tuning(strings)?,
            capo: 0,
            tempo: Tempo::default(),
            origin: 0,
            notes: Vec::new(),
            heard: Vec::new(),
        })
    }

    /// How many strings the tab has.
    pub fn strings(&self) -> usize {
        self.tuning.len()
    }

    /// The open pitch of string `string` (1 is the highest), as a MIDI
    /// note, without the capo.
    pub fn open_string(&self, string: u8) -> Option<u8> {
        let n = self.tuning.len();
        let s = string as usize;
        (1..=n).contains(&s).then(|| self.tuning[n - s])
    }

    /// The MIDI note a note sounds, before any bend: the open string,
    /// plus the capo, plus the fret. `None` for a dead note or a string
    /// the tab does not have.
    pub fn pitch(&self, note: &TabNote) -> Option<u8> {
        if note.dead {
            return None;
        }
        let open = self.open_string(note.string)?;
        Some(open.saturating_add(self.capo).saturating_add(note.fret))
    }

    /// Ticks to seconds from the tab's start, at its tempo.
    pub fn seconds(&self, ticks: u64) -> f64 {
        ticks as f64 * 60.0 / (self.tempo.bpm * f64::from(PPQ))
    }

    /// Ticks in one bar at the tab's time signature.
    pub fn bar_ticks(&self) -> u64 {
        let beat = u64::from(PPQ) * 4 / u64::from(self.tempo.denominator.max(1));
        beat * u64::from(self.tempo.numerator)
    }

    /// The frame of the clip's file where a tick falls, at `sample_rate`.
    pub fn file_frame(&self, ticks: u64, sample_rate: f64) -> u64 {
        self.origin + (self.seconds(ticks) * sample_rate).round() as u64
    }

    /// The tick just after the last note.
    pub fn end(&self) -> u64 {
        self.notes.iter().map(TabNote::end).max().unwrap_or(0)
    }

    /// Notes that overlap an earlier note on the same string, as pairs
    /// of indices into `notes`. A string sounds one note at a time, so
    /// these are allowed but almost always a mistake.
    pub fn overlaps(&self) -> Vec<(usize, usize)> {
        let mut order: Vec<usize> = (0..self.notes.len()).collect();
        order.sort_by_key(|&i| (self.notes[i].string, self.notes[i].start));
        order
            .windows(2)
            .filter_map(|w| {
                let (a, b) = (&self.notes[w[0]], &self.notes[w[1]]);
                (a.string == b.string && b.start < a.end()).then_some((w[0], w[1]))
            })
            .collect()
    }

    /// Checks what the types cannot: a string count the instrument has,
    /// tunings and frets in range, notes on strings that exist with a
    /// length, velocities from 0 to 1 and bends of a sensible size.
    pub fn validate(&self) -> Result<(), String> {
        let label = if self.name.is_empty() {
            "the tab".to_string()
        } else {
            format!("tab {:?}", self.name)
        };
        let strings = self.instrument.strings();
        if !strings.contains(&self.tuning.len()) {
            return Err(format!(
                "{label} has {} strings; a {} has {} to {}",
                self.tuning.len(),
                match self.instrument {
                    Instrument::Guitar => "guitar",
                    Instrument::Bass => "bass",
                },
                strings.start(),
                strings.end()
            ));
        }
        if let Some(t) = self.tuning.iter().find(|&&t| t > 127) {
            return Err(format!(
                "{label} tunes a string to {t}, above MIDI note 127"
            ));
        }
        if self.capo > MAX_FRET {
            return Err(format!(
                "{label} has a capo at fret {}, past {MAX_FRET}",
                self.capo
            ));
        }
        if !(self.tempo.bpm.is_finite() && self.tempo.bpm > 0.0) {
            return Err(format!("{label} has an unusable tempo, {}", self.tempo.bpm));
        }
        if self.tempo.numerator == 0 || self.tempo.denominator == 0 {
            return Err(format!("{label} has a zero in its time signature"));
        }
        for (i, note) in self.notes.iter().enumerate() {
            let at = format!("{label} note {} (string {})", i + 1, note.string);
            if note.string == 0 || usize::from(note.string) > self.tuning.len() {
                return Err(format!(
                    "{label} note {} is on string {}, but strings run from 1 to {}",
                    i + 1,
                    note.string,
                    self.tuning.len()
                ));
            }
            if note.fret > MAX_FRET {
                return Err(format!("{at} is at fret {}, past {MAX_FRET}", note.fret));
            }
            if note.length == 0 {
                return Err(format!("{at} has no length"));
            }
            if !(0.0..=1.0).contains(&note.velocity) {
                return Err(format!("{at} has a velocity outside 0 to 1"));
            }
            if let Some(bend) = note.bend {
                if !(bend.semitones > 0.0 && bend.semitones <= MAX_BEND) {
                    return Err(format!(
                        "{at} bends {} semitones; a bend is more than 0 and at most {MAX_BEND}",
                        bend.semitones
                    ));
                }
            }
            if note.hammer_on && note.pull_off {
                return Err(format!("{at} is both a hammer-on and a pull-off"));
            }
        }
        for (i, note) in self.heard.iter().enumerate() {
            let at = format!("{label} heard note {}", i + 1);
            if note.pitch > 127 {
                return Err(format!(
                    "{at} has pitch {}, above MIDI note 127",
                    note.pitch
                ));
            }
            if !(note.onset.is_finite() && note.onset >= 0.0) {
                return Err(format!("{at} starts at {} s, before the tab", note.onset));
            }
            if !(note.length.is_finite() && note.length > 0.0) {
                return Err(format!("{at} has no length"));
            }
            if !(0.0..=1.0).contains(&note.velocity) {
                return Err(format!("{at} has a velocity outside 0 to 1"));
            }
            if !(note.bend.is_finite() && note.bend.abs() <= MAX_BEND) {
                return Err(format!(
                    "{at} bends {} semitones, past {MAX_BEND}",
                    note.bend
                ));
            }
        }
        Ok(())
    }
}

/// Reads and validates a tab file. Any `origin` is dropped: a file is
/// not on a clip until it is attached to one.
pub fn tab_from_json(text: &str) -> Result<Tab, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("a tab file holds one JSON object".into());
    };
    object.remove("$schema");
    object.remove("origin");
    let tab: Tab =
        serde_json::from_value(value).map_err(|e| format!("does not match the tab format: {e}"))?;
    tab.validate()?;
    Ok(tab)
}

/// A tab as a file: pretty JSON without its origin.
pub fn tab_to_json(tab: &Tab) -> String {
    let file = Tab {
        origin: 0,
        ..tab.clone()
    };
    let mut text = serde_json::to_string_pretty(&file).expect("a tab serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff() -> Tab {
        let mut tab = Tab::new("Riff", Instrument::Guitar, 6).unwrap();
        tab.notes = vec![
            TabNote {
                palm_mute: true,
                ..TabNote::new(6, 0, 0, 480)
            },
            TabNote::new(5, 2, 480, 480),
            TabNote {
                bend: Some(Bend {
                    semitones: 2.0,
                    release: true,
                }),
                vibrato: true,
                ..TabNote::new(3, 7, 960, 1920)
            },
        ];
        tab
    }

    #[test]
    fn strings_count_from_the_top() {
        let tab = riff();
        assert_eq!(tab.open_string(1), Some(64));
        assert_eq!(tab.open_string(6), Some(40));
        assert_eq!(tab.open_string(7), None);
        assert_eq!(tab.open_string(0), None);
        assert_eq!(tab.pitch(&tab.notes[1]), Some(47));
    }

    #[test]
    fn the_capo_raises_every_fret() {
        let mut tab = riff();
        tab.capo = 2;
        assert_eq!(tab.pitch(&TabNote::new(6, 0, 0, 1)), Some(42));
        let dead = TabNote {
            dead: true,
            ..TabNote::new(6, 0, 0, 1)
        };
        assert_eq!(tab.pitch(&dead), None);
    }

    #[test]
    fn every_instrument_and_string_count_has_a_standard_tuning() {
        for instrument in [Instrument::Guitar, Instrument::Bass] {
            for n in instrument.strings() {
                let tab = Tab::new("", instrument, n).unwrap();
                assert_eq!(tab.strings(), n);
                tab.validate().unwrap();
                assert!(tab.tuning.windows(2).all(|w| w[0] < w[1]));
            }
        }
        assert!(Tab::new("", Instrument::Guitar, 4).is_none());
        assert!(Tab::new("", Instrument::Bass, 7).is_none());
    }

    #[test]
    fn any_tuning_reads() {
        // Drop D, DADGAD and a baritone B standard all validate.
        for tuning in [
            vec![38, 45, 50, 55, 59, 64],
            vec![38, 45, 50, 55, 57, 62],
            vec![35, 40, 45, 50, 54, 59],
        ] {
            let tab = Tab { tuning, ..riff() };
            tab.validate().unwrap();
        }
    }

    #[test]
    fn validate_names_what_is_wrong() {
        type Break = fn(&mut Tab);
        let cases: [(Break, &str); 7] = [
            (
                |t| t.tuning.truncate(5),
                "has 5 strings; a guitar has 6 to 8",
            ),
            (|t| t.notes[0].string = 7, "strings run from 1 to 6"),
            (|t| t.notes[0].fret = 40, "past 36"),
            (|t| t.notes[0].length = 0, "has no length"),
            (|t| t.notes[0].velocity = 1.5, "velocity outside 0 to 1"),
            (
                |t| t.notes[2].bend.as_mut().unwrap().semitones = 6.0,
                "bends 6 semitones",
            ),
            (|t| t.tempo.bpm = 0.0, "unusable tempo"),
        ];
        for (break_it, message) in cases {
            let mut tab = riff();
            break_it(&mut tab);
            let err = tab.validate().unwrap_err();
            assert!(err.contains(message), "{err:?} lacks {message:?}");
        }
        let mut bass = Tab::new("", Instrument::Bass, 4).unwrap();
        bass.tuning.push(48);
        bass.tuning.push(53);
        bass.tuning.push(58);
        assert!(bass.validate().unwrap_err().contains("a bass has 4 to 6"));
    }

    #[test]
    fn a_file_round_trips_without_its_origin() {
        let mut tab = riff();
        tab.origin = 12_000;
        let text = tab_to_json(&tab);
        assert!(!text.contains("origin"));
        assert!(!text.contains("velocity"), "full velocity is left out");
        assert!(
            !text.contains("slide"),
            "techniques that are off are left out"
        );
        let back = tab_from_json(&text).unwrap();
        assert_eq!(back, Tab { origin: 0, ..tab });
    }

    #[test]
    fn a_file_with_schema_and_origin_reads() {
        let text = r#"{
            "$schema": "https://example.com/tab.schema.json",
            "instrument": "bass",
            "tuning": [28, 33, 38, 43],
            "origin": 99,
            "notes": [{ "string": 4, "fret": 0, "start": 0, "length": 960 }]
        }"#;
        let tab = tab_from_json(text).unwrap();
        assert_eq!(tab.origin, 0);
        assert_eq!(tab.tempo, Tempo::default());
        assert_eq!(tab.pitch(&tab.notes[0]), Some(28));
        assert!(tab_from_json("[]").unwrap_err().contains("one JSON object"));
        assert!(tab_from_json(r#"{"instrument":"ukulele","tuning":[]}"#)
            .unwrap_err()
            .contains("does not match the tab format"));
    }

    #[test]
    fn heard_notes_are_kept_and_checked() {
        let mut tab = riff();
        tab.heard = vec![HeardNote {
            pitch: 47,
            onset: 0.25,
            length: 0.5,
            velocity: 0.8,
            bend: 0.0,
        }];
        let text = tab_to_json(&tab);
        assert!(!text.contains("\"bend\": 0"), "a still note writes no bend");
        assert_eq!(tab_from_json(&text).unwrap(), tab);
        tab.heard[0].onset = -1.0;
        assert!(tab.validate().unwrap_err().contains("before the tab"));
        tab.heard[0].onset = 0.0;
        tab.heard[0].length = 0.0;
        assert!(tab
            .validate()
            .unwrap_err()
            .contains("heard note 1 has no length"));
        // A hand-written tab writes no heard field at all.
        assert!(!tab_to_json(&riff()).contains("heard"));
    }

    #[test]
    fn overlaps_on_a_string_are_found() {
        let mut tab = riff();
        assert!(tab.overlaps().is_empty());
        tab.notes.push(TabNote::new(6, 3, 240, 480));
        assert_eq!(tab.overlaps(), vec![(0, 3)]);
    }

    #[test]
    fn ticks_become_seconds_and_file_frames() {
        let mut tab = riff();
        tab.origin = 1000;
        // 120 bpm: a quarter note is half a second.
        assert_eq!(tab.seconds(960), 0.5);
        assert_eq!(tab.file_frame(960, 48_000.0), 25_000);
        assert_eq!(tab.bar_ticks(), 3840);
        tab.tempo.numerator = 6;
        tab.tempo.denominator = 8;
        assert_eq!(tab.bar_ticks(), 2880);
    }
}
