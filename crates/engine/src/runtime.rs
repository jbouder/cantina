//! The real-time graph runtime and its UI-thread controller.
//!
//! [`Runtime`] lives on the audio thread and runs the active
//! [`Schedule`]. [`Controller`] lives on the UI thread, owns the editable
//! [`Graph`], and talks to the runtime over three lock-free queues:
//!
//! - compiled schedules go in, and the runtime swaps one in at the start
//!   of a block, so the change lands on a block boundary and never
//!   mid-buffer;
//! - the replaced schedule comes back out, so it is dropped here and never
//!   on the audio thread;
//! - parameter changes go in, addressed by node, and are applied before
//!   the block runs. Smoothing is the node's job, so a step from the UI
//!   never clicks.
//!
//! The runtime also owns the [`Transport`]; its handle lives on the
//! controller and its commands land at the same block boundary. A block is
//! cut into segments at the loop wrap, each processed with its own
//! [`BlockContext`], so loops are sample-accurate.
//!
//! Nothing in [`Runtime::run`] allocates, locks, blocks or performs I/O.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cantina_dsp::{ParamId, MAX_CHANNELS};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::{
    AudioThreadGuard, BlockContext, Graph, GraphConfig, GraphError, NodeId, ParamRoute, Schedule,
};
use crate::stream::BlockProcessor;
use crate::tempo::TempoMap;
use crate::transport::{Transport, TransportHandle};

/// A parameter change addressed to one node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamChange {
    pub node: NodeId,
    pub param: ParamId,
    pub value: f32,
}

/// Schedules that can wait for the audio thread before `commit` refuses.
const SCHEDULE_QUEUE: usize = 8;
/// Retired schedules that can wait for the UI thread to collect them.
const GARBAGE_QUEUE: usize = SCHEDULE_QUEUE * 2;
/// Parameter changes that can wait between two blocks. UI gestures
/// produce far fewer; a full queue refuses the newest change.
const PARAM_QUEUE: usize = 1024;

/// Counters the audio thread bumps. Read from any thread.
#[derive(Debug, Default)]
pub struct RuntimeStats {
    pub blocks: AtomicU64,
    /// Schedules swapped in.
    pub swaps: AtomicU64,
    /// Node slots found locked, which should never happen; the node was
    /// bypassed for that block.
    pub busy_nodes: AtomicU64,
    /// Parameter changes for nodes not in the running schedule.
    pub params_dropped: AtomicU64,
    /// Retired schedules and tempo maps the UI thread never collected,
    /// leaked rather than freed on the audio thread.
    pub leaked: AtomicU64,
}

impl RuntimeStats {
    pub(crate) fn bump_busy(&self) {
        self.busy_nodes.fetch_add(1, Ordering::Relaxed);
    }
}

/// Why [`Controller::commit`] could not hand a schedule over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    Graph(GraphError),
    /// The audio thread has not taken the last schedules yet. Try again
    /// after it has run a block.
    QueueFull,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::Graph(e) => write!(f, "{e}"),
            CommitError::QueueFull => {
                f.write_str("the audio thread has not taken the last schedule yet")
            }
        }
    }
}

impl std::error::Error for CommitError {}

impl From<GraphError> for CommitError {
    fn from(e: GraphError) -> Self {
        CommitError::Graph(e)
    }
}

/// Anything retired by the audio thread that must be freed elsewhere.
enum Garbage {
    Schedule(Box<Schedule>),
    TempoMap(Box<TempoMap>),
}

/// The audio-thread half. Implements [`BlockProcessor`]: the input is
/// copied to every graph channel; a mono stream takes channel 0 and a
/// stereo stream takes channels 0 and 1, or channel 0 on both sides from
/// a one-channel graph.
pub struct Runtime {
    config: GraphConfig,
    active: Option<Box<Schedule>>,
    incoming: Consumer<Box<Schedule>>,
    retired: Producer<Garbage>,
    params: Consumer<ParamChange>,
    transport: Transport,
    stats: Arc<RuntimeStats>,
    /// Scratch for the mono stream adapter.
    scratch: Vec<f32>,
}

/// The UI-thread half.
pub struct Controller {
    graph: Graph,
    schedules: Producer<Box<Schedule>>,
    retired: Consumer<Garbage>,
    params: Producer<ParamChange>,
    transport: TransportHandle,
    stats: Arc<RuntimeStats>,
}

impl Runtime {
    /// Creates a runtime and its controller. The controller's graph starts
    /// empty, which plays silence until a schedule is committed, and the
    /// transport starts stopped at zero with a default tempo map.
    pub fn new(config: GraphConfig) -> Result<(Runtime, Controller), GraphError> {
        let graph = Graph::new(config)?;
        let (schedules, incoming) = RingBuffer::new(SCHEDULE_QUEUE);
        let (retired_in, retired_out) = RingBuffer::new(GARBAGE_QUEUE);
        let (params_in, params_out) = RingBuffer::new(PARAM_QUEUE);
        let (transport, transport_handle) = Transport::new(TempoMap::default());
        let stats = Arc::new(RuntimeStats::default());
        let runtime = Runtime {
            config,
            active: None,
            incoming,
            retired: retired_in,
            params: params_out,
            transport,
            stats: Arc::clone(&stats),
            scratch: vec![0.0; config.channels * config.max_block_frames],
        };
        let controller = Controller {
            graph,
            schedules,
            retired: retired_out,
            params: params_in,
            transport: transport_handle,
            stats,
        };
        Ok((runtime, controller))
    }

    pub fn config(&self) -> GraphConfig {
        self.config
    }

    pub fn stats(&self) -> &Arc<RuntimeStats> {
        &self.stats
    }

    /// Runs one block. `inputs` and `outputs` carry the graph's channel
    /// count; every slice is the same length, at most the configured
    /// maximum. Audio thread.
    pub fn run(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) {
        let _audio = AudioThreadGuard::enter();
        self.stats.blocks.fetch_add(1, Ordering::Relaxed);
        self.swap_in_latest();
        self.apply_params();
        let (retired, stats) = (&mut self.retired, &self.stats);
        self.transport
            .apply_commands(|map| retire(retired, stats, Garbage::TempoMap(map)));

        let frames = outputs
            .first()
            .map_or(0, |o| o.len())
            .min(self.config.max_block_frames);
        let channels = self.config.channels.min(outputs.len());
        let mut offset = 0;
        while offset < frames {
            let segment = self.transport.next_segment(frames - offset);
            let end = offset + segment.frames;
            let ctx = BlockContext {
                sample_rate: self.config.sample_rate,
                frames: segment.frames,
                playing: segment.playing,
                position: segment.position,
                loop_range: self.transport.loop_range(),
                tempo: self.transport.tempo_map(),
            };
            let mut ins: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
            for (lane, input) in ins.iter_mut().zip(inputs.iter()) {
                *lane = &input[offset..end];
            }
            let mut outs: [&mut [f32]; MAX_CHANNELS] = std::array::from_fn(|_| &mut [][..]);
            for (lane, output) in outs.iter_mut().zip(outputs.iter_mut()) {
                *lane = &mut output[offset..end];
            }
            match self.active.as_mut() {
                Some(schedule) => {
                    schedule.process(&ctx, &ins[..channels], &mut outs[..channels], &self.stats)
                }
                None => {
                    for output in outs.iter_mut() {
                        output.fill(0.0);
                    }
                }
            }
            offset = end;
        }
        self.transport.publish();
    }

    /// Runs the graph over whole signals offline, in blocks of
    /// `block_frames`, as fast as the machine allows. `inputs` is
    /// channel-major with the graph's channel count; fewer channels are
    /// treated as silence. Returns the output the same shape as the input.
    pub fn render(&mut self, inputs: &[Vec<f32>], block_frames: usize) -> Vec<Vec<f32>> {
        let frames = inputs.first().map_or(0, |c| c.len());
        let channels = self.config.channels;
        let block = block_frames.clamp(1, self.config.max_block_frames);
        let silence = vec![0.0f32; frames];
        let mut outputs = vec![vec![0.0f32; frames]; channels];
        let mut start = 0;
        while start < frames {
            let end = (start + block).min(frames);
            let mut ins: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
            for (lane, c) in ins.iter_mut().zip(0..channels) {
                *lane = &inputs.get(c).unwrap_or(&silence)[start..end];
            }
            let mut outs: [&mut [f32]; MAX_CHANNELS] = std::array::from_fn(|_| &mut [][..]);
            for (lane, output) in outs.iter_mut().zip(outputs.iter_mut()) {
                *lane = &mut output[start..end];
            }
            self.run(&ins[..channels], &mut outs[..channels]);
            start = end;
        }
        outputs
    }

    /// Takes the newest queued schedule, retiring the current one and
    /// anything older that was queued behind it.
    fn swap_in_latest(&mut self) {
        while let Ok(next) = self.incoming.pop() {
            if let Some(old) = self.active.replace(next) {
                self.retire(old);
            }
            self.stats.swaps.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn retire(&mut self, schedule: Box<Schedule>) {
        retire(&mut self.retired, &self.stats, Garbage::Schedule(schedule));
    }

    fn apply_params(&mut self) {
        while let Ok(change) = self.params.pop() {
            let applied = self
                .active
                .as_ref()
                .is_some_and(|s| s.set_param(change.node, change.param, change.value));
            if !applied {
                self.stats.params_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Hands garbage to the UI thread, or leaks it when the UI thread has
/// stopped collecting: freeing it here would break the real-time rules.
fn retire(queue: &mut Producer<Garbage>, stats: &RuntimeStats, garbage: Garbage) {
    if let Err(rtrb::PushError::Full(garbage)) = queue.push(garbage) {
        std::mem::forget(garbage);
        stats.leaked.fetch_add(1, Ordering::Relaxed);
    }
}

impl Runtime {
    /// Runs the graph on `input` into the scratch lanes and returns them
    /// with the frame count; the caller puts them back.
    fn run_into_scratch(&mut self, input: &[f32]) -> (Vec<f32>, usize) {
        let frames = input.len().min(self.config.max_block_frames);
        let channels = self.config.channels;
        let inputs: [&[f32]; MAX_CHANNELS] = [&input[..frames]; MAX_CHANNELS];
        let mut scratch = std::mem::take(&mut self.scratch);
        {
            let mut outputs: [&mut [f32]; MAX_CHANNELS] = std::array::from_fn(|_| &mut [][..]);
            for (lane, chunk) in outputs
                .iter_mut()
                .zip(scratch.chunks_mut(self.config.max_block_frames))
            {
                *lane = &mut chunk[..frames];
            }
            self.run(&inputs[..channels], &mut outputs[..channels]);
        }
        (scratch, frames)
    }
}

impl BlockProcessor for Runtime {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let (scratch, frames) = self.run_into_scratch(input);
        output[..frames].copy_from_slice(&scratch[..frames]);
        self.scratch = scratch;
    }

    fn process_stereo(&mut self, input: &[f32], left: &mut [f32], right: &mut [f32]) {
        let (scratch, frames) = self.run_into_scratch(input);
        let lane = self.config.max_block_frames;
        left[..frames].copy_from_slice(&scratch[..frames]);
        if self.config.channels >= 2 {
            right[..frames].copy_from_slice(&scratch[lane..lane + frames]);
        } else {
            right[..frames].copy_from_slice(&scratch[..frames]);
        }
        self.scratch = scratch;
    }
}

impl Controller {
    /// The graph as the UI sees it. Edits take effect on [`commit`].
    ///
    /// [`commit`]: Controller::commit
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut Graph {
        &mut self.graph
    }

    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }

    /// The transport: play, stop, locate, loop and the tempo map.
    pub fn transport(&self) -> &TransportHandle {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut TransportHandle {
        &mut self.transport
    }

    /// Compiles the graph and hands it to the audio thread, which swaps it
    /// in at its next block. Collects retired schedules first.
    pub fn commit(&mut self) -> Result<(), CommitError> {
        self.collect();
        if self.schedules.is_full() {
            return Err(CommitError::QueueFull);
        }
        let schedule = self.graph.compile()?;
        self.schedules
            .push(schedule)
            .map_err(|_| CommitError::QueueFull)
    }

    /// Changes a parameter, directly if the node is not running yet and
    /// over the queue otherwise. Returns false for an unknown node or
    /// parameter, or when the queue is full.
    pub fn set_param(&mut self, node: NodeId, param: ParamId, value: f32) -> bool {
        match self.graph.set_param(node, param, value) {
            ParamRoute::Unknown => false,
            ParamRoute::Applied => true,
            ParamRoute::Live => self
                .params
                .push(ParamChange {
                    node,
                    param,
                    value: self.graph.node(node).map_or(value, |n| n.values[param]),
                })
                .is_ok(),
        }
    }

    /// Drops everything the audio thread has retired. Returns how many
    /// schedules and tempo maps were freed. Call from the UI thread now and
    /// then; `commit` does it too.
    pub fn collect(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(garbage) = self.retired.pop() {
            match garbage {
                Garbage::Schedule(schedule) => drop(schedule),
                Garbage::TempoMap(map) => drop(map),
            }
            freed += 1;
        }
        freed
    }

    /// True when the audio thread has gone away, so commits will never be
    /// taken.
    pub fn is_abandoned(&self) -> bool {
        self.schedules.is_abandoned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, Node};
    use crate::tempo::MusicalPosition;
    use crate::transport::LoopRange;
    use cantina_dsp::{stock, Buffer, Gain};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use std::thread::{self, ThreadId};

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;

    fn config(channels: usize) -> GraphConfig {
        GraphConfig {
            sample_rate: SR,
            max_block_frames: BLOCK,
            channels,
        }
    }

    fn sine(frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin() * 0.5)
            .collect()
    }

    /// Runs the whole signal through the runtime in blocks, calling `at`
    /// before each block with its index.
    fn render(runtime: &mut Runtime, input: &[f32], mut at: impl FnMut(usize)) -> Vec<f32> {
        let mut output = vec![0.0; input.len()];
        for (i, (inp, out)) in input
            .chunks(BLOCK)
            .zip(output.chunks_mut(BLOCK))
            .enumerate()
        {
            at(i);
            runtime.process(inp, out);
        }
        output
    }

    #[test]
    fn empty_graph_is_silent_and_chain_processes() {
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let input = sine(BLOCK * 4);
        let out = render(&mut runtime, &input, |_| {});
        assert!(out.iter().all(|v| *v == 0.0));

        let g = controller
            .graph_mut()
            .add_processor(Box::new(Gain::new(-6.0206)));
        controller.graph_mut().set_chain(&[g]).unwrap();
        controller.commit().unwrap();
        let out = render(&mut runtime, &input, |_| {});
        for (o, i) in out.iter().zip(&input) {
            assert!((o - i * 0.5).abs() < 1e-4);
        }
        assert_eq!(controller.stats().swaps.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn swap_keeps_running_nodes_and_their_state() {
        let input = sine(BLOCK * 40);

        // Reference: overdrive alone for the whole signal.
        let (mut reference, mut rc) = Runtime::new(config(1)).unwrap();
        let od = rc.graph_mut().add_processor(stock("overdrive").unwrap());
        rc.graph_mut().set_chain(&[od]).unwrap();
        rc.commit().unwrap();
        let expected = render(&mut reference, &input, |_| {});

        // Same, but a unity gain is inserted after the overdrive halfway
        // through. The overdrive's filters keep their state, so the output
        // is identical to the reference.
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let od = controller
            .graph_mut()
            .add_processor(stock("overdrive").unwrap());
        controller.graph_mut().set_chain(&[od]).unwrap();
        controller.commit().unwrap();
        let actual = render(&mut runtime, &input, |block| {
            if block == 20 {
                let unity = controller
                    .graph_mut()
                    .add_processor(Box::new(Gain::new(0.0)));
                controller.graph_mut().set_chain(&[od, unity]).unwrap();
                controller.commit().unwrap();
            }
        });
        assert_eq!(controller.stats().swaps.load(Ordering::Relaxed), 2);
        assert_eq!(controller.collect(), 1, "the first schedule came back");
        let worst = expected
            .iter()
            .zip(&actual)
            .map(|(e, a)| (e - a).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "swap changed the output by {worst}");

        // A fresh overdrive would not match: its tone filter starts empty.
        let (mut fresh, mut fc) = Runtime::new(config(1)).unwrap();
        let od2 = fc.graph_mut().add_processor(stock("overdrive").unwrap());
        fc.graph_mut().set_chain(&[od2]).unwrap();
        fc.commit().unwrap();
        let _ = render(&mut fresh, &input[..BLOCK * 20], |_| {});
        let od3 = fc.graph_mut().add_processor(stock("overdrive").unwrap());
        fc.graph_mut().set_chain(&[od3]).unwrap();
        fc.commit().unwrap();
        let tail = render(&mut fresh, &input[BLOCK * 20..], |_| {});
        let differs = expected[BLOCK * 20..]
            .iter()
            .zip(&tail)
            .any(|(e, a)| (e - a).abs() > 1e-4);
        assert!(differs, "replacing the node should have reset its state");
    }

    #[test]
    fn queued_param_change_is_smoothed() {
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let g = controller.graph_mut().add_processor(stock("gain").unwrap());
        controller.graph_mut().set_chain(&[g]).unwrap();
        controller.commit().unwrap();
        let input = vec![1.0f32; BLOCK * 40];
        let out = render(&mut runtime, &input, |block| {
            if block == 4 {
                assert!(controller.set_param(g, 0, -20.0));
            }
        });
        assert_eq!(out[BLOCK * 4 - 1], 1.0, "unchanged before the block");
        assert!(
            (out[out.len() - 1] - 0.1).abs() < 1e-3,
            "reached the target"
        );
        let max_step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.01, "largest step was {max_step}");
        assert_eq!(controller.graph().node(g).unwrap().values[0], -20.0);
        assert_eq!(controller.stats().params_dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn params_for_missing_nodes_are_counted_not_crashed() {
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let g = controller.graph_mut().add_processor(stock("gain").unwrap());
        controller.commit().unwrap();
        controller.graph_mut().remove(g).unwrap();
        // The UI's graph forgets the node, so the change is refused here.
        assert!(!controller.set_param(g, 0, -3.0));
        // A change for a node the schedule does not know is dropped there.
        controller
            .params
            .push(ParamChange {
                node: g,
                param: 0,
                value: -3.0,
            })
            .unwrap();
        controller.commit().unwrap();
        runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
        assert_eq!(controller.stats().params_dropped.load(Ordering::Relaxed), 1);
    }

    /// Records the thread that dropped it.
    struct Witness {
        dropped_on: Arc<Mutex<Option<ThreadId>>>,
    }

    impl Node for Witness {
        fn name(&self) -> &str {
            "witness"
        }
        fn prepare(&mut self, _: f64, _: usize) {}
        fn process(&mut self, _: &BlockContext, _: &mut Buffer) {}
    }

    impl Drop for Witness {
        fn drop(&mut self) {
            *self.dropped_on.lock().unwrap() = Some(thread::current().id());
        }
    }

    #[test]
    fn replaced_schedule_and_removed_node_drop_off_the_audio_thread() {
        let dropped_on = Arc::new(Mutex::new(None));
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let w = controller.graph_mut().add(Box::new(Witness {
            dropped_on: Arc::clone(&dropped_on),
        }));
        controller.graph_mut().set_chain(&[w]).unwrap();
        controller.commit().unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::clone(runtime.stats());
        let audio = thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                while !stop.load(Ordering::Relaxed) {
                    runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
                    thread::sleep(std::time::Duration::from_micros(200));
                }
                thread::current().id()
            }
        });

        controller.graph_mut().remove(w).unwrap();
        controller.commit().unwrap();
        while stats.swaps.load(Ordering::Relaxed) < 2 {
            thread::yield_now();
        }
        assert!(
            dropped_on.lock().unwrap().is_none(),
            "still held by the retired schedule"
        );
        assert_eq!(controller.collect(), 1);
        stop.store(true, Ordering::Relaxed);
        let audio_thread = audio.join().unwrap();
        let freed_on = dropped_on
            .lock()
            .unwrap()
            .expect("node was dropped by collect");
        assert_eq!(freed_on, thread::current().id());
        assert_ne!(freed_on, audio_thread);
        assert_eq!(stats.leaked.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn commit_refuses_when_the_audio_thread_is_not_draining() {
        let (_runtime, mut controller) = Runtime::new(config(1)).unwrap();
        for _ in 0..SCHEDULE_QUEUE {
            controller.commit().unwrap();
        }
        assert_eq!(controller.commit(), Err(CommitError::QueueFull));
    }

    #[test]
    fn stereo_runtime_takes_mono_stream_on_both_lanes() {
        let (mut runtime, mut controller) = Runtime::new(config(2)).unwrap();
        let g = controller
            .graph_mut()
            .add_processor(Box::new(Gain::new(-6.0206)));
        controller.graph_mut().set_chain(&[g]).unwrap();
        controller.commit().unwrap();
        let mut out = [0.0f32; 16];
        runtime.process(&[1.0; 16], &mut out);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-4));
        let mut left = [0.0f32; 4];
        let mut right = [0.0f32; 4];
        runtime.run(&[&[1.0; 4], &[0.0; 4]], &mut [&mut left, &mut right]);
        assert!(left.iter().all(|v| (v - 0.5).abs() < 1e-4));
        assert!(right.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn click_follows_transport_and_loops_sample_accurately() {
        let (mut runtime, mut controller) = Runtime::new(config(1)).unwrap();
        let click = controller
            .graph_mut()
            .add(Box::new(crate::click::Click::default()));
        controller
            .graph_mut()
            .connect(click, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();

        // Stopped: silence, position holds.
        let silent = render(&mut runtime, &vec![0.0; BLOCK * 10], |_| {});
        assert!(silent.iter().all(|v| *v == 0.0));
        assert_eq!(controller.transport().position(), 0);

        // Loop over the first beat and a half at 120 bpm: 36 000 samples.
        // The click at the loop start recurs every 36 000 samples, the
        // second beat's click at 24 000 into each pass.
        controller.transport_mut().set_loop(Some(LoopRange {
            start: 0,
            end: 36_000,
        }));
        controller.transport_mut().play();
        let out = render(&mut runtime, &vec![0.0; 36_000 * 3], |_| {});
        let loud = |i: usize| out[i].abs() > 1e-4;
        for pass in 0..3 {
            let base = pass * 36_000;
            assert!(loud(base), "downbeat at {base}");
            if pass > 0 {
                assert!(!loud(base - 1), "silence before {base}");
            }
            assert!(loud(base + 24_000), "beat two at {}", base + 24_000);
            assert!(!loud(base + 24_000 - 1));
            assert!(!loud(base + 35_000), "rang out before the wrap");
        }
        assert!(controller.transport().is_playing());
        assert_eq!(
            controller.transport().position(),
            0,
            "back at the loop start"
        );
        assert_eq!(
            controller.transport().musical_position(SR),
            MusicalPosition::new(0, 0, 0)
        );

        // A new tempo map takes effect and the old one comes back here.
        let mut faster = TempoMap::default();
        faster.set_tempo(0, 240.0);
        controller.transport_mut().set_tempo_map(faster);
        controller.transport_mut().set_loop(None);
        controller.transport_mut().locate(0);
        let out = render(&mut runtime, &vec![0.0; 24_000], |_| {});
        assert!(
            loud(0) && out[12_000].abs() > 1e-4,
            "beats every 12 000 at 240 bpm"
        );
        assert!(out[12_000 - 1].abs() <= 1e-4);
        assert_eq!(controller.collect(), 1, "the old tempo map was retired");
        assert_eq!(controller.stats().leaked.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn offline_render_matches_block_processing() {
        let (mut runtime, mut controller) = Runtime::new(config(2)).unwrap();
        let g = controller
            .graph_mut()
            .add_processor(stock("overdrive").unwrap());
        controller.graph_mut().set_chain(&[g]).unwrap();
        controller.commit().unwrap();
        let input = sine(BLOCK * 10 + 17);
        let rendered = runtime.render(std::slice::from_ref(&input), 50);
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].len(), input.len());
        assert!(
            rendered[1].iter().all(|s| *s == 0.0),
            "missing lane is silence"
        );

        let (mut reference, mut rc) = Runtime::new(config(1)).unwrap();
        let g = rc.graph_mut().add_processor(stock("overdrive").unwrap());
        rc.graph_mut().set_chain(&[g]).unwrap();
        rc.commit().unwrap();
        let expected = render(&mut reference, &input, |_| {});
        for (a, e) in rendered[0].iter().zip(&expected) {
            assert!((a - e).abs() < 1e-6);
        }
    }

    #[test]
    fn dropping_the_runtime_abandons_the_controller() {
        let (runtime, controller) = Runtime::new(config(1)).unwrap();
        assert!(!controller.is_abandoned());
        drop(runtime);
        assert!(controller.is_abandoned());
    }
}
