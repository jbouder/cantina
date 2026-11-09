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

use cantina_engine::sequencer::{PatternData, Placement, Score};
use cantina_engine::{TempoMap, TimeSignature};
use cantina_project::{Meter, Section, TrackId};
use serde::Serialize;

/// The first id the jam's tracks and pedals take: far above any a
/// project gives out, and apart from an auditioned rig's.
pub const JAM_IDS: u64 = 1 << 61;
/// The track the input plays on, through the jam's rig.
pub const INPUT_TRACK: TrackId = TrackId(JAM_IDS);
/// The jam's drums.
pub const DRUM_TRACK: TrackId = TrackId(JAM_IDS + 1);
/// The first id the rig's pedals take.
pub const FIRST_PEDAL: u64 = JAM_IDS + 16;

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

#[cfg(test)]
mod tests {
    use super::*;
    use cantina_project::PPQ;

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
}
