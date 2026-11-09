//! The step sequencer: plays drum patterns through a [`Sampler`] under
//! the tempo map.
//!
//! A [`Sequencer`] is a graph node. Its [`SequencerHandle`] on the UI
//! thread sets the [`Score`]: the patterns placed on the timeline, each a
//! [`Placement`] of a pattern from a tick for a number of ticks, looping
//! the pattern inside that span. Every block, the node asks the tempo map
//! which ticks the block covers, works out which steps land inside it and
//! at which frame, and queues each hit on the sampler for exactly that
//! frame. Nothing is remembered between blocks, so a locate, a loop wrap
//! or a tempo change needs no special handling: the next block is simply
//! computed from where the transport is.
//!
//! Positions are in ticks (`PPQ` per quarter note) rather than samples
//! so a pattern follows the tempo: change the tempo and every hit moves
//! to where the beat now is.
//!
//! The score reaches the audio thread the way schedules do: a new one is
//! pushed through a lock-free queue, swapped in at the start of a block,
//! and the old one comes back out to be dropped here.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cantina_dsp::{Buffer, LoadedKit, ParamId, ParamInfo, Processor, Sampler};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::{BlockContext, Node};
use crate::tempo::PPQ;

/// Passes of a pattern the hit table covers when it humanizes or plays by
/// chance: each pass is a little different, and after this many the
/// same passes come round again.
pub const PASSES: u32 = 8;
/// The most a hit is moved by timing humanize, in ticks.
pub const HUMANIZE_TICKS: f64 = 48.0;
/// The most a hit's velocity is moved by velocity humanize, as a fraction.
pub const HUMANIZE_VELOCITY: f32 = 0.3;
/// How far before its step a flam's grace note falls, in ticks.
pub const FLAM_TICKS: f64 = 30.0;
/// A flam's grace note against the hit it leads into.
pub const FLAM_VELOCITY: f32 = 0.45;

/// A pattern as the sequencer plays it: a grid of velocities, with pads
/// already resolved to the kit's pad indices, and how it feels.
///
/// [`PatternData::built`] works out every hit (swing, humanize, chance,
/// ratchets and flams) into a table off the audio thread; the audio
/// thread only looks hits up in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PatternData {
    /// Steps per quarter note.
    pub steps_per_beat: u32,
    /// Steps per pass.
    pub length: u32,
    /// 0 straight; 1 delays every second step by a third of a step.
    pub swing: f32,
    /// Pad index in the kit and that pad's velocities, one per step.
    pub rows: Vec<(usize, Vec<f32>)>,
    /// Each row's own feel, in the order of `rows`; missing rows are plain.
    pub feel: Vec<RowFeel>,
    /// The pattern's humanize, for rows without their own.
    pub humanize: Humanize,
    /// Seeds humanize and chance.
    pub seed: u32,
    hits: Vec<Hit>,
    passes: u32,
}

/// How far hits stray: 0 to 1 for timing and for velocity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Humanize {
    /// Up to [`HUMANIZE_TICKS`] early or late at 1.
    pub timing: f32,
    /// Up to [`HUMANIZE_VELOCITY`] softer or harder at 1.
    pub velocity: f32,
}

/// A row's own swing, humanize and step marks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RowFeel {
    /// Instead of the pattern's swing.
    pub swing: Option<f32>,
    /// Instead of the pattern's humanize.
    pub humanize: Option<Humanize>,
    /// Steps that play by chance, ratchet or flam.
    pub marks: Vec<Mark>,
}

/// What a step does beyond its velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    /// The step, from 0.
    pub step: u32,
    /// The chance it plays on a pass, 0 to 1.
    pub chance: f32,
    /// Hits in the step, 1 to 4.
    pub ratchet: u8,
    /// A soft grace note just before it.
    pub flam: bool,
}

/// One hit in the table: where its step is and where it sounds, in ticks
/// from the start of the table's first pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The step's own tick, which decides whether a clip plays it.
    pub base: f64,
    /// Where it sounds: swung, humanized, ratcheted or a flam's grace.
    pub tick: f64,
    /// Pad index in the kit.
    pub pad: usize,
    /// 0 to 1.
    pub velocity: f32,
}

/// A number from 0 to 1 that depends only on its inputs (splitmix64).
fn chance_of(seed: u32, row: usize, step: u32, pass: u32, salt: u64) -> f64 {
    let mut z = (u64::from(seed) << 32)
        ^ ((row as u64) << 48)
        ^ (u64::from(step) << 16)
        ^ (u64::from(pass) << 8)
        ^ salt;
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

impl PatternData {
    /// A plain pattern: these rows, this swing, no humanize or marks,
    /// with its hits worked out.
    pub fn plain(
        steps_per_beat: u32,
        length: u32,
        swing: f32,
        rows: Vec<(usize, Vec<f32>)>,
    ) -> Self {
        Self {
            steps_per_beat,
            length,
            swing,
            rows,
            ..Self::default()
        }
        .built()
    }

    /// A pattern with a feel, its hits worked out.
    pub fn with_feel(
        steps_per_beat: u32,
        length: u32,
        swing: f32,
        rows: Vec<(usize, Vec<f32>)>,
        feel: Vec<RowFeel>,
        humanize: Humanize,
        seed: u32,
    ) -> Self {
        Self {
            steps_per_beat,
            length,
            swing,
            rows,
            feel,
            humanize,
            seed,
            ..Self::default()
        }
        .built()
    }

    /// Works out every hit into the table the audio thread reads. Call
    /// it, off the audio thread, after setting the fields.
    pub fn built(mut self) -> Self {
        let varies = self.humanize != Humanize::default()
            || self.feel.iter().any(|f| {
                f.humanize.is_some_and(|h| h != Humanize::default())
                    || f.marks.iter().any(|m| m.chance < 1.0)
            });
        self.passes = if varies { PASSES } else { 1 };
        let step = self.step_ticks() as f64;
        let pass_ticks = self.ticks() as f64;
        let mut hits = Vec::new();
        for pass in 0..self.passes {
            for (r, (pad, steps)) in self.rows.iter().enumerate() {
                let feel = self.feel.get(r).cloned().unwrap_or_default();
                let swing = f64::from(feel.swing.unwrap_or(self.swing));
                let human = feel.humanize.unwrap_or(self.humanize);
                for (s, &velocity) in steps.iter().enumerate() {
                    if velocity <= 0.0 {
                        continue;
                    }
                    let s = s as u32;
                    let mark = feel.marks.iter().find(|m| m.step == s).copied();
                    let roll = |salt| chance_of(self.seed, r, s, pass, salt);
                    if let Some(m) = mark {
                        if roll(1) >= f64::from(m.chance) {
                            continue;
                        }
                    }
                    let base = f64::from(pass) * pass_ticks + f64::from(s) * step;
                    let shift = if s % 2 == 1 { swing * step / 3.0 } else { 0.0 };
                    let late = (roll(2) * 2.0 - 1.0) * f64::from(human.timing) * HUMANIZE_TICKS;
                    let louder = (roll(3) as f32 * 2.0 - 1.0) * human.velocity * HUMANIZE_VELOCITY;
                    let velocity = (velocity * (1.0 + louder)).clamp(0.02, 1.0);
                    let at = base + shift + late;
                    let ratchet = mark.map_or(1, |m| m.ratchet.clamp(1, 4));
                    for i in 0..ratchet {
                        hits.push(Hit {
                            base,
                            tick: at + f64::from(i) * step / f64::from(ratchet),
                            pad: *pad,
                            velocity: velocity * (1.0 - 0.12 * f32::from(i)),
                        });
                    }
                    if mark.is_some_and(|m| m.flam) {
                        hits.push(Hit {
                            base,
                            tick: at - FLAM_TICKS,
                            pad: *pad,
                            velocity: (velocity * FLAM_VELOCITY).max(0.02),
                        });
                    }
                }
            }
        }
        hits.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        self.hits = hits;
        self
    }

    /// The hits, by tick, over [`PatternData::cycle`] ticks.
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Ticks the hit table covers: one pass, or [`PASSES`] when the
    /// pattern varies from pass to pass.
    pub fn cycle(&self) -> u64 {
        self.ticks() * u64::from(self.passes.max(1))
    }

    /// Ticks per step.
    pub fn step_ticks(&self) -> u64 {
        PPQ as u64 / self.steps_per_beat.max(1) as u64
    }

    /// Ticks in one pass.
    pub fn ticks(&self) -> u64 {
        self.step_ticks() * self.length.max(1) as u64
    }

    /// How far a step is delayed by the swing, in ticks: every second
    /// step by up to a third of a step.
    #[cfg(test)]
    fn shift(&self, step: u32) -> f64 {
        if step % 2 == 1 {
            self.swing as f64 * self.step_ticks() as f64 / 3.0
        } else {
            0.0
        }
    }
}

/// A pattern on the timeline from `start` for `length` ticks, looping.
/// `offset` ticks into the pattern play first, so a clip trimmed at the
/// front keeps its beats where they were.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The pattern, shared with the audio thread.
    pub pattern: Arc<PatternData>,
    /// Timeline tick it starts at.
    pub start: u64,
    /// Ticks into the pattern it starts from.
    pub offset: u64,
    /// `u64::MAX` plays until stopped.
    pub length: u64,
}

impl Placement {
    /// A pattern looping from tick zero for ever, for auditioning.
    pub fn forever(pattern: Arc<PatternData>) -> Self {
        Self {
            pattern,
            start: 0,
            offset: 0,
            length: u64::MAX,
        }
    }

    fn end(&self) -> u64 {
        self.start.saturating_add(self.length)
    }
}

/// Everything a drum track plays.
pub type Score = Vec<Placement>;

/// Scores that can wait for the audio thread before `set_score` refuses.
const SCORE_QUEUE: usize = 8;
const RETIRED_QUEUE: usize = SCORE_QUEUE * 2;

/// Counters bumped by the audio thread. Read from anywhere.
#[derive(Debug, Default)]
pub struct SequencerStats {
    /// Hits queued on the sampler.
    pub hits: AtomicU64,
    /// Hits the sampler refused because its queue was full.
    pub dropped: AtomicU64,
    /// Scores swapped in.
    pub swaps: AtomicU64,
    /// Retired scores the UI thread never collected, leaked rather than
    /// freed on the audio thread.
    pub leaked: AtomicU64,
}

/// The graph node.
pub struct Sequencer {
    sampler: Sampler,
    score: Box<Score>,
    incoming: Consumer<Box<Score>>,
    retired: Producer<Box<Score>>,
    stats: Arc<SequencerStats>,
}

/// The UI-thread half.
pub struct SequencerHandle {
    outgoing: Producer<Box<Score>>,
    retired: Consumer<Box<Score>>,
    score: Score,
    stats: Arc<SequencerStats>,
}

impl Sequencer {
    /// A sequencer playing `kit` and its handle for the UI thread.
    pub fn new(kit: Arc<LoadedKit>) -> (Sequencer, SequencerHandle) {
        let (outgoing, incoming) = RingBuffer::new(SCORE_QUEUE);
        let (retired_in, retired_out) = RingBuffer::new(RETIRED_QUEUE);
        let stats = Arc::new(SequencerStats::default());
        let sequencer = Sequencer {
            sampler: Sampler::new(kit),
            score: Box::default(),
            incoming,
            retired: retired_in,
            stats: Arc::clone(&stats),
        };
        let handle = SequencerHandle {
            outgoing,
            retired: retired_out,
            score: Vec::new(),
            stats,
        };
        (sequencer, handle)
    }

    /// Counters for this sequencer, shared with its handle.
    pub fn stats(&self) -> &Arc<SequencerStats> {
        &self.stats
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
    }

    /// Queues every hit whose timeline sample lies in the block's span,
    /// on the frame it sounds at. Only reads the patterns' hit tables.
    fn queue_hits(&mut self, ctx: &BlockContext) {
        let (tempo, position, sample_rate) = (ctx.tempo, ctx.position, ctx.sample_rate);
        let end = position + ctx.span;
        let t0 = tempo.samples_to_ticks(position, sample_rate);
        let t1 = tempo.samples_to_ticks(end, sample_rate);
        for placement in self.score.iter() {
            let pattern = &placement.pattern;
            let cycle = pattern.cycle();
            let hits = pattern.hits();
            if cycle == 0 || hits.is_empty() {
                continue;
            }
            // A hit sounds at most a step plus humanize and a flam from
            // its own step; nothing further from the block can land in it.
            let reach = pattern.step_ticks() as f64 + HUMANIZE_TICKS + FLAM_TICKS;
            if (placement.end() as f64) < t0 - reach || (placement.start as f64) > t1 + reach {
                continue;
            }
            // Where the table's first tick sits on the timeline.
            let origin = placement.start as f64 - (placement.offset % cycle) as f64;
            let c = cycle as f64;
            let first = ((t0 - reach - origin) / c).floor().max(0.0) as u64;
            let last = ((t1 + reach - origin) / c).floor().max(0.0) as u64;
            for n in first..=last {
                let at = origin + n as f64 * c;
                let from = hits.partition_point(|h| at + h.tick < t0);
                for hit in &hits[from..] {
                    let tick = at + hit.tick;
                    if tick >= t1 {
                        break;
                    }
                    let base = at + hit.base;
                    if base < placement.start as f64 || base >= placement.end() as f64 {
                        continue;
                    }
                    let sample = tempo.ticks_to_samples(tick.max(0.0), sample_rate);
                    if sample < position || sample >= end {
                        continue;
                    }
                    if self
                        .sampler
                        .trigger(hit.pad, hit.velocity, ctx.frame_of(sample))
                    {
                        self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

impl Node for Sequencer {
    fn name(&self) -> &str {
        "Sequencer"
    }

    fn params(&self) -> &[ParamInfo] {
        self.sampler.params()
    }

    fn param(&self, id: ParamId) -> f32 {
        self.sampler.param(id)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        self.sampler.set_param(id, value);
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.sampler.prepare(sample_rate, max_block_frames);
    }

    fn reset(&mut self) {
        self.sampler.reset();
    }

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        self.swap_in_latest();
        if ctx.playing && buffer.frames() > 0 {
            self.queue_hits(ctx);
        }
        self.sampler.process(buffer);
    }
}

impl SequencerHandle {
    /// Counters for the sequencer, shared with it.
    pub fn stats(&self) -> &Arc<SequencerStats> {
        &self.stats
    }

    /// The score as last set from this side.
    pub fn score(&self) -> &Score {
        &self.score
    }

    /// Replaces the score. The audio thread takes it at its next block.
    /// Returns false when the queue is full, which means the audio
    /// thread has not run for a while; the score here is unchanged then.
    pub fn set_score(&mut self, score: Score) -> bool {
        self.collect();
        match self.outgoing.push(Box::new(score.clone())) {
            Ok(()) => {
                self.score = score;
                true
            }
            Err(_) => false,
        }
    }

    /// Drops every score the audio thread has retired. Returns how many.
    pub fn collect(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::runtime::{Controller, Runtime};
    use crate::stream::BlockProcessor;
    use crate::tempo::{TempoMap, TimeSignature};
    use crate::transport::LoopRange;
    use cantina_dsp::{Kit, Sample};

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;
    /// Samples per tick at 120 bpm.
    const TICK: u64 = 25;

    /// Kick is an impulse of 1, snare an impulse of 0.5: a render reads
    /// as a list of hits.
    fn kit() -> Arc<LoadedKit> {
        let kit = Kit::from_json(
            r#"{"version":1,"name":"t","pads":[
              {"id":"kick","layers":[{"file":"k"}]},
              {"id":"snare","layers":[{"file":"s"}]}]}"#,
        )
        .unwrap();
        Arc::new(
            LoadedKit::build(&kit, |file| {
                Ok(Arc::new(Sample {
                    sample_rate: SR,
                    channels: vec![vec![if file == "k" { 1.0 } else { 0.5 }]],
                }))
            })
            .unwrap(),
        )
    }

    /// A bar of sixteenths: kick on 1 and 3, snare on 2 and 4.
    fn rock(swing: f32) -> Arc<PatternData> {
        let mut kick = vec![0.0; 16];
        let mut snare = vec![0.0; 16];
        kick[0] = 1.0;
        kick[8] = 1.0;
        snare[4] = 1.0;
        snare[12] = 1.0;
        Arc::new(PatternData::plain(
            4,
            16,
            swing,
            vec![(0, kick), (1, snare)],
        ))
    }

    fn runtime_with_sequencer() -> (Runtime, Controller, SequencerHandle) {
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: BLOCK,
            channels: 1,
        })
        .unwrap();
        let (sequencer, handle) = Sequencer::new(kit());
        let id = controller.graph_mut().add(Box::new(sequencer));
        controller
            .graph_mut()
            .connect(id, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();
        runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
        (runtime, controller, handle)
    }

    /// Renders with a mix of block sizes so hits cross boundaries.
    fn render(runtime: &mut Runtime, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        let mut cursor = 0;
        for block in [BLOCK, 17, 1, 63, 40].into_iter().cycle() {
            if cursor >= frames {
                break;
            }
            let n = block.min(frames - cursor);
            let input = vec![0.0; n];
            runtime.process(&input, &mut out[cursor..cursor + n]);
            cursor += n;
        }
        out
    }

    /// `(sample, value)` for every nonzero sample.
    fn hits(out: &[f32]) -> Vec<(usize, f32)> {
        out.iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.0)
            .map(|(i, v)| (i, *v))
            .collect()
    }

    #[test]
    fn a_looping_pattern_hits_every_step_once_across_blocks() {
        let (mut runtime, mut controller, mut handle) = runtime_with_sequencer();
        assert!(handle.set_score(vec![Placement::forever(rock(0.0))]));
        assert!(
            render(&mut runtime, 1000).iter().all(|v| *v == 0.0),
            "silent while stopped"
        );
        controller.transport_mut().play();
        let out = render(&mut runtime, 2 * 96_000 + 10);
        let beat = 24_000;
        assert_eq!(
            hits(&out),
            vec![
                (0, 1.0),
                (beat, 0.5),
                (2 * beat, 1.0),
                (3 * beat, 0.5),
                (4 * beat, 1.0),
                (5 * beat, 0.5),
                (6 * beat, 1.0),
                (7 * beat, 0.5),
                (8 * beat, 1.0),
            ]
        );
        assert_eq!(handle.stats().hits.load(Ordering::Relaxed), 9);
        assert_eq!(handle.stats().dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn swing_delays_every_second_step() {
        let (mut runtime, mut controller, mut handle) = runtime_with_sequencer();
        let mut hats = vec![0.0; 16];
        for (i, v) in hats.iter_mut().enumerate() {
            *v = if i % 2 == 0 { 1.0 } else { 0.5 };
        }
        let pattern = Arc::new(PatternData::plain(4, 16, 0.5, vec![(0, hats)]));
        handle.set_score(vec![Placement::forever(pattern)]);
        controller.transport_mut().play();
        let out = render(&mut runtime, 96_000);
        let step = 6_000;
        // Half swing: odd steps move by half of a third of a step.
        let shift = (240.0 * 0.5 / 3.0 * TICK as f64).round() as usize;
        let expected: Vec<(usize, f32)> = (0..16)
            .map(|i| {
                if i % 2 == 0 {
                    (i * step, 1.0)
                } else {
                    (i * step + shift, 0.5)
                }
            })
            .collect();
        assert_eq!(hits(&out), expected);
    }

    #[test]
    fn a_placement_plays_its_span_from_its_offset() {
        let (mut runtime, mut controller, mut handle) = runtime_with_sequencer();
        // From bar 2, starting a beat into the pattern, for two beats:
        // the pattern's beats 2 and 3 land on bar 2's beats 1 and 2.
        handle.set_score(vec![Placement {
            pattern: rock(0.0),
            start: 4 * PPQ as u64,
            offset: PPQ as u64,
            length: 2 * PPQ as u64,
        }]);
        controller.transport_mut().play();
        let out = render(&mut runtime, 3 * 96_000);
        assert_eq!(hits(&out), vec![(96_000, 0.5), (96_000 + 24_000, 1.0)]);
    }

    #[test]
    fn follows_the_tempo_map_and_a_change_in_it() {
        let (mut runtime, mut controller, mut handle) = runtime_with_sequencer();
        let mut map = TempoMap::new(120.0, TimeSignature::COMMON);
        // Bar 2 at 60 bpm: beats twice as far apart from there.
        map.set_tempo(4 * PPQ as u64, 60.0);
        controller.transport_mut().set_tempo_map(map.clone());
        handle.set_score(vec![Placement::forever(rock(0.0))]);
        controller.transport_mut().play();
        let out = render(&mut runtime, 96_000 + 4 * 48_000 + 1);
        let expected: Vec<(usize, f32)> = (0..9)
            .map(|beat| {
                let sample = map.ticks_to_samples((beat * PPQ) as f64, SR) as usize;
                (sample, if beat % 2 == 0 { 1.0 } else { 0.5 })
            })
            .collect();
        assert_eq!(hits(&out), expected);
        assert_eq!(expected[5].0, 96_000 + 48_000);
    }

    #[test]
    fn loops_with_the_transport_and_swaps_scores_live() {
        let (mut runtime, mut controller, mut handle) = runtime_with_sequencer();
        handle.set_score(vec![Placement::forever(rock(0.0))]);
        // Loop the first beat and a half: the downbeat recurs every
        // 36 000 samples, beat two 24 000 into each pass.
        controller.transport_mut().set_loop(Some(LoopRange {
            start: 0,
            end: 36_000,
        }));
        controller.transport_mut().play();
        let out = render(&mut runtime, 36_000 * 3);
        assert_eq!(
            hits(&out),
            vec![
                (0, 1.0),
                (24_000, 0.5),
                (36_000, 1.0),
                (60_000, 0.5),
                (72_000, 1.0),
                (96_000, 0.5)
            ]
        );
        // A new score takes over at the next block; the old one is
        // retired and collected here.
        controller.transport_mut().set_loop(None);
        controller.transport_mut().locate(0);
        let mut only_snare = (*rock(0.0)).clone();
        only_snare.rows.remove(0);
        let only_snare = only_snare.built();
        handle.set_score(vec![Placement::forever(Arc::new(only_snare))]);
        let out = render(&mut runtime, 96_000);
        assert_eq!(hits(&out), vec![(24_000, 0.5), (72_000, 0.5)]);
        assert_eq!(handle.stats().swaps.load(Ordering::Relaxed), 2);
        assert_eq!(handle.collect(), 1, "the first score came back");
        assert_eq!(handle.stats().leaked.load(Ordering::Relaxed), 0);
        assert_eq!(handle.score().len(), 1);
    }

    #[test]
    fn set_score_refuses_when_the_audio_thread_is_not_draining() {
        let (_sequencer, mut handle) = Sequencer::new(kit());
        for _ in 0..SCORE_QUEUE {
            assert!(handle.set_score(Vec::new()));
        }
        assert!(!handle.set_score(vec![Placement::forever(rock(0.0))]));
        assert!(handle.score().is_empty(), "a refused score is not kept");
    }

    /// One hit on the first step of a one-beat row, with a feel.
    fn felt(feel: RowFeel, humanize: Humanize, seed: u32) -> PatternData {
        PatternData {
            steps_per_beat: 4,
            length: 4,
            swing: 0.0,
            rows: vec![(0, vec![1.0, 0.0, 0.0, 0.0])],
            feel: vec![feel],
            humanize,
            seed,
            ..PatternData::default()
        }
        .built()
    }

    fn mark(step: u32) -> Mark {
        Mark {
            step,
            chance: 1.0,
            ratchet: 1,
            flam: false,
        }
    }

    #[test]
    fn ratchets_flams_and_chance_are_in_the_table() {
        let ratchet = felt(
            RowFeel {
                marks: vec![Mark {
                    ratchet: 3,
                    ..mark(0)
                }],
                ..RowFeel::default()
            },
            Humanize::default(),
            0,
        );
        let ticks: Vec<f64> = ratchet.hits().iter().map(|h| h.tick).collect();
        assert_eq!(ticks, vec![0.0, 80.0, 160.0], "three in one sixteenth");
        assert!(ratchet.hits()[2].velocity < ratchet.hits()[0].velocity);

        let flam = felt(
            RowFeel {
                marks: vec![Mark {
                    flam: true,
                    ..mark(0)
                }],
                ..RowFeel::default()
            },
            Humanize::default(),
            0,
        );
        let hits = flam.hits();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].tick, hits[1].tick),
            (-FLAM_TICKS, 0.0),
            "grace first"
        );
        assert!((hits[0].velocity - FLAM_VELOCITY).abs() < 1e-6);
        assert_eq!(hits[0].base, 0.0, "the grace belongs to its step");

        // A half chance plays on some of the eight passes, never none or all.
        let half = felt(
            RowFeel {
                marks: vec![Mark {
                    chance: 0.5,
                    ..mark(0)
                }],
                ..RowFeel::default()
            },
            Humanize::default(),
            3,
        );
        assert_eq!(half.cycle(), 960 * u64::from(PASSES));
        assert!(
            (1..PASSES as usize).contains(&half.hits().len()),
            "{}",
            half.hits().len()
        );
        let never = felt(
            RowFeel {
                marks: vec![Mark {
                    chance: 0.0,
                    ..mark(0)
                }],
                ..RowFeel::default()
            },
            Humanize::default(),
            3,
        );
        assert!(never.hits().is_empty());
    }

    #[test]
    fn humanize_is_the_same_every_build_and_differs_by_pass_and_seed() {
        let human = Humanize {
            timing: 1.0,
            velocity: 1.0,
        };
        let a = felt(RowFeel::default(), human, 1);
        assert_eq!(a, felt(RowFeel::default(), human, 1), "deterministic");
        assert_ne!(
            a.hits(),
            felt(RowFeel::default(), human, 2).hits(),
            "seeded"
        );
        let offsets: Vec<f64> = a.hits().iter().map(|h| h.tick - h.base).collect();
        assert_eq!(offsets.len(), PASSES as usize);
        assert!(offsets.iter().all(|o| o.abs() <= HUMANIZE_TICKS));
        assert!(offsets.windows(2).any(|w| w[0] != w[1]), "passes differ");
        assert!(a
            .hits()
            .iter()
            .all(|h| (1.0 - HUMANIZE_VELOCITY..=1.0).contains(&h.velocity)));
        // A row's own humanize wins over the pattern's.
        let row_off = felt(
            RowFeel {
                humanize: Some(Humanize::default()),
                ..RowFeel::default()
            },
            human,
            1,
        );
        assert!(row_off.hits().iter().all(|h| h.tick == h.base));
    }

    #[test]
    fn a_rows_swing_overrides_the_patterns() {
        let mut p = PatternData {
            steps_per_beat: 4,
            length: 4,
            swing: 0.0,
            rows: vec![(0, vec![0.0, 1.0, 0.0, 0.0])],
            feel: vec![RowFeel {
                swing: Some(1.0),
                ..RowFeel::default()
            }],
            ..PatternData::default()
        }
        .built();
        assert_eq!(p.hits()[0].tick, 240.0 + 80.0);
        p.feel.clear();
        assert_eq!(p.built().hits()[0].tick, 240.0);
    }

    #[test]
    fn pattern_arithmetic() {
        let p = rock(1.0);
        assert_eq!(p.step_ticks(), 240);
        assert_eq!(p.ticks(), 3840);
        assert_eq!(p.shift(0), 0.0);
        assert_eq!(p.shift(1), 80.0);
        assert_eq!(p.shift(2), 0.0);
        let triplets = PatternData::plain(3, 12, 0.0, Vec::new());
        assert_eq!(triplets.step_ticks(), 320);
        assert_eq!(triplets.ticks(), 3840);
        assert_eq!(Placement::forever(p).end(), u64::MAX);
    }
}
