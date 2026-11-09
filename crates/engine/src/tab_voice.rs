//! The tab voice: plays a track's tabs through plucked strings
//! ([`cantina_dsp::Pluck`]) under the tempo map.
//!
//! A [`TabVoice`] is a graph node, fed like the drum sequencer: its
//! [`TabVoiceHandle`] on the UI thread sets the [`TabScore`], every note
//! of the track's tabs placed on the timeline in ticks, and the node
//! swaps it in at the start of a block. Each block it asks the tempo map
//! which ticks the block covers and plucks the notes that start inside,
//! on their exact frame, for as many frames as their length takes at the
//! current tempo and practice speed. Placing notes in ticks is what lets
//! a practice speed slow the voice with the drums and the click.
//!
//! A jump in the timeline (a locate, a loop wrap) or a stop lets every
//! string go, so nothing rings across the cut.
//!
//! Beside the score the voice can play a [`TabLoop`]: a phrase repeated
//! every so many ticks until a tick, which is how a jam's chord backing
//! goes on for as long as the jam does without a score that grows.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cantina_dsp::pluck::Instrument;
use cantina_dsp::{Buffer, ParamId, ParamInfo, Pluck, PluckNote, Processor};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::{BlockContext, Node};

/// One note of a tab on the timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabEvent {
    /// Timeline tick it starts on.
    pub tick: f64,
    /// Ticks it lasts.
    pub ticks: f64,
    /// Which voice plays it.
    pub instrument: Instrument,
    /// The note; its `length` is worked out when it is played.
    pub note: PluckNote,
}

/// Every note a track's tabs play, in order of tick.
pub type TabScore = Vec<TabEvent>;

/// A phrase played over and over: `notes`, their ticks counted from the
/// start of each pass, every `period` ticks from `start`, and none from
/// `until` on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabLoop {
    /// In order of tick, each within `0..period`.
    pub notes: Vec<TabEvent>,
    /// Timeline tick the first pass starts on.
    pub start: u64,
    /// Ticks in a pass; 0 plays nothing.
    pub period: u64,
    /// No note starts on or after this tick; `u64::MAX` for ever.
    pub until: u64,
}

/// Scores that can wait for the audio thread before `set_score` refuses.
const SCORE_QUEUE: usize = 8;
const RETIRED_QUEUE: usize = SCORE_QUEUE * 2;

/// Counters bumped by the audio thread. Read from anywhere.
#[derive(Debug, Default)]
pub struct TabVoiceStats {
    /// Notes plucked.
    pub notes: AtomicU64,
    /// Notes the voice refused because its queue was full.
    pub dropped: AtomicU64,
    /// Scores swapped in.
    pub swaps: AtomicU64,
    /// Retired scores the UI thread never collected, leaked rather than
    /// freed on the audio thread.
    pub leaked: AtomicU64,
}

/// The graph node.
pub struct TabVoice {
    guitar: Pluck,
    bass: Pluck,
    score: Box<TabScore>,
    incoming: Consumer<Box<TabScore>>,
    retired: Producer<Box<TabScore>>,
    looped: Box<TabLoop>,
    incoming_loop: Consumer<Box<TabLoop>>,
    retired_loop: Producer<Box<TabLoop>>,
    stats: Arc<TabVoiceStats>,
    /// Where the next block starts if the timeline runs on; anything else
    /// is a jump.
    next: Option<u64>,
}

/// The UI-thread half.
pub struct TabVoiceHandle {
    outgoing: Producer<Box<TabScore>>,
    retired: Consumer<Box<TabScore>>,
    score: TabScore,
    outgoing_loop: Producer<Box<TabLoop>>,
    retired_loop: Consumer<Box<TabLoop>>,
    looped: Box<TabLoop>,
    stats: Arc<TabVoiceStats>,
}

impl TabVoice {
    /// A silent tab voice and its handle for the UI thread.
    pub fn new() -> (TabVoice, TabVoiceHandle) {
        let (outgoing, incoming) = RingBuffer::new(SCORE_QUEUE);
        let (retired_in, retired_out) = RingBuffer::new(RETIRED_QUEUE);
        let (outgoing_loop, incoming_loop) = RingBuffer::new(SCORE_QUEUE);
        let (retired_loop_in, retired_loop_out) = RingBuffer::new(RETIRED_QUEUE);
        let stats = Arc::new(TabVoiceStats::default());
        let voice = TabVoice {
            guitar: Pluck::new(Instrument::Guitar),
            bass: Pluck::new(Instrument::Bass),
            score: Box::default(),
            incoming,
            retired: retired_in,
            looped: Box::default(),
            incoming_loop,
            retired_loop: retired_loop_in,
            stats: Arc::clone(&stats),
            next: None,
        };
        let handle = TabVoiceHandle {
            outgoing,
            retired: retired_out,
            score: Vec::new(),
            outgoing_loop,
            retired_loop: retired_loop_out,
            looped: Box::default(),
            stats,
        };
        (voice, handle)
    }

    fn swap_in_latest(&mut self) {
        while let Ok(next) = self.incoming.pop() {
            let old = std::mem::replace(&mut self.score, next);
            if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
                std::mem::forget(old);
                self.stats.leaked.fetch_add(1, Ordering::Relaxed);
            }
            self.stats.swaps.fetch_add(1, Ordering::Relaxed);
        }
        while let Ok(next) = self.incoming_loop.pop() {
            let old = std::mem::replace(&mut self.looped, next);
            if let Err(rtrb::PushError::Full(old)) = self.retired_loop.push(old) {
                std::mem::forget(old);
                self.stats.leaked.fetch_add(1, Ordering::Relaxed);
            }
            self.stats.swaps.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn release_all(&mut self) {
        self.guitar.release_all();
        self.bass.release_all();
    }

    /// Plucks every note that starts in the block's span: the score's,
    /// and the loop's in every pass the span touches.
    fn queue_notes(&mut self, ctx: &BlockContext) {
        let end = ctx.position + ctx.span;
        let t0 = ctx.tempo.samples_to_ticks(ctx.position, ctx.sample_rate);
        let t1 = ctx.tempo.samples_to_ticks(end, ctx.sample_rate);
        let first = self.score.partition_point(|e| e.tick < t0);
        for i in first..self.score.len() {
            let event = self.score[i];
            if event.tick >= t1 {
                break;
            }
            self.pluck(&event, 0.0, ctx);
        }
        let l = &self.looped;
        if l.period == 0 || l.notes.is_empty() {
            return;
        }
        let (start, period) = (l.start as f64, l.period as f64);
        let from = t0.max(start);
        let to = t1.min(l.until as f64);
        if from >= to {
            return;
        }
        let mut pass = ((from - start) / period).floor();
        loop {
            let base = start + pass * period;
            if base >= to {
                break;
            }
            let (lo, hi) = (from - base, to - base);
            let first = self.looped.notes.partition_point(|e| e.tick < lo);
            for i in first..self.looped.notes.len() {
                let event = self.looped.notes[i];
                if event.tick >= hi {
                    break;
                }
                self.pluck(&event, base, ctx);
            }
            pass += 1.0;
        }
    }

    /// Plucks `event`, `offset` ticks later than it says, if it starts in
    /// the block.
    fn pluck(&mut self, event: &TabEvent, offset: f64, ctx: &BlockContext) {
        let (tempo, rate) = (ctx.tempo, ctx.rate.max(f64::MIN_POSITIVE));
        let end = ctx.position + ctx.span;
        let tick = event.tick + offset;
        let sample = tempo.ticks_to_samples(tick, ctx.sample_rate);
        if sample < ctx.position || sample >= end {
            return;
        }
        let off = tempo.ticks_to_samples(tick + event.ticks, ctx.sample_rate);
        let length = ((off.saturating_sub(sample)) as f64 / rate).min(u32::MAX as f64) as u32;
        let note = PluckNote {
            length: length.max(1),
            ..event.note
        };
        let voice = match event.instrument {
            Instrument::Guitar => &mut self.guitar,
            Instrument::Bass => &mut self.bass,
        };
        if voice.play(note, ctx.frame_of(sample)) {
            self.stats.notes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Node for TabVoice {
    fn name(&self) -> &str {
        "Tab voice"
    }

    fn params(&self) -> &[ParamInfo] {
        self.guitar.params()
    }

    fn param(&self, id: ParamId) -> f32 {
        self.guitar.param(id)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        self.guitar.set_param(id, value);
        self.bass.set_param(id, value);
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.guitar.prepare(sample_rate, max_block_frames);
        self.bass.prepare(sample_rate, max_block_frames);
        self.next = None;
    }

    fn reset(&mut self) {
        self.guitar.reset();
        self.bass.reset();
        self.next = None;
    }

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        self.swap_in_latest();
        if ctx.playing && buffer.frames() > 0 {
            if self.next != Some(ctx.position) {
                self.release_all();
            }
            self.queue_notes(ctx);
            self.next = Some(ctx.position + ctx.span);
        } else if self.next.take().is_some() {
            self.release_all();
        }
        self.guitar.process(buffer);
        self.bass.process(buffer);
    }
}

impl TabVoiceHandle {
    /// Counters for the voice, shared with it.
    pub fn stats(&self) -> &Arc<TabVoiceStats> {
        &self.stats
    }

    /// The score as last set from this side.
    pub fn score(&self) -> &TabScore {
        &self.score
    }

    /// Replaces the score, sorting it by tick. The audio thread takes it
    /// at its next block. Returns false when the queue is full, which
    /// means the audio thread has not run for a while; the score here is
    /// unchanged then.
    pub fn set_score(&mut self, mut score: TabScore) -> bool {
        score.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        self.collect();
        match self.outgoing.push(Box::new(score.clone())) {
            Ok(()) => {
                self.score = score;
                true
            }
            Err(_) => false,
        }
    }

    /// The loop as last set from this side.
    pub fn looped(&self) -> &TabLoop {
        &self.looped
    }

    /// Replaces the loop, sorting its notes by tick. Like
    /// [`TabVoiceHandle::set_score`], false when the queue is full.
    pub fn set_loop(&mut self, mut looped: TabLoop) -> bool {
        looped.notes.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        self.collect();
        match self.outgoing_loop.push(Box::new(looped.clone())) {
            Ok(()) => {
                *self.looped = looped;
                true
            }
            Err(_) => false,
        }
    }

    /// Drops every score and loop the audio thread has retired. Returns
    /// how many.
    pub fn collect(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        while self.retired_loop.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::{TempoMap, PPQ};

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;

    fn event(tick: u64, ticks: u64, string: u8, pitch: f32) -> TabEvent {
        TabEvent {
            tick: tick as f64,
            ticks: ticks as f64,
            instrument: Instrument::Guitar,
            note: PluckNote::new(string, pitch, 0.8, 0),
        }
    }

    /// Runs the voice from `start` for `blocks` blocks at `rate`, as the
    /// transport would, and returns the output and the notes plucked.
    fn run(
        voice: &mut TabVoice,
        tempo: &TempoMap,
        start: u64,
        blocks: usize,
        rate: f64,
    ) -> Vec<f32> {
        let mut out = vec![0.0f32; blocks * BLOCK];
        let mut position = start;
        let mut fraction = 0.0;
        for b in 0..blocks {
            let exact = fraction + BLOCK as f64 * rate;
            let span = exact.floor() as u64;
            fraction = exact - exact.floor();
            let ctx = BlockContext {
                sample_rate: SR,
                frames: BLOCK,
                span,
                rate,
                playing: true,
                position,
                loop_range: None,
                count_in: None,
                tempo,
            };
            let mut chunk = [&mut out[b * BLOCK..(b + 1) * BLOCK]];
            voice.process(&ctx, &mut Buffer::new(&mut chunk));
            position += span;
        }
        out
    }

    fn onset(out: &[f32]) -> Option<usize> {
        out.iter().position(|s| s.abs() > 1e-4)
    }

    #[test]
    fn notes_sound_on_their_tick_and_follow_the_practice_speed() {
        let tempo = TempoMap::default();
        // At 120 bpm a quarter note is 24 000 samples; a note on beat 2.
        let quarter = u64::from(PPQ);
        let (mut voice, mut handle) = TabVoice::new();
        voice.prepare(SR, BLOCK);
        assert!(handle.set_score(vec![event(quarter, quarter, 1, 64.0)]));
        let out = run(&mut voice, &tempo, 0, 1_000, 1.0);
        assert_eq!(onset(&out), Some(24_000));
        assert_eq!(voice.stats.notes.load(Ordering::Relaxed), 1);

        // At half speed beat 2 comes twice as late.
        let (mut slow, mut handle) = TabVoice::new();
        slow.prepare(SR, BLOCK);
        handle.set_score(vec![event(quarter, quarter, 1, 64.0)]);
        let out = run(&mut slow, &tempo, 0, 1_000, 0.5);
        assert_eq!(onset(&out), Some(48_000));
    }

    /// Onsets: the first sample of each stretch of sound after silence
    /// of at least `gap` samples.
    fn onsets(out: &[f32], gap: usize) -> Vec<usize> {
        let mut found = Vec::new();
        let mut quiet = gap;
        for (i, s) in out.iter().enumerate() {
            if s.abs() > 1e-4 {
                if quiet >= gap {
                    found.push(i);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        found
    }

    #[test]
    fn a_loop_repeats_its_phrase_until_it_ends() {
        let tempo = TempoMap::default();
        let beat = u64::from(PPQ);
        let (mut voice, mut handle) = TabVoice::new();
        voice.prepare(SR, BLOCK);
        // A short note on beat 2 of every bar from bar 2, for three bars.
        let mut short = event(beat, 60, 1, 64.0);
        short.note.palm_mute = true;
        assert!(handle.set_loop(TabLoop {
            notes: vec![short],
            start: 4 * beat,
            period: 4 * beat,
            until: 16 * beat,
        }));
        let out = run(&mut voice, &tempo, 0, 6 * 96_000 / BLOCK, 1.0);
        // Beat 2 of bars 2, 3 and 4; nothing in bar 5 or after.
        assert_eq!(onsets(&out, 20_000), vec![120_000, 216_000, 312_000]);
        assert_eq!(voice.stats.notes.load(Ordering::Relaxed), 3);
        assert_eq!(handle.looped().period, 4 * beat);
    }

    #[test]
    fn a_loop_pass_that_spans_blocks_plays_each_note_once() {
        let tempo = TempoMap::default();
        let (mut voice, mut handle) = TabVoice::new();
        voice.prepare(SR, BLOCK);
        // A pass of 7 ticks with three notes: many passes in each block.
        handle.set_loop(TabLoop {
            notes: vec![
                event(0, 1, 1, 64.0),
                event(3, 1, 2, 59.0),
                event(5, 1, 3, 55.0),
            ],
            start: 0,
            period: 7,
            until: 700,
        });
        run(&mut voice, &tempo, 0, 400, 1.0);
        assert_eq!(voice.stats.notes.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn a_jump_lets_the_strings_go() {
        let tempo = TempoMap::default();
        let (mut voice, mut handle) = TabVoice::new();
        voice.prepare(SR, BLOCK);
        handle.set_score(vec![event(0, u64::from(PPQ) * 8, 6, 40.0)]);
        let ringing = run(&mut voice, &tempo, 0, 100, 1.0);
        assert!(ringing[6_000..].iter().any(|s| s.abs() > 1e-3));
        // Locate far away: the note fades and nothing new starts.
        let after = run(&mut voice, &tempo, 480_000, 100, 1.0);
        assert!(after[3_000..].iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn scores_are_sorted_and_swapped_without_freeing_on_the_audio_thread() {
        let tempo = TempoMap::default();
        let (mut voice, mut handle) = TabVoice::new();
        voice.prepare(SR, BLOCK);
        handle.set_score(vec![event(960, 240, 1, 64.0), event(0, 240, 2, 59.0)]);
        assert_eq!(handle.score()[0].tick, 0.0);
        handle.set_score(Vec::new());
        run(&mut voice, &tempo, 0, 1, 1.0);
        assert_eq!(voice.stats.swaps.load(Ordering::Relaxed), 2);
        assert_eq!(handle.collect(), 2);
    }
}
