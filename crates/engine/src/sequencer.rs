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
use crate::tempo::{TempoMap, PPQ};

/// A pattern as the sequencer plays it: a grid of velocities, with pads
/// already resolved to the kit's pad indices.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternData {
    /// Steps per quarter note.
    pub steps_per_beat: u32,
    /// Steps per pass.
    pub length: u32,
    /// 0 straight; 1 delays every second step by a third of a step.
    pub swing: f32,
    /// Pad index in the kit and that pad's velocities, one per step.
    pub rows: Vec<(usize, Vec<f32>)>,
}

impl PatternData {
    pub fn step_ticks(&self) -> u64 {
        PPQ as u64 / self.steps_per_beat.max(1) as u64
    }

    /// Ticks in one pass.
    pub fn ticks(&self) -> u64 {
        self.step_ticks() * self.length.max(1) as u64
    }

    /// How far a step is delayed by the swing, in ticks: every second
    /// step by up to a third of a step.
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
    pub pattern: Arc<PatternData>,
    pub start: u64,
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

    /// Queues every hit whose sample lies in `position..position + frames`.
    fn queue_hits(&mut self, tempo: &TempoMap, position: u64, frames: usize, sample_rate: f64) {
        let end = position + frames as u64;
        let t0 = tempo.samples_to_ticks(position, sample_rate);
        let t1 = tempo.samples_to_ticks(end, sample_rate);
        for placement in self.score.iter() {
            let pattern = &placement.pattern;
            let step = pattern.step_ticks();
            let cycle = pattern.ticks();
            if step == 0 || cycle == 0 {
                continue;
            }
            // A step base more than a step before the block cannot land
            // in it even swung; one past the block's end cannot either.
            if (placement.end() as f64) < t0 - step as f64 || (placement.start as f64) > t1 {
                continue;
            }
            // Where step 0 of the pattern sits on the absolute tick line.
            let origin = placement.start as i64 - (placement.offset % cycle) as i64;
            let first = ((t0 - origin as f64) / step as f64).floor() as i64 - 1;
            let mut k = first.max(0) as u64;
            loop {
                let base = origin + (k * step) as i64;
                if base as f64 > t1 + step as f64 {
                    break;
                }
                if base >= 0 && (base as u64) >= placement.start && (base as u64) < placement.end()
                {
                    let in_pattern = (k % pattern.length.max(1) as u64) as u32;
                    let tick = base as f64 + pattern.shift(in_pattern);
                    let sample = tempo.ticks_to_samples(tick, sample_rate);
                    if sample >= position && sample < end {
                        let frame = (sample - position) as usize;
                        for (pad, steps) in &pattern.rows {
                            let velocity = steps.get(in_pattern as usize).copied().unwrap_or(0.0);
                            if velocity > 0.0 {
                                if self.sampler.trigger(*pad, velocity, frame) {
                                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                                } else {
                                    self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
                k += 1;
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
            self.queue_hits(ctx.tempo, ctx.position, buffer.frames(), ctx.sample_rate);
        }
        self.sampler.process(buffer);
    }
}

impl SequencerHandle {
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
    use crate::tempo::TimeSignature;
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
        Arc::new(PatternData {
            steps_per_beat: 4,
            length: 16,
            swing,
            rows: vec![(0, kick), (1, snare)],
        })
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
        let pattern = Arc::new(PatternData {
            steps_per_beat: 4,
            length: 16,
            swing: 0.5,
            rows: vec![(0, hats)],
        });
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

    #[test]
    fn pattern_arithmetic() {
        let p = rock(1.0);
        assert_eq!(p.step_ticks(), 240);
        assert_eq!(p.ticks(), 3840);
        assert_eq!(p.shift(0), 0.0);
        assert_eq!(p.shift(1), 80.0);
        assert_eq!(p.shift(2), 0.0);
        let triplets = PatternData {
            steps_per_beat: 3,
            length: 12,
            swing: 0.0,
            rows: Vec::new(),
        };
        assert_eq!(triplets.step_ticks(), 320);
        assert_eq!(triplets.ticks(), 3840);
        assert_eq!(Placement::forever(p).end(), u64::MAX);
    }
}
