//! Jam mode: drums (and later a backing) to play along with, your rig
//! on the input, heard in place of the project without touching it.
//!
//! While a jam is open the engine hears a project of its own making (the
//! input through the jam's rig, and a drum track) instead of the open
//! one. Its timeline only ever moves forward: there is no loop to wrap.
//! What plays where is a [`Schedule`], a list of section changes and
//! tempo changes on bar lines. Asking for a section or a tempo puts a
//! change on the next bar line far enough ahead that the new score and
//! tempo map reach the audio thread before it, and since they differ from
//! the old ones only from that bar on, the change lands on the bar to the
//! sample however late in the block the swap came.

use std::collections::HashMap;
use std::sync::Arc;

use cantina_dsp::pluck::Instrument as Voice;
use cantina_dsp::PluckNote;
use cantina_engine::sequencer::{PatternData, Placement, Score};
use cantina_engine::{TabEvent, TabLoop, TabScore, TempoMap, TimeSignature};
use cantina_project::{Bar, Chord, Instrument, Meter, Rhythm, Section, TrackId, PPQ};
use serde::Serialize;

/// The first id the jam's tracks and pedals take: far above any a
/// project gives out, and apart from an auditioned rig's.
pub const JAM_IDS: u64 = 1 << 61;
/// The track the input plays on, through the jam's rig.
pub const INPUT_TRACK: TrackId = TrackId(JAM_IDS);
/// The jam's drums.
pub const DRUM_TRACK: TrackId = TrackId(JAM_IDS + 1);
/// The backing: a progression's voice, or audio.
pub const BACKING_TRACK: TrackId = TrackId(JAM_IDS + 2);
/// The first id the rig's pedals take.
pub const FIRST_PEDAL: u64 = JAM_IDS + 16;
/// The first id the backing's pedals take, apart from the input rig's.
pub const FIRST_BACKING_PEDAL: u64 = JAM_IDS + (1 << 59);

/// The sections in the order the view lists them and keys 1 to 6 reach.
pub const SECTIONS: [Section; 6] = [
    Section::Intro,
    Section::Verse,
    Section::Chorus,
    Section::Bridge,
    Section::Fill,
    Section::Ending,
];

/// How a section plays once it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plays {
    /// Round and round until another is asked for.
    Loop,
    /// Once, then back to the section it interrupted.
    Once,
    /// Once, and then the jam stops.
    End,
}

/// How `section` plays.
pub fn plays(section: Section) -> Plays {
    match section {
        Section::Intro | Section::Fill => Plays::Once,
        Section::Ending => Plays::End,
        Section::Verse | Section::Chorus | Section::Bridge => Plays::Loop,
    }
}

/// What plays where: section changes and tempo changes, each on a bar
/// line, in ticks from the start of the jam.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Ticks in a bar.
    bar: u64,
    /// In order of tick; the first at 0.
    sections: Vec<(u64, Section)>,
    /// In order of tick; the first at 0.
    tempos: Vec<(u64, f64)>,
    /// Where an ending ends, when one is coming.
    end: Option<u64>,
}

/// A point where the section or the tempo changes, for the view to show
/// where the jam is and what is coming. Mirrors `JamMark` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamMark {
    /// Timeline sample it falls on.
    pub sample: u64,
    /// Tick it falls on.
    pub tick: u64,
    /// The section playing from here.
    pub section: Section,
    /// The tempo from here.
    pub bpm: f64,
}

impl Schedule {
    /// A jam that starts with `first` at `bpm`; `length` is how long each
    /// section's pattern is, in ticks, and `home` the section a once-only
    /// one returns to when nothing looped before it.
    pub fn new(
        bar: u64,
        first: Section,
        bpm: f64,
        length: impl Fn(Section) -> u64,
        home: Section,
    ) -> Self {
        let mut s = Self {
            bar: bar.max(1),
            sections: Vec::new(),
            tempos: vec![(0, bpm)],
            end: None,
        };
        s.switch(0, first, length(first), home);
        s
    }

    /// Plays `section` from the bar line `at`, dropping whatever was
    /// coming from there on. A once-only section returns, after its
    /// `length`, to the looping section it interrupted, or `home`; an
    /// ending stops the jam after its length.
    pub fn switch(&mut self, at: u64, section: Section, length: u64, home: Section) {
        let back = self
            .sections
            .iter()
            .rev()
            .filter(|(t, _)| *t < at)
            .map(|(_, s)| *s)
            .find(|s| plays(*s) == Plays::Loop)
            .unwrap_or(home);
        self.sections.retain(|(t, _)| *t < at);
        self.sections.push((at, section));
        self.end = None;
        let length = length.max(self.bar);
        match plays(section) {
            Plays::Loop => {}
            Plays::Once => self.sections.push((at + length, back)),
            Plays::End => self.end = Some(at + length),
        }
    }

    /// Plays at `bpm` from the bar line `at`, dropping tempo changes
    /// that were coming after it.
    pub fn set_tempo(&mut self, at: u64, bpm: f64) {
        self.tempos.retain(|(t, _)| *t < at);
        if self.tempos.last().is_some_and(|(_, b)| *b == bpm) {
            return;
        }
        self.tempos.push((at, bpm));
    }

    /// The section playing at `tick`.
    pub fn section_at(&self, tick: u64) -> Section {
        self.sections
            .iter()
            .rev()
            .find(|(t, _)| *t <= tick)
            .or(self.sections.first())
            .map_or(Section::Verse, |(_, s)| *s)
    }

    /// The tempo at `tick`.
    pub fn tempo_at(&self, tick: u64) -> f64 {
        self.tempos
            .iter()
            .rev()
            .find(|(t, _)| *t <= tick)
            .or(self.tempos.first())
            .map_or(120.0, |(_, b)| *b)
    }

    /// The last tempo asked for.
    pub fn tempo(&self) -> f64 {
        self.tempos.last().map_or(120.0, |(_, b)| *b)
    }

    /// Where an ending ends, when one is coming.
    pub fn end(&self) -> Option<u64> {
        self.end
    }

    /// Where the coming ending starts.
    pub fn ending_start(&self) -> Option<u64> {
        self.end?;
        self.sections
            .iter()
            .rev()
            .find(|(_, s)| *s == Section::Ending)
            .map(|(t, _)| *t)
    }

    /// Each section with where it starts and ends, in ticks; the last
    /// ends at the end, or never (`u64::MAX`).
    pub fn segments(&self) -> Vec<(u64, u64, Section)> {
        self.sections
            .iter()
            .enumerate()
            .map(|(i, (start, section))| {
                let end = self
                    .sections
                    .get(i + 1)
                    .map(|(t, _)| *t)
                    .or(self.end)
                    .unwrap_or(u64::MAX);
                (*start, end, *section)
            })
            .collect()
    }

    /// The tempo map the transport plays: one meter, the tempo changes.
    pub fn tempo_map(&self, meter: Meter) -> TempoMap {
        let mut map = TempoMap::new(
            self.tempos.first().map_or(120.0, |(_, b)| *b),
            TimeSignature::new(meter.numerator, meter.denominator),
        );
        for (tick, bpm) in self.tempos.iter().skip(1) {
            map.set_tempo(*tick, *bpm);
        }
        map
    }

    /// What the drums play: each section's pattern from its change to
    /// the next, from the start of the pattern; the last for ever, or to
    /// the end. A section with no pattern plays nothing.
    pub fn placements(&self, patterns: &HashMap<Section, Arc<PatternData>>) -> Score {
        let mut score = Score::with_capacity(self.sections.len());
        for (i, (start, section)) in self.sections.iter().enumerate() {
            let until = self
                .sections
                .get(i + 1)
                .map(|(t, _)| *t)
                .or(self.end)
                .unwrap_or(u64::MAX);
            let Some(pattern) = patterns.get(section) else {
                continue;
            };
            score.push(Placement {
                pattern: pattern.clone(),
                start: *start,
                offset: 0,
                length: until.saturating_sub(*start),
            });
        }
        score
    }

    /// Every point where the section or the tempo changes, and the end,
    /// placed on the timeline by `map`.
    pub fn marks(&self, map: &TempoMap, sample_rate: f64) -> Vec<JamMark> {
        let mut ticks: Vec<u64> = self
            .sections
            .iter()
            .map(|(t, _)| *t)
            .chain(self.tempos.iter().map(|(t, _)| *t))
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        ticks
            .into_iter()
            .filter(|t| self.end.is_none_or(|end| *t < end))
            .map(|tick| JamMark {
                sample: map.ticks_to_samples(tick as f64, sample_rate),
                tick,
                section: self.section_at(tick),
                bpm: self.tempo_at(tick),
            })
            .collect()
    }

    /// The first bar line at least `margin` samples after `position`,
    /// in ticks: far enough ahead that a change sent now reaches the
    /// audio thread first.
    pub fn next_bar(&self, map: &TempoMap, position: u64, sample_rate: f64, margin: u64) -> u64 {
        let tick = map.samples_to_ticks(position + margin, sample_rate);
        (tick / self.bar as f64).ceil() as u64 * self.bar
    }
}

/// The section a jam goes home to: the verse, or the first section that
/// loops, or the first it has at all.
pub fn home(available: &[Section]) -> Section {
    if available.contains(&Section::Verse) {
        return Section::Verse;
    }
    available
        .iter()
        .copied()
        .find(|s| plays(*s) == Plays::Loop)
        .or(available.first().copied())
        .unwrap_or(Section::Verse)
}

/// The section a jam starts with: the intro when it has one, then home.
pub fn first(available: &[Section]) -> Section {
    if available.contains(&Section::Intro) {
        Section::Intro
    } else {
        home(available)
    }
}

/// Ticks a strummed chord takes from its lowest string to its highest.
const STRUM_TICKS: f64 = 6.0;

/// The pitches a guitar plays for `chord`, lowest first, as a barre
/// chord would: the bass from E2 up, then the fifth, the octave (or the
/// seventh or sixth), the third, the fifth and the root (or the ninth)
/// above. A power chord is three strings.
pub fn guitar_voicing(chord: &Chord) -> Vec<f32> {
    let has = |i: u8| chord.intervals.contains(&i);
    let bass = 40 + (i32::from(chord.bass) - 4).rem_euclid(12);
    let mut root = 40 + (i32::from(chord.root) - 4).rem_euclid(12);
    if chord.bass != chord.root && root <= bass {
        root += 12;
    }
    let fifth: i32 = [7, 6, 8].into_iter().find(|i| has(*i)).map_or(7, i32::from);
    let third: Option<i32> = [4, 3, 5, 2].into_iter().find(|i| has(*i)).map(i32::from);
    let mut notes = vec![bass];
    if chord.bass != chord.root {
        notes.push(root);
    }
    let Some(third) = third else {
        // A power chord: root, fifth, octave.
        notes.extend([root + fifth, root + 12]);
        notes.dedup();
        return notes.into_iter().map(|n| n as f32).collect();
    };
    let middle: i32 = [10, 11, 9]
        .into_iter()
        .find(|i| has(*i))
        .map_or(12, i32::from);
    let top = if has(14) { 26 } else { 24 };
    for n in [
        root + fifth,
        root + middle,
        root + 12 + third,
        root + 12 + fifth,
        root + top,
    ] {
        if n > *notes.last().expect("the bass is there") {
            notes.push(n);
        }
    }
    notes.truncate(6);
    notes.into_iter().map(|n| n as f32).collect()
}

/// The note a bass plays for `chord`: its bass note from E1 up.
pub fn bass_note(chord: &Chord) -> f32 {
    (28 + (i32::from(chord.bass) - 4).rem_euclid(12)) as f32
}

/// The bass string a pitch is played on: E, A or D.
fn bass_string(pitch: f32) -> u8 {
    if pitch < 33.0 {
        4
    } else if pitch < 38.0 {
        3
    } else {
        2
    }
}

/// Ticks between strikes for a rhythm, and whether it is a strike at all
/// (a whole note is struck once per chord).
fn strike_ticks(rhythm: Rhythm, beat: f64) -> Option<f64> {
    match rhythm {
        Rhythm::Whole => None,
        Rhythm::Half => Some(2.0 * beat),
        Rhythm::Quarter => Some(beat),
        Rhythm::Eighth => Some(beat / 2.0),
    }
}

/// One chord's notes struck at `tick` for `ticks`: a strum across the
/// guitar's strings (up on an off-beat eighth), or the bass's note.
fn strike(
    out: &mut TabScore,
    chord: &Chord,
    instrument: Instrument,
    tick: f64,
    ticks: f64,
    velocity: f32,
    up: bool,
) {
    match instrument {
        Instrument::Guitar => {
            let notes = guitar_voicing(chord);
            let strings = notes.len();
            for (i, pitch) in notes.into_iter().enumerate() {
                // Strings count from the highest: the lowest note is on
                // string 6, or the 5th or 4th for fewer notes.
                let string = (6 - i) as u8;
                let order = if up { strings - 1 - i } else { i };
                let delay = order as f64 * STRUM_TICKS;
                out.push(TabEvent {
                    tick: tick + delay,
                    ticks: (ticks - delay).max(1.0),
                    instrument: Voice::Guitar,
                    note: PluckNote::new(string, pitch, velocity * if up { 0.8 } else { 1.0 }, 0),
                });
            }
        }
        Instrument::Bass => {
            let pitch = bass_note(chord);
            out.push(TabEvent {
                tick,
                ticks: (ticks * 0.9).max(1.0),
                instrument: Voice::Bass,
                note: PluckNote::new(bass_string(pitch), pitch, velocity, 0),
            });
        }
    }
}

/// Each chord of a progression with where it starts and how long it
/// lasts, in ticks from the start; a held chord lasts on. The total is
/// the progression's length.
pub fn chord_spans(bars: &[Bar], bar_ticks: u64) -> (Vec<(f64, f64, Chord)>, u64) {
    let mut spans: Vec<(f64, f64, Chord)> = Vec::new();
    for (b, bar) in bars.iter().enumerate() {
        let share = bar_ticks as f64 / bar.len().max(1) as f64;
        for (i, chord) in bar.iter().enumerate() {
            let at = b as f64 * bar_ticks as f64 + i as f64 * share;
            match chord {
                Some(c) => spans.push((at, share, c.clone())),
                None => {
                    if let Some(last) = spans.last_mut() {
                        last.1 += share;
                    }
                }
            }
        }
    }
    (spans, bars.len() as u64 * bar_ticks)
}

/// A progression as the voice plays it: one pass of its chords, struck
/// in the rhythm, repeated from the start of the jam until `until`.
pub fn progression_loop(
    bars: &[Bar],
    instrument: Instrument,
    rhythm: Rhythm,
    meter: Meter,
    until: u64,
) -> TabLoop {
    let beat = f64::from(PPQ) * 4.0 / f64::from(meter.denominator.max(1));
    let bar_ticks = (beat * f64::from(meter.numerator.max(1))) as u64;
    let (spans, period) = chord_spans(bars, bar_ticks);
    let mut notes = TabScore::new();
    for (start, length, chord) in &spans {
        let end = start + length;
        let step = strike_ticks(rhythm, beat).unwrap_or(*length);
        let mut at = *start;
        let mut k = 0;
        while at < end - 0.5 {
            let ticks = step.min(end - at);
            let on_bar = (at % bar_ticks as f64).abs() < 0.5;
            let velocity = if on_bar { 0.85 } else { 0.7 };
            let up = rhythm == Rhythm::Eighth && k % 2 == 1;
            strike(&mut notes, chord, instrument, at, ticks, velocity, up);
            at += step;
            k += 1;
        }
    }
    notes.retain(|n| n.tick < period as f64);
    notes.sort_by(|a, b| a.tick.total_cmp(&b.tick));
    TabLoop {
        notes,
        start: 0,
        period,
        until,
    }
}

/// An exercise's phrase as the voice plays it: every note as it sounds,
/// strums and all, repeated every pass of the loop until `until`.
pub fn phrase_loop(exercise: &cantina_project::Exercise, until: u64) -> TabLoop {
    let period = exercise.loop_ticks();
    let voice = match exercise.instrument {
        Instrument::Guitar => Voice::Guitar,
        Instrument::Bass => Voice::Bass,
    };
    let notes = exercise
        .sounded()
        .into_iter()
        .filter(|s| s.start < period)
        .map(|s| TabEvent {
            tick: s.start as f64,
            ticks: s.length as f64,
            instrument: voice,
            note: PluckNote {
                string: s.string,
                pitch: s.pitch,
                velocity: s.velocity,
                length: 0,
                bend: s.bend,
                release: s.release,
                vibrato: s.vibrato,
                palm_mute: s.palm_mute,
                dead: s.dead,
                harmonic: s.harmonic,
            },
        })
        .collect();
    TabLoop {
        notes,
        start: 0,
        period,
        until,
    }
}

/// What the backing plays when an ending starts at `at`: the
/// progression's first chord, once, ringing for `ticks`.
pub fn ending_chord(bars: &[Bar], instrument: Instrument, at: u64, ticks: u64) -> TabScore {
    let mut out = TabScore::new();
    if let Some(chord) = bars.iter().flatten().flatten().next() {
        strike(
            &mut out,
            chord,
            instrument,
            at as f64,
            ticks as f64,
            0.85,
            false,
        );
    }
    out
}

/// Where a kept take goes in a project, and the drums that go with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The clip's first sample on the project's timeline.
    pub start: u64,
    /// Frames of the take left off its front, played before bar 1.
    pub offset: u64,
    /// The section, where its pattern clip starts in project ticks, how
    /// far into the pattern it starts and how long it lasts.
    pub drums: Vec<(Section, u64, u64, u64)>,
}

/// Lines a take up with a project: `start` is the jam's timeline sample of
/// its first frame (before 0 when it began before the jam rolled), or
/// none when the jam never rolled under it. The bar the take begins in
/// goes on the project's bar `at` (in ticks), at `bpm`; the drums are the
/// sections that played under the take, to whole bars.
#[allow(clippy::too_many_arguments)]
pub fn place_take(
    start: Option<i64>,
    frames: u64,
    map: &TempoMap,
    sample_rate: f64,
    segments: &[(u64, u64, Section)],
    pattern_ticks: impl Fn(Section) -> u64,
    bar: u64,
    at: u64,
    bpm: f64,
) -> Placed {
    let per_tick = sample_rate * 60.0 / (bpm * f64::from(PPQ));
    let project = |ticks: u64| (ticks as f64 * per_tick).round() as i64;
    let Some(start) = start else {
        return Placed {
            start: project(at) as u64,
            offset: 0,
            drums: Vec::new(),
        };
    };
    let jam_ticks = |sample: i64| map.samples_to_ticks(sample.max(0) as u64, sample_rate);
    let first = jam_ticks(start);
    let bar0 = (first / bar as f64).floor() as u64 * bar;
    let end = jam_ticks(start + frames as i64);
    let last = (end / bar as f64).ceil() as u64 * bar;
    let into = start - map.ticks_to_samples(bar0 as f64, sample_rate) as i64;
    let clip = project(at) + into;
    let mut drums = Vec::new();
    for (from, to, section) in segments {
        let (from, to) = ((*from).max(bar0), (*to).min(last));
        if from >= to {
            continue;
        }
        let length = pattern_ticks(*section).max(1);
        let begun = segments
            .iter()
            .find(|(f, t, s)| s == section && *f <= from && from < *t)
            .map_or(from, |(f, _, _)| *f);
        drums.push((
            *section,
            from - bar0 + at,
            (from - begun) % length,
            to - from,
        ));
    }
    Placed {
        start: clip.max(0) as u64,
        offset: (-clip).max(0) as u64,
        drums,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: u64 = 4 * PPQ as u64;
    const SR: f64 = 48_000.0;

    fn bars(n: u64) -> u64 {
        n * BAR
    }

    fn one_bar(_: Section) -> u64 {
        BAR
    }

    fn common() -> Meter {
        Meter {
            numerator: 4,
            denominator: 4,
        }
    }

    #[test]
    fn starts_with_the_intro_and_goes_home_after_it() {
        let all = [Section::Intro, Section::Verse, Section::Fill];
        assert_eq!(first(&all), Section::Intro);
        assert_eq!(first(&[Section::Chorus, Section::Fill]), Section::Chorus);
        assert_eq!(home(&[Section::Fill, Section::Bridge]), Section::Bridge);
        let s = Schedule::new(BAR, first(&all), 100.0, |_| bars(2), home(&all));
        assert_eq!(s.section_at(0), Section::Intro);
        assert_eq!(s.section_at(bars(2) - 1), Section::Intro);
        assert_eq!(s.section_at(bars(2)), Section::Verse);
        assert_eq!(s.section_at(bars(500)), Section::Verse);
    }

    #[test]
    fn a_fill_returns_to_the_section_it_interrupted() {
        let mut s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        s.switch(bars(4), Section::Chorus, bars(2), Section::Verse);
        s.switch(bars(9), Section::Fill, BAR, Section::Verse);
        assert_eq!(s.section_at(bars(9)), Section::Fill);
        assert_eq!(s.section_at(bars(10)), Section::Chorus);
        // Asking again before the bar replaces what was coming.
        s.switch(bars(10), Section::Verse, BAR, Section::Verse);
        assert_eq!(
            s.sections,
            vec![
                (0, Section::Verse),
                (bars(4), Section::Chorus),
                (bars(9), Section::Fill),
                (bars(10), Section::Verse)
            ]
        );
    }

    #[test]
    fn an_ending_ends_and_anything_after_cancels_it() {
        let mut s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        s.switch(bars(3), Section::Ending, bars(2), Section::Verse);
        assert_eq!(s.end(), Some(bars(5)));
        let marks = s.marks(&s.tempo_map(common()), SR);
        assert_eq!(marks.len(), 2, "{marks:?}");
        s.switch(bars(4), Section::Chorus, BAR, Section::Verse);
        assert_eq!(s.end(), None);
    }

    #[test]
    fn placements_play_each_section_to_the_next() {
        let pattern = |n: u32| Arc::new(PatternData::plain(4, 16 * n, 0.0, vec![]));
        let patterns = HashMap::from([
            (Section::Verse, pattern(1)),
            (Section::Fill, pattern(2)),
            (Section::Ending, pattern(3)),
        ]);
        let mut s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        s.switch(bars(2), Section::Fill, BAR, Section::Verse);
        let score = s.placements(&patterns);
        let spans: Vec<(u64, u64)> = score.iter().map(|p| (p.start, p.length)).collect();
        assert_eq!(
            spans,
            vec![(0, bars(2)), (bars(2), BAR), (bars(3), u64::MAX - bars(3))]
        );
        assert!(Arc::ptr_eq(&score[1].pattern, &patterns[&Section::Fill]));
        s.switch(bars(5), Section::Ending, bars(1), Section::Verse);
        let last = s.placements(&patterns).pop().unwrap();
        assert_eq!((last.start, last.length), (bars(5), BAR));
        // A section without a pattern is silence.
        s.switch(bars(6), Section::Chorus, BAR, Section::Verse);
        assert_eq!(s.placements(&patterns).len(), 4);
    }

    #[test]
    fn tempo_changes_go_on_their_bar_and_into_the_map() {
        let mut s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        s.set_tempo(bars(2), 150.0);
        s.set_tempo(bars(4), 90.0);
        // A nudge before the coming one replaces it.
        s.set_tempo(bars(3), 140.0);
        assert_eq!(
            s.tempos,
            vec![(0, 120.0), (bars(2), 150.0), (bars(3), 140.0)]
        );
        assert_eq!(s.tempo(), 140.0);
        // The same tempo again adds nothing.
        s.set_tempo(bars(5), 140.0);
        assert_eq!(s.tempos.len(), 3);
        let map = s.tempo_map(common());
        assert_eq!(map.tempo_at(bars(2) - 1), 120.0);
        assert_eq!(map.tempo_at(bars(2)), 150.0);
        let marks = s.marks(&map, SR);
        // Two bars at 120 is four seconds.
        assert_eq!(marks[1].sample, 4 * 48_000);
        assert_eq!((marks[1].section, marks[1].bpm), (Section::Verse, 150.0));
    }

    #[test]
    fn the_next_bar_leaves_room_for_the_swap() {
        let s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        let map = s.tempo_map(common());
        // A bar at 120 is 96 000 samples.
        assert_eq!(s.next_bar(&map, 0, SR, 0), 0);
        assert_eq!(s.next_bar(&map, 1, SR, 0), BAR);
        assert_eq!(s.next_bar(&map, 50_000, SR, 1_000), BAR);
        // Too close to the line: the one after.
        assert_eq!(s.next_bar(&map, 95_500, SR, 1_000), bars(2));
        assert_eq!(s.next_bar(&map, 96_000, SR, 0), BAR);
    }

    fn chord(token: &str) -> Chord {
        cantina_project::parse_chord(token, 9).unwrap()
    }

    #[test]
    fn guitar_chords_are_barre_shapes() {
        // E major: E2 B2 E3 G#3 B3 E4, the open E shape.
        assert_eq!(
            guitar_voicing(&chord("E")),
            vec![40.0, 47.0, 52.0, 56.0, 59.0, 64.0]
        );
        // A minor 7 from E2's string: A2 E3 G3 C4 E4 A4.
        assert_eq!(
            guitar_voicing(&chord("Am7")),
            vec![45.0, 52.0, 55.0, 60.0, 64.0, 69.0]
        );
        // A power chord is three strings.
        assert_eq!(guitar_voicing(&chord("F#5")), vec![42.0, 49.0, 54.0]);
        // C over E: E in the bass, the C shape above it.
        let slash = guitar_voicing(&chord("C/E"));
        assert_eq!(slash[0], 40.0);
        assert_eq!(slash[1], 48.0);
        assert!(slash.windows(2).all(|w| w[0] < w[1]), "{slash:?}");
        assert_eq!(bass_note(&chord("A")), 33.0);
        assert_eq!(bass_note(&chord("D#")), 39.0);
    }

    #[test]
    fn a_progression_strikes_in_its_rhythm_and_loops() {
        let prog = cantina_project::parse_progression("I | IV | - V", 9).unwrap();
        let common = common();
        let l = progression_loop(&prog, Instrument::Bass, Rhythm::Quarter, common, u64::MAX);
        assert_eq!(l.period, bars(3));
        // A note on every beat of three bars.
        assert_eq!(l.notes.len(), 12);
        let beat = f64::from(PPQ);
        assert_eq!(l.notes[4].tick, 4.0 * beat);
        // Bar 3 holds IV for two beats, then V.
        assert_eq!(l.notes[9].note.pitch, bass_note(&chord("IV")));
        assert_eq!(l.notes[10].note.pitch, bass_note(&chord("V")));
        // Half notes on a guitar: a six-string strum on beats 1 and 3.
        let l = progression_loop(&prog[..1], Instrument::Guitar, Rhythm::Half, common, 7);
        assert_eq!(l.notes.len(), 12);
        assert_eq!(
            (l.notes[1].tick, l.notes[6].tick),
            (STRUM_TICKS, 2.0 * beat)
        );
        assert_eq!(l.until, 7);
        // A whole-note chord rings for as long as it lasts, holds and all.
        let l = progression_loop(&prog, Instrument::Bass, Rhythm::Whole, common, 0);
        assert_eq!(l.notes.len(), 3);
        assert_eq!(l.notes[1].ticks, 6.0 * beat * 0.9);
    }

    #[test]
    fn an_ending_rings_the_first_chord() {
        let prog = cantina_project::parse_progression("vi | IV", 9).unwrap();
        let end = ending_chord(&prog, Instrument::Guitar, bars(8), BAR);
        assert_eq!(end.len(), 6);
        assert_eq!(end[0].tick, bars(8) as f64);
        assert_eq!(end[0].note.pitch, guitar_voicing(&chord("vi"))[0]);
    }

    /// Renders a progression through the voice, as the jam's backing
    /// track plays it, at 120 bpm: what comes out, and the notes plucked.
    fn render(chords: &str, instrument: Instrument, rhythm: Rhythm, bars: u64) -> (Vec<f32>, u64) {
        use cantina_engine::{Endpoint, GraphConfig, Runtime, TabVoice};
        let prog = cantina_project::parse_progression(chords, 4).unwrap();
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: 256,
            channels: 1,
        })
        .unwrap();
        let (voice, mut handle) = TabVoice::new();
        let id = controller.graph_mut().add(Box::new(voice));
        controller
            .graph_mut()
            .connect(id, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();
        assert!(handle.set_loop(progression_loop(
            &prog,
            instrument,
            rhythm,
            common(),
            u64::MAX
        )));
        controller.transport_mut().play();
        let frames = (bars * 96_000) as usize;
        let out = runtime.render(&[vec![0.0; frames]], 256).remove(0);
        let notes = handle
            .stats()
            .notes
            .load(std::sync::atomic::Ordering::Relaxed);
        (out, notes)
    }

    fn peak_rms(out: &[f32]) -> (f32, f32) {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        (peak, rms)
    }

    #[test]
    fn a_backing_renders_in_time_and_in_range() {
        // E minor: i | bVI | bVII | i, half-note strums, four bars at 120.
        let (out, notes) = render("i | bVI | bVII | i", Instrument::Guitar, Rhythm::Half, 4);
        assert_eq!(notes, 4 * 2 * 6);
        let (peak, rms) = peak_rms(&out);
        eprintln!(
            "guitar: peak {:.1} dBFS, rms {:.1} dBFS",
            20.0 * peak.log10(),
            20.0 * rms.log10()
        );
        assert!(peak < 1.0 && rms > 0.01, "peak {peak} rms {rms}");
        // Every strum starts on its beat: the 10 ms after it are louder
        // than the 10 ms before, where the last strum is dying away.
        for strum in 1..8 {
            let at = strum * 48_000;
            let (_, before) = peak_rms(&out[at - 480..at]);
            let (_, after) = peak_rms(&out[at..at + 480]);
            assert!(after > 2.0 * before, "strum {strum}: {before} then {after}");
        }
        let (out, notes) = render("i | bVI | bVII | i", Instrument::Bass, Rhythm::Quarter, 4);
        assert_eq!(notes, 16);
        let (peak, rms) = peak_rms(&out);
        eprintln!(
            "bass: peak {:.1} dBFS, rms {:.1} dBFS",
            20.0 * peak.log10(),
            20.0 * rms.log10()
        );
        assert!(peak < 1.0 && rms > 0.01, "peak {peak} rms {rms}");
    }

    #[test]
    fn a_take_lines_up_with_its_bars() {
        let map = TempoMap::new(120.0, TimeSignature::COMMON);
        // Verse from 0, a 1-bar fill at bar 5, verse again; a 2-bar verse.
        let segments = vec![
            (0, bars(4), Section::Verse),
            (bars(4), bars(5), Section::Fill),
            (bars(5), u64::MAX, Section::Verse),
        ];
        let ticks = |s: Section| if s == Section::Verse { bars(2) } else { BAR };
        // A take from half way through bar 4 (beat 3) to bar 7, put on
        // the project's bar 9 at the same tempo.
        let start = 3 * 96_000 + 48_000;
        let p = place_take(
            Some(start),
            3 * 96_000,
            &map,
            SR,
            &segments,
            ticks,
            BAR,
            bars(8),
            120.0,
        );
        // Bar 4 lands on bar 9: the take starts half a bar into it.
        assert_eq!((p.start, p.offset), (8 * 96_000 + 48_000, 0));
        assert_eq!(
            p.drums,
            vec![
                // Bar 4 is the second bar of a two-bar verse that began at bar 3.
                (Section::Verse, bars(8), BAR, BAR),
                (Section::Fill, bars(9), 0, BAR),
                (Section::Verse, bars(10), 0, bars(2)),
            ]
        );
        // Begun before the jam rolled: the part before bar 1 is cut off a
        // new project's start.
        let p = place_take(
            Some(-12_000),
            96_000,
            &map,
            SR,
            &segments,
            ticks,
            BAR,
            0,
            120.0,
        );
        assert_eq!((p.start, p.offset), (0, 12_000));
        assert_eq!(p.drums, vec![(Section::Verse, 0, 0, BAR)]);
        // Never rolled: just the audio, at the bar asked for.
        let p = place_take(
            None,
            96_000,
            &map,
            SR,
            &segments,
            ticks,
            BAR,
            bars(2),
            120.0,
        );
        assert_eq!((p.start, p.drums.len()), (2 * 96_000, 0));
    }

    /// The stage takes its beat from the tempo map the jam hands the
    /// transport. A tempo change goes in ahead of its bar and changes
    /// nothing before it, so the beat the scenes pulse on keeps time
    /// through the change: on every beat, to the sample, at each tempo.
    #[test]
    fn an_exercise_phrase_loops_every_pass_as_it_sounds() {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../exercises/e-to-a.exercise.json"),
        )
        .unwrap();
        let e = cantina_project::exercise_from_json(&text).unwrap();
        let l = phrase_loop(&e, 99);
        assert_eq!(l.period, 4 * 3840);
        assert_eq!(l.until, 99);
        assert_eq!(l.notes.len(), e.sounded().len());
        // Strummed low to high: the low E first, the high E 50 ticks on.
        assert_eq!((l.notes[0].note.string, l.notes[0].tick), (6, 0.0));
        assert!(l.notes.windows(2).all(|w| w[0].tick <= w[1].tick));
        assert!(l.notes.iter().all(|n| n.instrument == Voice::Guitar));
        assert_eq!(l.notes[0].note.pitch, 40.0);
    }

    #[test]
    fn the_stage_beat_stays_locked_across_a_tempo_change() {
        use cantina_engine::beat_at;
        let mut s = Schedule::new(BAR, Section::Verse, 120.0, one_bar, Section::Verse);
        let before = s.tempo_map(common());
        s.set_tempo(bars(2), 150.0);
        let map = s.tempo_map(common());
        // Until the change the beat is the one the old map gave.
        for sample in (0..192_000).step_by(997) {
            assert_eq!(beat_at(&map, sample, SR), beat_at(&before, sample, SR));
        }
        // Each beat starts where the beat count moves on: 24 000 samples
        // apart at 120, 19 200 at 150, and the change on bar 3's first.
        let mut starts = Vec::new();
        let mut last = beat_at(&map, 0, SR);
        for sample in 1..400_000u64 {
            let b = beat_at(&map, sample, SR);
            if (b.bar, b.beat) != (last.bar, last.beat) {
                starts.push(sample);
                assert!(
                    b.phase < 0.001,
                    "beat {}.{} starts at phase {}",
                    b.bar,
                    b.beat,
                    b.phase
                );
            }
            last = b;
        }
        let gaps: Vec<u64> = starts.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(starts[7], 192_000, "bar 3 starts after two bars at 120");
        assert!(gaps[..7].iter().all(|g| *g == 24_000), "{gaps:?}");
        // Floating point rounds a beat start to one sample either way.
        assert!(
            gaps[7..].iter().all(|g| g.abs_diff(19_200) <= 1),
            "{gaps:?}"
        );
        assert_eq!(beat_at(&map, 192_000, SR).bpm, 150.0);
    }
}
