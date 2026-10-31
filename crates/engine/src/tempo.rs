//! The tempo map: sample-accurate conversion between samples, seconds and
//! musical positions.
//!
//! Musical time is counted in ticks, [`PPQ`] per quarter note, so every
//! beat and bar boundary is an integer. Tempo is piecewise constant with
//! changes at any tick; the time signature changes only on bar boundaries.
//! Seconds are the bridge between ticks and samples, so the map is
//! independent of sample rate and every method that touches samples takes
//! the rate as an argument.
//!
//! The map is immutable once it reaches the audio thread, and every lookup
//! there is a binary search over a preallocated table: nothing here
//! allocates after construction except the `set_*` editing methods, which
//! run on the UI thread.

use std::fmt;

/// Ticks per quarter note.
pub const PPQ: u32 = 960;

/// A time signature such as 4/4 or 6/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl TimeSignature {
    pub const COMMON: TimeSignature = TimeSignature::new(4, 4);

    /// The denominator must be a power of two from 1 to 64.
    pub const fn new(numerator: u8, denominator: u8) -> Self {
        assert!(numerator > 0, "time signature numerator must be positive");
        assert!(
            denominator.is_power_of_two() && denominator <= 64,
            "time signature denominator must be a power of two up to 64"
        );
        Self {
            numerator,
            denominator,
        }
    }

    /// Ticks in one beat, where a beat is one `1/denominator` note.
    pub const fn ticks_per_beat(&self) -> u64 {
        (PPQ as u64 * 4) / self.denominator as u64
    }

    pub const fn ticks_per_bar(&self) -> u64 {
        self.ticks_per_beat() * self.numerator as u64
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// A tempo from `tick` onward, in quarter notes per minute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoChange {
    pub tick: u64,
    pub bpm: f64,
}

/// A time signature from `bar` onward. Bars are zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterChange {
    pub bar: u32,
    pub signature: TimeSignature,
}

/// A position in bars, beats and ticks, all zero-based. Displays
/// one-based as `bar.beat.tick`, the way a transport readout does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MusicalPosition {
    pub bar: u32,
    pub beat: u32,
    pub tick: u32,
}

impl MusicalPosition {
    pub const fn new(bar: u32, beat: u32, tick: u32) -> Self {
        Self { bar, beat, tick }
    }
}

impl fmt::Display for MusicalPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{:03}", self.bar + 1, self.beat + 1, self.tick)
    }
}

/// A beat found by [`TempoMap::beats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// Timeline sample the beat lands on.
    pub sample: u64,
    pub position: MusicalPosition,
    /// True on the first beat of a bar.
    pub is_downbeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TempoSegment {
    tick: u64,
    /// Seconds at `tick`, accumulated from the start.
    seconds: f64,
    bpm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MeterSegment {
    bar: u32,
    /// Tick at the start of `bar`, accumulated from the start.
    tick: u64,
    signature: TimeSignature,
}

/// Tempo and time signature over the whole timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    tempos: Vec<TempoSegment>,
    meters: Vec<MeterSegment>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::new(120.0, TimeSignature::COMMON)
    }
}

impl TempoMap {
    /// A map with one tempo and one time signature from the start.
    pub fn new(bpm: f64, signature: TimeSignature) -> Self {
        let mut map = Self {
            tempos: vec![TempoSegment {
                tick: 0,
                seconds: 0.0,
                bpm: bpm.max(1.0),
            }],
            meters: vec![MeterSegment {
                bar: 0,
                tick: 0,
                signature,
            }],
        };
        map.rebuild();
        map
    }

    /// Sets the tempo from `tick` onward, replacing a change already there.
    pub fn set_tempo(&mut self, tick: u64, bpm: f64) {
        let bpm = bpm.max(1.0);
        match self.tempos.binary_search_by_key(&tick, |t| t.tick) {
            Ok(i) => self.tempos[i].bpm = bpm,
            Err(i) => self.tempos.insert(
                i,
                TempoSegment {
                    tick,
                    seconds: 0.0,
                    bpm,
                },
            ),
        }
        self.rebuild();
    }

    /// Removes the tempo change at `tick`. The change at tick 0 cannot be
    /// removed, only replaced. Returns whether one was there.
    pub fn remove_tempo(&mut self, tick: u64) -> bool {
        if tick == 0 {
            return false;
        }
        match self.tempos.binary_search_by_key(&tick, |t| t.tick) {
            Ok(i) => {
                self.tempos.remove(i);
                self.rebuild();
                true
            }
            Err(_) => false,
        }
    }

    /// Sets the time signature from `bar` onward.
    pub fn set_meter(&mut self, bar: u32, signature: TimeSignature) {
        match self.meters.binary_search_by_key(&bar, |m| m.bar) {
            Ok(i) => self.meters[i].signature = signature,
            Err(i) => self.meters.insert(
                i,
                MeterSegment {
                    bar,
                    tick: 0,
                    signature,
                },
            ),
        }
        self.rebuild();
    }

    /// Removes the meter change at `bar`; bar 0 cannot be removed.
    pub fn remove_meter(&mut self, bar: u32) -> bool {
        if bar == 0 {
            return false;
        }
        match self.meters.binary_search_by_key(&bar, |m| m.bar) {
            Ok(i) => {
                self.meters.remove(i);
                self.rebuild();
                true
            }
            Err(_) => false,
        }
    }

    pub fn tempo_changes(&self) -> impl Iterator<Item = TempoChange> + '_ {
        self.tempos.iter().map(|t| TempoChange {
            tick: t.tick,
            bpm: t.bpm,
        })
    }

    pub fn meter_changes(&self) -> impl Iterator<Item = MeterChange> + '_ {
        self.meters.iter().map(|m| MeterChange {
            bar: m.bar,
            signature: m.signature,
        })
    }

    /// Tempo in effect at `tick`.
    pub fn tempo_at(&self, tick: u64) -> f64 {
        self.tempos[self.tempo_index(tick)].bpm
    }

    /// Time signature in effect at `bar`.
    pub fn signature_at(&self, bar: u32) -> TimeSignature {
        self.meters[self.meter_index(bar)].signature
    }

    // --- ticks and seconds ---

    pub fn ticks_to_seconds(&self, ticks: f64) -> f64 {
        let segment = &self.tempos[self.tempo_index(ticks.max(0.0) as u64)];
        segment.seconds + (ticks - segment.tick as f64) * Self::seconds_per_tick(segment.bpm)
    }

    pub fn seconds_to_ticks(&self, seconds: f64) -> f64 {
        let i = self
            .tempos
            .partition_point(|t| t.seconds <= seconds)
            .saturating_sub(1);
        let segment = &self.tempos[i];
        segment.tick as f64 + (seconds - segment.seconds) / Self::seconds_per_tick(segment.bpm)
    }

    // --- samples ---

    pub fn samples_to_ticks(&self, samples: u64, sample_rate: f64) -> f64 {
        self.seconds_to_ticks(samples as f64 / sample_rate)
    }

    /// The sample a tick lands on, rounded to nearest. Monotonic in `ticks`.
    pub fn ticks_to_samples(&self, ticks: f64, sample_rate: f64) -> u64 {
        (self.ticks_to_seconds(ticks) * sample_rate)
            .round()
            .max(0.0) as u64
    }

    /// The position at a sample, to the nearest tick. A tick is coarser
    /// than a sample at any usable tempo, so a sample a beat was rounded
    /// to reads as that beat.
    pub fn samples_to_position(&self, samples: u64, sample_rate: f64) -> MusicalPosition {
        self.ticks_to_position(self.samples_to_ticks(samples, sample_rate).round() as u64)
    }

    pub fn position_to_samples(&self, position: MusicalPosition, sample_rate: f64) -> u64 {
        self.ticks_to_samples(self.position_to_ticks(position) as f64, sample_rate)
    }

    // --- ticks and musical positions ---

    pub fn ticks_to_position(&self, ticks: u64) -> MusicalPosition {
        let i = self
            .meters
            .partition_point(|m| m.tick <= ticks)
            .saturating_sub(1);
        let segment = &self.meters[i];
        let bar_len = segment.signature.ticks_per_bar();
        let beat_len = segment.signature.ticks_per_beat();
        let into = ticks - segment.tick;
        let bar = segment.bar + (into / bar_len) as u32;
        let in_bar = into % bar_len;
        MusicalPosition {
            bar,
            beat: (in_bar / beat_len) as u32,
            tick: (in_bar % beat_len) as u32,
        }
    }

    /// Ticks at a position. A beat or tick past the end of its bar carries
    /// into the next, so `bar 0 beat 4` in 4/4 is the start of bar 1.
    pub fn position_to_ticks(&self, position: MusicalPosition) -> u64 {
        let segment = &self.meters[self.meter_index(position.bar)];
        let signature = segment.signature;
        segment.tick
            + (position.bar - segment.bar) as u64 * signature.ticks_per_bar()
            + position.beat as u64 * signature.ticks_per_beat()
            + position.tick as u64
    }

    /// The beats whose sample lies in `start..end`, in order. Does not
    /// allocate, so the audio thread may call it.
    pub fn beats(&self, start: u64, end: u64, sample_rate: f64) -> Beats<'_> {
        let mut beats = Beats {
            map: self,
            sample_rate,
            end,
            next: None,
        };
        if start < end {
            let mut candidate = self.beat_at_or_before(start, sample_rate);
            if candidate.sample < start {
                candidate = self.beat_after(candidate.position, sample_rate);
            }
            beats.next = Some(candidate);
        }
        beats
    }

    fn beat_at_or_before(&self, sample: u64, sample_rate: f64) -> Beat {
        let position = self.samples_to_position(sample, sample_rate);
        let position = MusicalPosition {
            tick: 0,
            ..position
        };
        self.beat_at(position, sample_rate)
    }

    fn beat_at(&self, position: MusicalPosition, sample_rate: f64) -> Beat {
        Beat {
            sample: self.position_to_samples(position, sample_rate),
            position,
            is_downbeat: position.beat == 0,
        }
    }

    fn beat_after(&self, position: MusicalPosition, sample_rate: f64) -> Beat {
        let signature = self.signature_at(position.bar);
        let next = if position.beat + 1 >= signature.numerator as u32 {
            MusicalPosition::new(position.bar + 1, 0, 0)
        } else {
            MusicalPosition::new(position.bar, position.beat + 1, 0)
        };
        self.beat_at(next, sample_rate)
    }

    fn tempo_index(&self, tick: u64) -> usize {
        self.tempos
            .partition_point(|t| t.tick <= tick)
            .saturating_sub(1)
    }

    fn meter_index(&self, bar: u32) -> usize {
        self.meters
            .partition_point(|m| m.bar <= bar)
            .saturating_sub(1)
    }

    fn seconds_per_tick(bpm: f64) -> f64 {
        60.0 / (bpm * PPQ as f64)
    }

    /// Recomputes the accumulated seconds and ticks after an edit.
    fn rebuild(&mut self) {
        for i in 1..self.meters.len() {
            let previous = self.meters[i - 1];
            let bars = (self.meters[i].bar - previous.bar) as u64;
            self.meters[i].tick = previous.tick + bars * previous.signature.ticks_per_bar();
        }
        for i in 1..self.tempos.len() {
            let previous = self.tempos[i - 1];
            let ticks = (self.tempos[i].tick - previous.tick) as f64;
            self.tempos[i].seconds =
                previous.seconds + ticks * Self::seconds_per_tick(previous.bpm);
        }
    }
}

/// Iterator over the beats in a sample range. See [`TempoMap::beats`].
pub struct Beats<'a> {
    map: &'a TempoMap,
    sample_rate: f64,
    end: u64,
    next: Option<Beat>,
}

impl Iterator for Beats<'_> {
    type Item = Beat;

    fn next(&mut self) -> Option<Beat> {
        let beat = self.next.take().filter(|b| b.sample < self.end)?;
        self.next = Some(self.map.beat_after(beat.position, self.sample_rate));
        Some(beat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    #[test]
    fn constant_tempo_conversions() {
        let map = TempoMap::default();
        assert_eq!(map.ticks_to_seconds(PPQ as f64), 0.5);
        assert_eq!(map.seconds_to_ticks(1.0), 2.0 * PPQ as f64);
        assert_eq!(map.ticks_to_samples(PPQ as f64, SR), 24_000);
        assert_eq!(map.samples_to_ticks(48_000, SR), 1920.0);
        assert_eq!(map.ticks_to_position(0), MusicalPosition::new(0, 0, 0));
        assert_eq!(
            map.ticks_to_position(PPQ as u64 * 5 + 7),
            MusicalPosition::new(1, 1, 7)
        );
        assert_eq!(
            map.position_to_ticks(MusicalPosition::new(1, 1, 7)),
            PPQ as u64 * 5 + 7
        );
        assert_eq!(MusicalPosition::new(1, 1, 7).to_string(), "2.2.007");
        assert_eq!(TimeSignature::new(6, 8).to_string(), "6/8");
    }

    #[test]
    fn tempo_changes_accumulate() {
        let mut map = TempoMap::new(120.0, TimeSignature::COMMON);
        // Bar 2 starts at tick 7680 = 4 s at 120 bpm; from there 60 bpm.
        map.set_tempo(7680, 60.0);
        assert_eq!(map.ticks_to_seconds(7680.0), 4.0);
        assert_eq!(map.ticks_to_seconds(7680.0 + 960.0), 5.0);
        assert_eq!(map.seconds_to_ticks(5.0), 8640.0);
        assert_eq!(map.seconds_to_ticks(3.0), 5760.0);
        assert_eq!(map.tempo_at(7679), 120.0);
        assert_eq!(map.tempo_at(7680), 60.0);
        for ticks in (0..40_000).step_by(37) {
            let there = map.ticks_to_seconds(ticks as f64);
            let back = map.seconds_to_ticks(there);
            assert!(
                (back - ticks as f64).abs() < 1e-6,
                "{ticks} -> {there} -> {back}"
            );
        }
        assert_eq!(map.tempo_changes().count(), 2);
        map.set_tempo(7680, 240.0);
        assert_eq!(map.tempo_changes().count(), 2, "replaced, not added");
        assert_eq!(map.ticks_to_seconds(7680.0 + 960.0), 4.25);
        assert!(map.remove_tempo(7680));
        assert!(!map.remove_tempo(7680));
        assert!(!map.remove_tempo(0));
        assert_eq!(map.ticks_to_seconds(7680.0 + 960.0), 4.5);
    }

    #[test]
    fn meter_changes_shift_bars() {
        let mut map = TempoMap::default();
        map.set_meter(2, TimeSignature::new(3, 4));
        map.set_meter(4, TimeSignature::new(6, 8));
        let bar2 = 2 * 4 * PPQ as u64;
        assert_eq!(map.ticks_to_position(bar2), MusicalPosition::new(2, 0, 0));
        assert_eq!(
            map.ticks_to_position(bar2 + 3 * PPQ as u64),
            MusicalPosition::new(3, 0, 0)
        );
        let bar4 = bar2 + 2 * 3 * PPQ as u64;
        assert_eq!(
            map.ticks_to_position(bar4 + 480),
            MusicalPosition::new(4, 1, 0)
        );
        assert_eq!(
            map.position_to_ticks(MusicalPosition::new(4, 5, 10)),
            bar4 + 5 * 480 + 10
        );
        assert_eq!(map.signature_at(3), TimeSignature::new(3, 4));
        assert_eq!(map.signature_at(9), TimeSignature::new(6, 8));
        assert!(map.remove_meter(4));
        assert!(!map.remove_meter(0));
        assert_eq!(map.signature_at(9), TimeSignature::new(3, 4));
        assert_eq!(
            map.position_to_ticks(MusicalPosition::new(0, 4, 0)),
            4 * PPQ as u64,
            "beats carry into the next bar"
        );
    }

    #[test]
    fn beats_land_once_each_across_blocks() {
        let mut map = TempoMap::new(120.0, TimeSignature::COMMON);
        map.set_tempo(4 * PPQ as u64, 97.3);
        map.set_meter(2, TimeSignature::new(7, 8));
        let total = 10 * SR as u64;
        let mut found = Vec::new();
        let mut cursor = 0;
        for block in [64u64, 63, 1, 128, 4096, 5].into_iter().cycle() {
            if cursor >= total {
                break;
            }
            let end = (cursor + block).min(total);
            for beat in map.beats(cursor, end, SR) {
                assert!(beat.sample >= cursor && beat.sample < end);
                found.push(beat);
            }
            cursor = end;
        }
        let expected: Vec<Beat> = map.beats(0, total, SR).collect();
        assert_eq!(found, expected);
        for pair in expected.windows(2) {
            assert!(pair[0].sample < pair[1].sample);
            assert!(pair[0].position < pair[1].position);
        }
        assert_eq!(expected[0].sample, 0);
        assert!(expected[0].is_downbeat);
        assert_eq!(expected[1].sample, 24_000);
        assert!(!expected[1].is_downbeat);
        assert_eq!(expected[4].sample, 96_000, "bar 1 at 120 bpm");
        assert!(expected[4].is_downbeat);
        let downbeats: Vec<_> = expected.iter().filter(|b| b.is_downbeat).collect();
        assert_eq!(downbeats[2].position, MusicalPosition::new(2, 0, 0));
        assert_eq!(
            expected.iter().filter(|b| b.position.bar == 2).count(),
            7,
            "7/8 bar has seven beats"
        );
        let bar2 = map.position_to_samples(MusicalPosition::new(2, 0, 0), SR);
        assert_eq!(downbeats[2].sample, bar2);
        let eighth = 60.0 / 97.3 / 2.0 * SR;
        let bar3 = map.position_to_samples(MusicalPosition::new(3, 0, 0), SR);
        assert!(((bar3 - bar2) as f64 - 7.0 * eighth).abs() <= 1.0);
        assert!(map.beats(5, 5, SR).next().is_none());
        assert_eq!(map.beats(1, 24_000, SR).count(), 0);
        assert_eq!(map.beats(1, 24_001, SR).count(), 1);
    }

    #[test]
    fn positions_round_trip_through_samples() {
        let mut map = TempoMap::new(133.0, TimeSignature::new(3, 4));
        map.set_tempo(5000, 88.0);
        for bar in 0..20 {
            for beat in 0..3 {
                let position = MusicalPosition::new(bar, beat, 0);
                let sample = map.position_to_samples(position, SR);
                assert_eq!(map.samples_to_position(sample, SR), position);
                assert_eq!(
                    map.ticks_to_position(map.position_to_ticks(position)),
                    position
                );
            }
        }
    }
}
