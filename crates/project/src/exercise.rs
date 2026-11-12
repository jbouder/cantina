//! Exercises: something to practice, as a file of its own,
//! `<id>.exercise.json`. An exercise is a phrase to play (notes in the
//! tab format's terms, or a sequence of chord shapes), what it teaches,
//! instructions, a tempo range to climb, a count-in, a key, the rig it is
//! heard through and a stock groove to play it over.
//! `exercise.schema.json` describes the file.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "E to A",
//!   "instrument": "guitar",
//!   "teaches": "change",
//!   "style": "Beginner",
//!   "instructions": "Two bars of E, two of A. Lift only the fingers that move.",
//!   "key": { "tonic": "E", "mode": "major" },
//!   "tempo": { "start": 60, "goal": 100, "step": 5 },
//!   "rig": "classic-rock",
//!   "drums": { "kit": "rock", "groove": "Rock" },
//!   "phrase": {
//!     "kind": "chords",
//!     "chords": [
//!       { "name": "E", "frets": [0, 2, 2, 1, 0, 0], "fingers": [0, 2, 3, 1, 0, 0],
//!         "start": 0, "length": 7680 },
//!       { "name": "A", "frets": [null, 0, 2, 2, 2, 0], "start": 7680, "length": 7680 }
//!     ],
//!     "strum": { "pattern": "D-D-DUDU", "stepsPerBeat": 2 }
//!   }
//! }
//! ```
//!
//! A phrase of notes is a tab's notes: strings count from 1, the
//! highest-pitched, frets from the capo, times in ticks (960 per quarter
//! note) from the start of the loop. A chord shape lists a fret per
//! string from the lowest string to the highest, as the tuning does, with
//! `null` for a string not played. The phrase loops every `bars` bars, or
//! every whole bar it reaches when `bars` is left out.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::jam::{pitch_class, Key, JAM_TEMPO};
use crate::model::{Tempo, PPQ};
use crate::pattern_file::Meter;
use crate::tab::{Instrument, Tab, TabNote};
use crate::tab_export::harmonic_interval;

/// The version an exercise is written at.
pub const EXERCISE_VERSION: u32 = 1;

/// What an exercise file's name ends with.
pub const EXERCISE_SUFFIX: &str = ".exercise.json";

/// The highest fret an exercise may reach, the capo's included: a
/// 24-fret neck.
pub const FRETBOARD: u8 = 24;

/// The longest count-in, in bars.
pub const MAX_COUNT_IN: u8 = 4;

/// Steps per beat a strum pattern may use.
pub const STRUM_STEPS: [u8; 6] = [1, 2, 3, 4, 6, 8];

/// Ticks a strum takes from one string to the next: about 5 ms at 120
/// bpm, so a chord sounds strummed and not struck as one.
pub const STRUM_TICKS: u64 = 10;

/// Something to practice.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    /// The format version, 1.
    pub version: u32,
    /// Shown in the exercise list.
    pub name: String,
    /// Guitar or bass.
    pub instrument: Instrument,
    /// What it teaches, which the list sorts by.
    pub teaches: Teaches,
    /// The style it is for: `Metal`, `Blues`, `Beginner`…
    pub style: String,
    /// What to do, what to listen for and what goes wrong: a sentence or
    /// a few.
    pub instructions: String,
    /// The key it is in.
    pub key: Key,
    /// The meter it is counted in.
    #[serde(default = "common_time")]
    pub time_signature: Meter,
    /// Where to start, the goal and the step between.
    pub tempo: TempoLadder,
    /// Bars of click before the first pass, 0 to 4.
    #[serde(default = "one_bar")]
    pub count_in: u8,
    /// The open strings from the lowest to the highest, as MIDI notes;
    /// left out, standard tuning for six strings, or four on a bass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<Vec<u8>>,
    /// The fret the capo is at; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub capo: u8,
    /// Bars in one pass of the loop; left out, every bar the phrase
    /// reaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bars: Option<u32>,
    /// What to play.
    pub phrase: Phrase,
    /// The rig it is heard through, the example and your input both, by
    /// its file's name less `.rig.json`; none is dry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<String>,
    /// The drums it is played over; none is the click alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drums: Option<ExerciseDrums>,
}

/// What an exercise teaches.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Teaches {
    /// A chord shape.
    Chord,
    /// Moving between chords.
    Change,
    /// A scale or arpeggio as a shape on the neck.
    Scale,
    /// A technique of either hand: picking, bends, legato, harmonics.
    Technique,
    /// A riff to learn.
    Riff,
    /// Time and feel: strumming, muting, subdivisions.
    Rhythm,
}

/// The tempos an exercise climbs through, in beats per minute.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoLadder {
    /// Where to start: playable the first day.
    pub start: f64,
    /// The goal: the tempo the music it is for is at.
    pub goal: f64,
    /// How far each step up goes.
    pub step: f64,
}

/// The drums an exercise is played over.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseDrums {
    /// The kit, by id.
    pub kit: String,
    /// A stock groove, by its genre (`Rock`, `Heavy metal`); its verse
    /// plays under the exercise.
    pub groove: String,
    /// The drums' level in decibels, −60 to 12.
    #[serde(default)]
    pub level_db: f32,
}

/// What to play: notes, as a tab has them, or chord shapes.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Phrase {
    /// Notes on strings and frets, as a tab's.
    Notes {
        /// The notes, in any order.
        notes: Vec<TabNote>,
    },
    /// Chord shapes, each held for a while and strummed.
    Chords {
        /// The shapes, in any order.
        chords: Vec<ChordShape>,
        /// How each chord is strummed while it lasts; left out, it is
        /// struck once and rings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strum: Option<Strum>,
    },
}

/// A chord shape held for a while.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordShape {
    /// What it is called: `E`, `Am`, `G/B`.
    pub name: String,
    /// A fret per string, from the lowest string to the highest as the
    /// tuning lists them, counted from the capo; `null` for a string not
    /// played, 0 for an open one.
    pub frets: Vec<Option<u8>>,
    /// Which finger holds each string, in the same order: 1 the index to
    /// 4 the little finger, 0 for none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fingers: Vec<u8>,
    /// Ticks from the start of the loop, 960 per quarter note.
    pub start: u64,
    /// Ticks it is held.
    pub length: u64,
}

/// A strumming pattern, repeated for as long as each chord lasts.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Strum {
    /// One character a step: `D` a downstroke, `U` an upstroke, `x` a
    /// muted scratch, `-` no stroke (the hand still moves). Spaces are
    /// ignored, for reading.
    pub pattern: String,
    /// Steps in a beat: 2 for eighths, 4 for sixteenths.
    pub steps_per_beat: u8,
}

impl Strum {
    /// The strokes, spaces left out.
    pub fn strokes(&self) -> impl Iterator<Item = char> + '_ {
        self.pattern.chars().filter(|c| !c.is_whitespace())
    }
}

/// A note as it sounds: when, on which string, at what pitch and how.
/// Chords are strummed, so their strings start a little apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sounded {
    /// Ticks from the start of the loop.
    pub start: u64,
    /// Ticks long.
    pub length: u64,
    /// The string, 1 for the highest-pitched.
    pub string: u8,
    /// The pitch as a MIDI note, the capo included; a harmonic at the
    /// pitch it sounds.
    pub pitch: f32,
    /// 0 to 1.
    pub velocity: f32,
    /// Semitones it bends up to; 0 for none.
    pub bend: f32,
    /// The bend comes back down before the note ends.
    pub release: bool,
    /// Vibrato.
    pub vibrato: bool,
    /// Damped at the bridge.
    pub palm_mute: bool,
    /// A muted click.
    pub dead: bool,
    /// A natural harmonic.
    pub harmonic: bool,
}

fn common_time() -> Meter {
    Meter {
        numerator: 4,
        denominator: 4,
    }
}

fn one_bar() -> u8 {
    1
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

/// Reads and checks an exercise file's text. A `"$schema"` field is
/// ignored.
pub fn exercise_from_json(text: &str) -> Result<Exercise, Vec<String>> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| vec![format!("not an exercise: {e}")])?;
    if let Some(o) = value.as_object_mut() {
        o.remove("$schema");
    }
    let exercise: Exercise =
        serde_json::from_value(value).map_err(|e| vec![format!("not an exercise: {e}")])?;
    exercise.validate()?;
    Ok(exercise)
}

/// An exercise as the text of its file, pointing at its schema.
pub fn exercise_to_json(exercise: &Exercise) -> String {
    let mut value = serde_json::to_value(exercise).expect("an exercise serializes");
    if let Some(o) = value.as_object_mut() {
        let mut with = serde_json::Map::new();
        with.insert(
            "$schema".into(),
            Value::from(
                "https://cantina-docs.vibeproject.workers.dev/schemas/exercise.schema.json",
            ),
        );
        with.extend(std::mem::take(o));
        *o = with;
    }
    let mut text = serde_json::to_string_pretty(&value).expect("an exercise serializes");
    text.push('\n');
    text
}

/// The file name for an exercise id.
pub fn exercise_file_name(id: &str) -> String {
    format!("{id}{EXERCISE_SUFFIX}")
}

impl Teaches {
    /// Its name as files and the UI write it.
    pub fn name(self) -> &'static str {
        match self {
            Teaches::Chord => "chord",
            Teaches::Change => "change",
            Teaches::Scale => "scale",
            Teaches::Technique => "technique",
            Teaches::Riff => "riff",
            Teaches::Rhythm => "rhythm",
        }
    }
}

impl Exercise {
    /// The open strings, lowest first: the file's tuning, or standard.
    pub fn tuning(&self) -> Vec<u8> {
        self.tuning.clone().unwrap_or_else(|| {
            let strings = match self.instrument {
                Instrument::Guitar => 6,
                Instrument::Bass => 4,
            };
            self.instrument
                .standard_tuning(strings)
                .expect("six and four strings are standard")
        })
    }

    /// Ticks in one beat of the exercise's meter.
    pub fn beat_ticks(&self) -> u64 {
        u64::from(PPQ) * 4 / u64::from(self.time_signature.denominator.max(1))
    }

    /// Ticks in one bar of the exercise's meter.
    pub fn bar_ticks(&self) -> u64 {
        self.beat_ticks() * u64::from(self.time_signature.numerator.max(1))
    }

    /// The tick just after the phrase's last note or chord.
    pub fn phrase_end(&self) -> u64 {
        match &self.phrase {
            Phrase::Notes { notes } => notes.iter().map(TabNote::end).max().unwrap_or(0),
            Phrase::Chords { chords, .. } => {
                chords.iter().map(|c| c.start + c.length).max().unwrap_or(0)
            }
        }
    }

    /// Ticks in one pass of the loop: `bars` bars, or every whole bar the
    /// phrase reaches, at least one.
    pub fn loop_ticks(&self) -> u64 {
        let bar = self.bar_ticks().max(1);
        match self.bars {
            Some(b) => u64::from(b.max(1)) * bar,
            None => self.phrase_end().div_ceil(bar).max(1) * bar,
        }
    }

    /// The phrase as a tab, at the start tempo: notes as they are, and
    /// each chord strike as a note on every string it plays, all on the
    /// strike's tick. What the practice view reads.
    pub fn tab(&self) -> Tab {
        Tab {
            name: self.name.clone(),
            instrument: self.instrument,
            tuning: self.tuning(),
            capo: self.capo,
            tempo: Tempo {
                bpm: self.tempo.start,
                numerator: self.time_signature.numerator,
                denominator: self.time_signature.denominator,
            },
            origin: 0,
            notes: self.played().into_iter().map(|(n, _)| n).collect(),
            heard: Vec::new(),
        }
    }

    /// Every note as it sounds, in order of start: chords strummed across
    /// their strings, low to high on a downstroke and high to low on an
    /// upstroke; harmonics at the pitch they ring at. What the tab voice
    /// plays.
    pub fn sounded(&self) -> Vec<Sounded> {
        let tab = self.tab();
        let mut out: Vec<Sounded> = self
            .played()
            .into_iter()
            .filter_map(|(note, delay)| {
                let open = tab.open_string(note.string)?;
                let above = if note.harmonic {
                    harmonic_interval(note.fret)
                } else {
                    note.fret
                };
                let delay = delay.min(note.length.saturating_sub(1));
                Some(Sounded {
                    start: note.start + delay,
                    length: note.length - delay,
                    string: note.string,
                    pitch: f32::from(open) + f32::from(self.capo) + f32::from(above),
                    velocity: note.velocity,
                    bend: note.bend.map_or(0.0, |b| b.semitones),
                    release: note.bend.is_some_and(|b| b.release),
                    vibrato: note.vibrato,
                    palm_mute: note.palm_mute,
                    dead: note.dead,
                    harmonic: note.harmonic,
                })
            })
            .collect();
        out.sort_by_key(|s| (s.start, std::cmp::Reverse(s.string)));
        out
    }

    /// The tab's notes, each with how many ticks after its start it
    /// sounds: 0, or its place in a strum.
    fn played(&self) -> Vec<(TabNote, u64)> {
        match &self.phrase {
            Phrase::Notes { notes } => notes.iter().map(|n| (*n, 0)).collect(),
            Phrase::Chords { chords, strum } => {
                let strings = self.tuning().len();
                let mut out = Vec::new();
                for chord in chords {
                    for (at, length, stroke) in self.strikes(chord, strum.as_ref()) {
                        // Strings in the shape run low to high; tab strings
                        // count from the highest.
                        let sounding: Vec<(u8, u8)> = chord
                            .frets
                            .iter()
                            .enumerate()
                            .filter_map(|(i, f)| f.map(|f| ((strings - i) as u8, f)))
                            .collect();
                        let n = sounding.len();
                        for (k, (string, fret)) in sounding.into_iter().enumerate() {
                            let order = if stroke == 'U' { n - 1 - k } else { k };
                            let note = TabNote {
                                velocity: match stroke {
                                    'U' => 0.8,
                                    'x' => 0.7,
                                    _ => 1.0,
                                },
                                dead: stroke == 'x',
                                ..TabNote::new(string, fret, at, length)
                            };
                            out.push((note, order as u64 * STRUM_TICKS));
                        }
                    }
                }
                out
            }
        }
    }

    /// When a chord is struck while it lasts, for how long each strike
    /// rings and which stroke it is: once, down, without a strum.
    fn strikes(&self, chord: &ChordShape, strum: Option<&Strum>) -> Vec<(u64, u64, char)> {
        let end = chord.start + chord.length;
        let Some(strum) = strum else {
            return vec![(chord.start, chord.length, 'D')];
        };
        let strokes: Vec<char> = strum.strokes().collect();
        let step = self.beat_ticks() / u64::from(strum.steps_per_beat.max(1));
        if strokes.is_empty() || step == 0 {
            return vec![(chord.start, chord.length, 'D')];
        }
        let mut hits: Vec<(u64, char)> = Vec::new();
        let mut at = chord.start;
        let mut i = 0;
        while at < end {
            let c = strokes[i % strokes.len()];
            if matches!(c, 'D' | 'U' | 'x') {
                hits.push((at, c));
            }
            at += step;
            i += 1;
        }
        hits.iter()
            .enumerate()
            .map(|(k, &(at, c))| {
                let next = hits.get(k + 1).map_or(end, |h| h.0);
                (at, next - at, c)
            })
            .collect()
    }

    /// The rules beyond the shape. Every problem, the first first: a
    /// version this build reads, a name, a style and instructions, a
    /// tempo ladder that climbs inside 30 to 300 bpm, a count-in, a
    /// meter, a key, a tuning the instrument has, notes and chords on the
    /// fretboard, a strum that reads, and levels in range.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version == 0 || self.version > EXERCISE_VERSION {
            problems.push(format!(
                "version {} is not one this Cantina reads (1 to {EXERCISE_VERSION})",
                self.version
            ));
        }
        for (value, problem) in [
            (&self.name, "the name is empty"),
            (&self.style, "the style is empty"),
            (&self.instructions, "the instructions are empty"),
        ] {
            if value.trim().is_empty() {
                problems.push(problem.into());
            }
        }
        let (slow, fast) = JAM_TEMPO;
        let TempoLadder { start, goal, step } = self.tempo;
        let in_range = |t: f64| t.is_finite() && (slow..=fast).contains(&t);
        for (what, t) in [("start", start), ("goal", goal)] {
            if !in_range(t) {
                problems.push(format!(
                    "the {what} tempo {t} is outside {slow} to {fast} bpm"
                ));
            }
        }
        if in_range(start) && in_range(goal) {
            if goal < start {
                problems.push(format!(
                    "the goal tempo {goal} is below the start tempo {start}"
                ));
            } else if !(step.is_finite() && step > 0.0) {
                problems.push(format!("the tempo step {step} is not above 0"));
            } else if goal > start && step > goal - start {
                problems.push(format!(
                    "the tempo step {step} is more than the {} bpm from start to goal",
                    goal - start
                ));
            }
        }
        if self.count_in > MAX_COUNT_IN {
            problems.push(format!(
                "a count-in of {} bars is more than {MAX_COUNT_IN}",
                self.count_in
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
        let tuning = self.tuning();
        let instrument = match self.instrument {
            Instrument::Guitar => "guitar",
            Instrument::Bass => "bass",
        };
        let counts = self.instrument.strings();
        let strings_ok = counts.contains(&tuning.len());
        if !strings_ok {
            problems.push(format!(
                "the tuning has {} strings; a {instrument} has {} to {}",
                tuning.len(),
                counts.start(),
                counts.end()
            ));
        }
        if let Some(t) = tuning.iter().find(|&&t| t > 127) {
            problems.push(format!(
                "the tuning has a string at {t}, above MIDI note 127"
            ));
        }
        if self.capo >= FRETBOARD {
            problems.push(format!(
                "the capo at fret {} is off a {FRETBOARD}-fret neck",
                self.capo
            ));
        }
        let top = FRETBOARD.saturating_sub(self.capo);
        let neck = |fret: u8| {
            if self.capo > 0 {
                format!(
                    "fret {fret} above a capo at {} is off a {FRETBOARD}-fret neck",
                    self.capo
                )
            } else {
                format!("fret {fret} is off a {FRETBOARD}-fret neck")
            }
        };
        match &self.phrase {
            Phrase::Notes { notes } => {
                if notes.is_empty() {
                    problems.push("the phrase has no notes".into());
                }
                for (i, note) in notes.iter().enumerate() {
                    let at = format!(
                        "note {} (string {}, fret {})",
                        i + 1,
                        note.string,
                        note.fret
                    );
                    if note.string == 0 || usize::from(note.string) > tuning.len() {
                        problems.push(format!(
                            "note {} is on string {}, but the {instrument} has strings 1 to {}",
                            i + 1,
                            note.string,
                            tuning.len()
                        ));
                    } else if note.fret > top {
                        problems.push(format!("{at}: {}", neck(note.fret)));
                    }
                }
                if strings_ok && problems.is_empty() {
                    if let Err(e) = self.tab().validate() {
                        problems.push(e);
                    }
                }
            }
            Phrase::Chords { chords, strum } => {
                if chords.is_empty() {
                    problems.push("the phrase has no chords".into());
                }
                for (i, chord) in chords.iter().enumerate() {
                    let at = if chord.name.trim().is_empty() {
                        problems.push(format!("chord {} has no name", i + 1));
                        format!("chord {}", i + 1)
                    } else {
                        format!("chord {} ({})", i + 1, chord.name)
                    };
                    if chord.frets.len() != tuning.len() {
                        problems.push(format!(
                            "{at} has {} strings; the {instrument} has {}",
                            chord.frets.len(),
                            tuning.len()
                        ));
                    }
                    if chord.frets.iter().all(Option::is_none) {
                        problems.push(format!("{at} plays no strings"));
                    }
                    if let Some(f) = chord.frets.iter().flatten().find(|&&f| f > top) {
                        problems.push(format!("{at}: {}", neck(*f)));
                    }
                    if !chord.fingers.is_empty() && chord.fingers.len() != chord.frets.len() {
                        problems.push(format!(
                            "{at} has {} fingers for {} strings",
                            chord.fingers.len(),
                            chord.frets.len()
                        ));
                    }
                    if let Some(f) = chord.fingers.iter().find(|&&f| f > 4) {
                        problems.push(format!("{at} names finger {f}; fingers are 0 to 4"));
                    }
                    if chord.length == 0 {
                        problems.push(format!("{at} has no length"));
                    }
                }
                if let Some(strum) = strum {
                    if !STRUM_STEPS.contains(&strum.steps_per_beat) {
                        problems.push(format!(
                            "the strum's {} steps a beat is not one of 1, 2, 3, 4, 6 or 8",
                            strum.steps_per_beat
                        ));
                    }
                    if let Some(c) = strum
                        .strokes()
                        .find(|c| !matches!(c, 'D' | 'U' | 'x' | '-'))
                    {
                        problems.push(format!("the strum has {c:?}; a stroke is D, U, x or -"));
                    } else if !strum.strokes().any(|c| c != '-') {
                        problems.push("the strum has no strokes".into());
                    }
                }
            }
        }
        if let Some(bars) = self.bars {
            let end = self.phrase_end();
            if bars == 0 {
                problems.push("the loop is 0 bars".into());
            } else if end > u64::from(bars) * self.bar_ticks() {
                problems.push(format!(
                    "the phrase runs to tick {end}, past the loop's {bars} bars"
                ));
            }
        }
        if let Some(rig) = &self.rig {
            if rig.trim().is_empty() {
                problems.push("the rig is named but empty".into());
            }
        }
        if let Some(drums) = &self.drums {
            if drums.kit.trim().is_empty() {
                problems.push("the drums have no kit".into());
            }
            if drums.groove.trim().is_empty() {
                problems.push("the drums have no groove".into());
            }
            if !(drums.level_db.is_finite() && (-60.0..=12.0).contains(&drums.level_db)) {
                problems.push(format!(
                    "the drums' level {} dB is outside −60 to 12",
                    drums.level_db
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jam::Mode;

    fn chords() -> Exercise {
        exercise_from_json(
            r#"{
              "version": 1, "name": "E to A", "instrument": "guitar", "teaches": "change",
              "style": "Beginner", "instructions": "Two bars each.",
              "key": { "tonic": "E", "mode": "major" },
              "tempo": { "start": 60, "goal": 100, "step": 5 },
              "rig": "classic-rock", "drums": { "kit": "rock", "groove": "Rock" },
              "phrase": { "kind": "chords", "chords": [
                { "name": "E", "frets": [0, 2, 2, 1, 0, 0], "fingers": [0, 2, 3, 1, 0, 0],
                  "start": 0, "length": 7680 },
                { "name": "A", "frets": [null, 0, 2, 2, 2, 0], "start": 7680, "length": 7680 }
              ] }
            }"#,
        )
        .unwrap()
    }

    fn notes() -> Exercise {
        let mut e = chords();
        e.teaches = Teaches::Scale;
        e.phrase = Phrase::Notes {
            notes: vec![
                TabNote::new(6, 0, 0, 480),
                TabNote {
                    harmonic: true,
                    ..TabNote::new(6, 12, 480, 480)
                },
            ],
        };
        e
    }

    #[test]
    fn reads_and_writes_with_its_defaults() {
        let e = chords();
        assert_eq!(e.count_in, 1);
        assert_eq!(e.time_signature, common_time());
        assert_eq!(e.tuning(), vec![40, 45, 50, 55, 59, 64]);
        assert_eq!(
            e.key,
            Key {
                tonic: "E".into(),
                mode: Mode::Major
            }
        );
        let text = exercise_to_json(&e);
        assert!(text.starts_with("{\n  \"$schema\""), "{text}");
        assert!(!text.contains("\"capo\""), "{text}");
        assert_eq!(exercise_from_json(&text).unwrap(), e);
        assert_eq!(exercise_file_name("e-to-a"), "e-to-a.exercise.json");
        assert!(exercise_from_json("{}").unwrap_err()[0].starts_with("not an exercise"));
        let mut bass = e.clone();
        bass.instrument = Instrument::Bass;
        assert_eq!(bass.tuning(), vec![28, 33, 38, 43]);
    }

    #[test]
    fn loops_over_whole_bars() {
        let e = chords();
        assert_eq!(e.bar_ticks(), 3840);
        assert_eq!(e.phrase_end(), 15360);
        assert_eq!(e.loop_ticks(), 15360);
        let mut n = notes();
        assert_eq!(n.loop_ticks(), 3840);
        n.bars = Some(2);
        assert_eq!(n.loop_ticks(), 7680);
    }

    #[test]
    fn a_chord_without_a_strum_is_struck_once_low_to_high() {
        let e = chords();
        let tab = e.tab();
        // E plays six strings, A five.
        assert_eq!(tab.notes.len(), 11);
        assert!(tab.notes[..6]
            .iter()
            .all(|n| n.start == 0 && n.length == 7680));
        let s = e.sounded();
        let e_chord: Vec<_> = s.iter().filter(|n| n.start < 7680).collect();
        assert_eq!(e_chord.len(), 6);
        assert_eq!(e_chord[0].string, 6);
        assert_eq!(e_chord[0].pitch, 40.0);
        assert_eq!(e_chord[5].string, 1);
        assert_eq!(e_chord[5].start, 5 * STRUM_TICKS);
        assert_eq!(e_chord[5].length, 7680 - 5 * STRUM_TICKS);
        // The A's open A is its lowest string, the 5th.
        let a = s.iter().find(|n| n.start == 7680).unwrap();
        assert_eq!((a.string, a.pitch), (5, 45.0));
    }

    #[test]
    fn a_strum_repeats_through_each_chord() {
        let mut e = chords();
        if let Phrase::Chords { strum, .. } = &mut e.phrase {
            *strum = Some(Strum {
                pattern: "D-DU x-DU".into(),
                steps_per_beat: 2,
            });
        }
        assert_eq!(e.validate(), Ok(()));
        let tab = e.tab();
        // Per bar: D, D, U, x, D, U on eighths 0, 2, 3, 4, 6, 7. Two bars
        // of E, so twelve strikes of six strings.
        let e_starts: Vec<u64> = tab
            .notes
            .iter()
            .filter(|n| n.start < 7680 && n.string == 6)
            .map(|n| n.start)
            .collect();
        assert_eq!(
            e_starts,
            vec![0, 960, 1440, 1920, 2880, 3360, 3840, 4800, 5280, 5760, 6720, 7200]
        );
        let first = tab.notes.iter().find(|n| n.string == 6).unwrap();
        assert_eq!(first.length, 960, "rings until the next strike");
        let scratch = tab.notes.iter().find(|n| n.start == 1920).unwrap();
        assert!(scratch.dead);
        // An upstroke starts on the highest string.
        let up: Vec<_> = e
            .sounded()
            .into_iter()
            .filter(|n| (1440..1440 + 6 * STRUM_TICKS).contains(&n.start))
            .collect();
        assert_eq!(up[0].string, 1);
        assert_eq!(up[0].start, 1440);
        assert!(up[0].velocity < 1.0);
    }

    #[test]
    fn harmonics_and_the_capo_set_the_pitch() {
        let mut e = notes();
        e.capo = 2;
        let s = e.sounded();
        assert_eq!(s[0].pitch, 42.0);
        assert_eq!(s[1].pitch, 54.0);
        assert!(s[1].harmonic);
    }

    #[test]
    fn rejects_a_note_off_the_fretboard() {
        let mut e = notes();
        e.phrase = Phrase::Notes {
            notes: vec![TabNote::new(1, 25, 0, 480), TabNote::new(7, 0, 0, 480)],
        };
        let p = e.validate().unwrap_err();
        assert_eq!(
            p,
            vec![
                "note 1 (string 1, fret 25): fret 25 is off a 24-fret neck",
                "note 2 is on string 7, but the guitar has strings 1 to 6",
            ]
        );
        e.capo = 5;
        e.phrase = Phrase::Notes {
            notes: vec![TabNote::new(1, 20, 0, 480)],
        };
        assert!(e.validate().unwrap_err()[0].contains("above a capo at 5"));
    }

    #[test]
    fn rejects_a_ladder_that_does_not_climb() {
        let mut e = chords();
        e.tempo = TempoLadder {
            start: 120.0,
            goal: 90.0,
            step: 5.0,
        };
        assert_eq!(
            e.validate().unwrap_err(),
            vec!["the goal tempo 90 is below the start tempo 120"]
        );
        e.tempo = TempoLadder {
            start: 90.0,
            goal: 100.0,
            step: 0.0,
        };
        assert!(e.validate().unwrap_err()[0].contains("not above 0"));
        e.tempo.step = 20.0;
        assert!(e.validate().unwrap_err()[0].contains("more than the 10 bpm"));
        e.tempo = TempoLadder {
            start: 90.0,
            goal: 90.0,
            step: 5.0,
        };
        assert_eq!(
            e.validate(),
            Ok(()),
            "a fixed tempo is a ladder of one rung"
        );
        e.tempo.goal = 400.0;
        assert!(e.validate().unwrap_err()[0].contains("outside 30 to 300"));
    }

    #[test]
    fn rejects_a_chord_with_the_wrong_number_of_strings() {
        let mut e = chords();
        if let Phrase::Chords { chords, .. } = &mut e.phrase {
            chords[1].frets.pop();
            chords[0].fingers = vec![0, 1];
        }
        assert_eq!(
            e.validate().unwrap_err(),
            vec![
                "chord 1 (E) has 2 fingers for 6 strings",
                "chord 2 (A) has 5 strings; the guitar has 6",
            ]
        );
    }

    #[test]
    fn reports_every_problem() {
        let mut e = chords();
        e.version = 2;
        e.name = String::new();
        e.instructions = " ".into();
        e.count_in = 9;
        e.key.tonic = "H".into();
        e.tuning = Some(vec![40, 45, 50]);
        e.bars = Some(1);
        if let Phrase::Chords { strum, .. } = &mut e.phrase {
            *strum = Some(Strum {
                pattern: "DZ".into(),
                steps_per_beat: 5,
            });
        }
        e.drums.as_mut().unwrap().level_db = 40.0;
        let p = e.validate().unwrap_err();
        for want in [
            "version 2",
            "the name is empty",
            "the instructions are empty",
            "count-in of 9",
            "tonic \"H\"",
            "the tuning has 3 strings",
            "5 steps a beat",
            "'Z'",
            "past the loop's 1 bars",
            "level 40 dB",
        ] {
            assert!(p.iter().any(|x| x.contains(want)), "{want:?} not in {p:#?}");
        }
    }
}
