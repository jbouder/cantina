//! The processing graph.
//!
//! A [`Graph`] is built and edited on the UI thread: nodes are added,
//! prepared and wired between the stream input, each other and the stream
//! output. [`Graph::compile`] turns it into a [`Schedule`], a flat list of
//! steps with every buffer preallocated, which the [`Runtime`] swaps in on
//! the audio thread.
//!
//! Nodes are shared between consecutive schedules. An edit that adds a
//! pedal leaves the others running with their state intact, and only the
//! removed ones are dropped, on the UI thread, once the old schedule comes
//! back. Each node lives in a [`NodeSlot`] whose lock is only ever taken by
//! the audio thread: the UI thread touches a node only while no schedule
//! references it, so the lock is never contended and `try_lock` never
//! blocks. A failed `try_lock` is counted and the node is bypassed for that
//! block rather than waited on.
//!
//! [`Runtime`]: crate::runtime::Runtime

use std::cell::Cell;
use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

use cantina_dsp::{Buffer, ParamId, ParamInfo, Processor, MAX_CHANNELS};

use crate::runtime::RuntimeStats;
use crate::tempo::TempoMap;
use crate::transport::LoopRange;

/// Identifies a node for the life of its graph. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

/// One end of an edge: the stream input, the stream output or a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Input,
    Output,
    Node(NodeId),
}

impl From<NodeId> for Endpoint {
    fn from(id: NodeId) -> Self {
        Endpoint::Node(id)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Endpoint::Input => f.write_str("input"),
            Endpoint::Output => f.write_str("output"),
            Endpoint::Node(id) => write!(f, "{id}"),
        }
    }
}

/// What the audio thread knows about the block it is processing.
///
/// The runtime cuts a block at the loop wrap, so within one context the
/// timeline position is contiguous: frame `i` is at `position + i`.
#[derive(Debug, Clone, Copy)]
pub struct BlockContext<'a> {
    pub sample_rate: f64,
    /// Frames in this block.
    pub frames: usize,
    /// Whether the transport is rolling. The position holds while stopped.
    pub playing: bool,
    /// Timeline sample of the first frame.
    pub position: u64,
    /// The active loop, if any.
    pub loop_range: Option<LoopRange>,
    pub tempo: &'a TempoMap,
}

/// Something that runs in the graph. Stock and hosted pedals are wrapped
/// by [`ProcessorNode`]; engine-level nodes such as the click, players and
/// recorders implement this directly because they need the block context.
///
/// The threading rules are those of [`Processor`]: `prepare`, `reset` and
/// `drop` run off the audio thread and may allocate; `process`, `param`
/// and `set_param` run on the audio thread and never allocate, lock,
/// block or perform I/O.
pub trait Node: Send {
    /// Display name.
    fn name(&self) -> &str;

    /// The parameters, indexed by [`ParamId`].
    fn params(&self) -> &[ParamInfo] {
        &[]
    }

    /// Current target value of a parameter.
    fn param(&self, _id: ParamId) -> f32 {
        0.0
    }

    /// Sets a parameter's target. Clamped; unknown ids are ignored.
    fn set_param(&mut self, _id: ParamId, _value: f32) {}

    /// Samples of delay this node adds.
    fn latency(&self) -> usize {
        0
    }

    /// Readies the node for a sample rate and the largest block it will see.
    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize);

    /// Clears memory without touching parameters.
    fn reset(&mut self) {}

    /// Processes a block in place. The buffer holds the sum of the node's
    /// sources, or silence for a node with none.
    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer);
}

/// A [`Processor`] as a graph node.
pub struct ProcessorNode {
    inner: Box<dyn Processor>,
}

impl ProcessorNode {
    pub fn new(processor: Box<dyn Processor>) -> Self {
        Self { inner: processor }
    }

    pub fn processor(&self) -> &dyn Processor {
        self.inner.as_ref()
    }

    pub fn processor_mut(&mut self) -> &mut dyn Processor {
        self.inner.as_mut()
    }

    pub fn into_processor(self) -> Box<dyn Processor> {
        self.inner
    }
}

impl Node for ProcessorNode {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn params(&self) -> &[ParamInfo] {
        self.inner.params()
    }

    fn param(&self, id: ParamId) -> f32 {
        self.inner.param(id)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        self.inner.set_param(id, value);
    }

    fn latency(&self) -> usize {
        self.inner.latency()
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.inner.prepare(sample_rate, max_block_frames);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        self.inner.process(buffer);
    }
}

/// The audio configuration a graph's nodes are prepared for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphConfig {
    pub sample_rate: f64,
    pub max_block_frames: usize,
    /// Channels every buffer in the graph carries, at most [`MAX_CHANNELS`].
    pub channels: usize,
}

/// The UI thread's view of a node. The node itself may be on the audio
/// thread; this is what the engine knows about it without asking.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    pub name: String,
    pub params: Vec<ParamInfo>,
    /// Last value set for each parameter, clamped to its range.
    pub values: Vec<f32>,
    pub latency: usize,
}

/// Holds a node for the audio thread. See the module docs for who may
/// take the lock.
pub(crate) struct NodeSlot {
    node: Mutex<Box<dyn Node>>,
}

impl NodeSlot {
    fn new(node: Box<dyn Node>) -> Self {
        Self {
            node: Mutex::new(node),
        }
    }
}

/// Why a graph edit or compile was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    UnknownNode(NodeId),
    InvalidEdge {
        from: Endpoint,
        to: Endpoint,
        why: &'static str,
    },
    /// The edge would make a cycle.
    Cycle {
        from: Endpoint,
        to: Endpoint,
    },
    TooManyChannels(usize),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::UnknownNode(id) => write!(f, "no node {id} in the graph"),
            GraphError::InvalidEdge { from, to, why } => {
                write!(f, "cannot connect {from} to {to}: {why}")
            }
            GraphError::Cycle { from, to } => {
                write!(f, "connecting {from} to {to} would make a cycle")
            }
            GraphError::TooManyChannels(n) => {
                write!(f, "graphs carry at most {MAX_CHANNELS} channels, not {n}")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// What a parameter change did on the UI thread, and so whether the audio
/// thread needs telling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamRoute {
    /// No such node or parameter.
    Unknown,
    /// The node is not in a running schedule and was set directly.
    Applied,
    /// The node is running; the change must go over the queue.
    Live,
}

struct Entry {
    id: NodeId,
    slot: Arc<NodeSlot>,
    info: NodeInfo,
    /// True once a schedule holding this node has been handed to the
    /// audio thread.
    live: bool,
}

/// A graph of nodes, edited on the UI thread.
pub struct Graph {
    config: GraphConfig,
    next_id: u64,
    nodes: Vec<Entry>,
    edges: Vec<(Endpoint, Endpoint)>,
}

impl Graph {
    pub fn new(config: GraphConfig) -> Result<Self, GraphError> {
        if config.channels == 0 || config.channels > MAX_CHANNELS {
            return Err(GraphError::TooManyChannels(config.channels));
        }
        Ok(Self {
            config,
            next_id: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
        })
    }

    pub fn config(&self) -> GraphConfig {
        self.config
    }

    /// Adds a node, preparing it for the graph's configuration. It is not
    /// connected to anything yet.
    pub fn add(&mut self, mut node: Box<dyn Node>) -> NodeId {
        node.prepare(self.config.sample_rate, self.config.max_block_frames);
        let params = node.params().to_vec();
        let values = (0..params.len()).map(|id| node.param(id)).collect();
        let info = NodeInfo {
            name: node.name().to_string(),
            params,
            values,
            latency: node.latency(),
        };
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(Entry {
            id,
            slot: Arc::new(NodeSlot::new(node)),
            info,
            live: false,
        });
        id
    }

    /// Adds a [`Processor`] as a node.
    pub fn add_processor(&mut self, processor: Box<dyn Processor>) -> NodeId {
        self.add(Box::new(ProcessorNode::new(processor)))
    }

    /// Removes a node and every edge touching it. The node itself is
    /// dropped once no schedule references it.
    pub fn remove(&mut self, id: NodeId) -> Result<(), GraphError> {
        let index = self.index_of(id)?;
        self.nodes.remove(index);
        let gone = Endpoint::Node(id);
        self.edges.retain(|(from, to)| *from != gone && *to != gone);
        Ok(())
    }

    /// Connects two endpoints. A node sums every edge into it.
    pub fn connect(
        &mut self,
        from: impl Into<Endpoint>,
        to: impl Into<Endpoint>,
    ) -> Result<(), GraphError> {
        let (from, to) = (from.into(), to.into());
        let invalid = |why| GraphError::InvalidEdge { from, to, why };
        if from == Endpoint::Output {
            return Err(invalid("the output has no outgoing edges"));
        }
        if to == Endpoint::Input {
            return Err(invalid("the input has no incoming edges"));
        }
        if from == to {
            return Err(invalid("a node cannot feed itself"));
        }
        for endpoint in [from, to] {
            if let Endpoint::Node(id) = endpoint {
                self.index_of(id)?;
            }
        }
        if self.edges.contains(&(from, to)) {
            return Ok(());
        }
        if let (Endpoint::Node(a), Endpoint::Node(b)) = (from, to) {
            if self.reaches(b, a) {
                return Err(GraphError::Cycle { from, to });
            }
        }
        self.edges.push((from, to));
        Ok(())
    }

    /// Removes an edge. Returns whether it existed.
    pub fn disconnect(&mut self, from: impl Into<Endpoint>, to: impl Into<Endpoint>) -> bool {
        let edge = (from.into(), to.into());
        let before = self.edges.len();
        self.edges.retain(|e| *e != edge);
        self.edges.len() != before
    }

    /// Connects `nodes` in series between the input and the output,
    /// clearing any existing edges first. An empty list wires the input
    /// straight to the output.
    pub fn set_chain(&mut self, nodes: &[NodeId]) -> Result<(), GraphError> {
        for id in nodes {
            self.index_of(*id)?;
        }
        self.edges.clear();
        let mut previous = Endpoint::Input;
        for id in nodes {
            self.connect(previous, *id)?;
            previous = Endpoint::Node(*id);
        }
        self.connect(previous, Endpoint::Output)
    }

    /// Node ids in insertion order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|e| e.id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.index_of(id).is_ok()
    }

    pub fn node(&self, id: NodeId) -> Option<&NodeInfo> {
        self.index_of(id).ok().map(|i| &self.nodes[i].info)
    }

    pub fn edges(&self) -> &[(Endpoint, Endpoint)] {
        &self.edges
    }

    /// Records a parameter change. A node that has never been handed to
    /// the audio thread is set directly; otherwise the caller must send
    /// the change over the queue. The clamped value is cached either way.
    pub fn set_param(&mut self, id: NodeId, param: ParamId, value: f32) -> ParamRoute {
        let Ok(index) = self.index_of(id) else {
            return ParamRoute::Unknown;
        };
        let entry = &mut self.nodes[index];
        let Some(info) = entry.info.params.get(param) else {
            return ParamRoute::Unknown;
        };
        let value = info.clamp(value);
        entry.info.values[param] = value;
        if entry.live {
            return ParamRoute::Live;
        }
        if let Ok(mut node) = entry.slot.node.try_lock() {
            node.set_param(param, value);
            entry.info.values[param] = node.param(param);
        }
        ParamRoute::Applied
    }

    /// Compiles the graph into a schedule for the audio thread and marks
    /// every node as live.
    pub(crate) fn compile(&mut self) -> Result<Box<Schedule>, GraphError> {
        let order = self.topological_order();
        debug_assert_eq!(
            order.len(),
            self.nodes.len(),
            "edges keep the graph acyclic"
        );

        let buffer_len = self.config.channels * self.config.max_block_frames;
        let mut buffers = vec![vec![0.0f32; buffer_len]];
        let mut steps = Vec::with_capacity(order.len());
        let mut step_of = vec![usize::MAX; self.nodes.len()];
        for &index in &order {
            let entry = &mut self.nodes[index];
            entry.live = true;
            let buffer = buffers.len();
            buffers.push(vec![0.0f32; buffer_len]);
            step_of[index] = steps.len();
            steps.push(Step {
                id: entry.id,
                slot: Arc::clone(&entry.slot),
                sources: Vec::new(),
                buffer,
            });
        }
        let mut output_sources = Vec::new();
        for &(from, to) in &self.edges {
            let source = match from {
                Endpoint::Input => INPUT_BUFFER,
                Endpoint::Node(id) => steps[step_of[self.index_of(id)?]].buffer,
                Endpoint::Output => unreachable!("connect refuses edges from the output"),
            };
            match to {
                Endpoint::Output => output_sources.push(source),
                Endpoint::Node(id) => steps[step_of[self.index_of(id)?]].sources.push(source),
                Endpoint::Input => unreachable!("connect refuses edges into the input"),
            }
        }
        Ok(Box::new(Schedule {
            channels: self.config.channels,
            max_frames: self.config.max_block_frames,
            steps,
            output_sources,
            buffers,
        }))
    }

    fn index_of(&self, id: NodeId) -> Result<usize, GraphError> {
        self.nodes
            .iter()
            .position(|e| e.id == id)
            .ok_or(GraphError::UnknownNode(id))
    }

    /// True if there is a path of edges from `from` to `to`.
    fn reaches(&self, from: NodeId, to: NodeId) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        while let Some(current) = stack.pop() {
            if current == to {
                return true;
            }
            if !seen.insert(current) {
                continue;
            }
            for (a, b) in &self.edges {
                if let (Endpoint::Node(a), Endpoint::Node(b)) = (a, b) {
                    if *a == current {
                        stack.push(*b);
                    }
                }
            }
        }
        false
    }

    /// Node indices in an order where every node comes after its sources.
    /// Ties keep insertion order so a chain runs in the order it was built.
    fn topological_order(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut incoming = vec![0usize; n];
        for (from, to) in &self.edges {
            if let (Endpoint::Node(_), Endpoint::Node(to)) = (from, to) {
                if let Ok(i) = self.index_of(*to) {
                    incoming[i] += 1;
                }
            }
        }
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        while order.len() < n {
            let Some(next) = (0..n).find(|&i| !done[i] && incoming[i] == 0) else {
                break;
            };
            done[next] = true;
            order.push(next);
            let id = self.nodes[next].id;
            for (from, to) in &self.edges {
                if let (Endpoint::Node(from), Endpoint::Node(to)) = (from, to) {
                    if *from == id {
                        if let Ok(i) = self.index_of(*to) {
                            incoming[i] -= 1;
                        }
                    }
                }
            }
        }
        order
    }
}

const INPUT_BUFFER: usize = 0;

struct Step {
    id: NodeId,
    slot: Arc<NodeSlot>,
    /// Buffers summed into this node's buffer before it runs.
    sources: Vec<usize>,
    buffer: usize,
}

/// A compiled graph: steps in execution order with every buffer
/// preallocated. Built on the UI thread, run on the audio thread, dropped
/// on the UI thread.
pub(crate) struct Schedule {
    channels: usize,
    max_frames: usize,
    steps: Vec<Step>,
    output_sources: Vec<usize>,
    /// Channel-major, `channels * max_frames` each. Index 0 is the input.
    buffers: Vec<Vec<f32>>,
}

thread_local! {
    static ON_AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as the audio thread for the duration of the
/// returned guard.
pub(crate) struct AudioThreadGuard(());

impl AudioThreadGuard {
    pub(crate) fn enter() -> Self {
        ON_AUDIO_THREAD.with(|f| f.set(true));
        Self(())
    }
}

impl Drop for AudioThreadGuard {
    fn drop(&mut self) {
        ON_AUDIO_THREAD.with(|f| f.set(false));
    }
}

/// True while the current thread is inside the runtime's block callback.
pub fn on_audio_thread() -> bool {
    ON_AUDIO_THREAD.with(|f| f.get())
}

impl Schedule {
    /// Sets a parameter on a running node. Audio thread.
    pub(crate) fn set_param(&self, id: NodeId, param: ParamId, value: f32) -> bool {
        let Some(step) = self.steps.iter().find(|s| s.id == id) else {
            return false;
        };
        match step.slot.node.try_lock() {
            Ok(mut node) => {
                node.set_param(param, value);
                true
            }
            Err(_) => false,
        }
    }

    /// Runs one block. Audio thread. `inputs` and `outputs` carry the
    /// graph's channel count; every slice is `ctx.frames` long.
    pub(crate) fn process(
        &mut self,
        ctx: &BlockContext,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        stats: &RuntimeStats,
    ) {
        let frames = ctx.frames.min(self.max_frames);
        let (channels, max) = (self.channels, self.max_frames);
        for (c, input) in inputs.iter().enumerate().take(channels) {
            self.buffers[INPUT_BUFFER][c * max..c * max + frames].copy_from_slice(&input[..frames]);
        }
        for step in &self.steps {
            mix_into(
                &mut self.buffers,
                step.buffer,
                &step.sources,
                channels,
                max,
                frames,
            );
            let dest = &mut self.buffers[step.buffer];
            let mut lanes: [&mut [f32]; MAX_CHANNELS] = std::array::from_fn(|_| &mut [][..]);
            for (lane, chunk) in lanes.iter_mut().zip(dest.chunks_mut(max)) {
                *lane = &mut chunk[..frames];
            }
            let mut buffer = Buffer::new(&mut lanes[..channels]);
            match step.slot.node.try_lock() {
                Ok(mut node) => node.process(ctx, &mut buffer),
                Err(_) => stats.bump_busy(),
            }
        }
        for (c, output) in outputs.iter_mut().enumerate().take(channels) {
            let output = &mut output[..frames];
            output.fill(0.0);
            for &source in &self.output_sources {
                let lane = &self.buffers[source][c * max..c * max + frames];
                for (o, s) in output.iter_mut().zip(lane) {
                    *o += s;
                }
            }
        }
    }
}

impl Drop for Schedule {
    fn drop(&mut self) {
        debug_assert!(
            !on_audio_thread(),
            "a schedule was dropped on the audio thread"
        );
    }
}

/// Sums `sources` into `dest`, or clears it when there are none.
fn mix_into(
    buffers: &mut [Vec<f32>],
    dest: usize,
    sources: &[usize],
    channels: usize,
    max: usize,
    frames: usize,
) {
    if sources.is_empty() {
        for c in 0..channels {
            buffers[dest][c * max..c * max + frames].fill(0.0);
        }
        return;
    }
    for (i, &source) in sources.iter().enumerate() {
        let (d, s) = pair_mut(buffers, dest, source);
        for c in 0..channels {
            let range = c * max..c * max + frames;
            let (d, s) = (&mut d[range.clone()], &s[range]);
            if i == 0 {
                d.copy_from_slice(s);
            } else {
                for (d, s) in d.iter_mut().zip(s) {
                    *d += s;
                }
            }
        }
    }
}

/// Borrows one buffer mutably and another immutably.
fn pair_mut(buffers: &mut [Vec<f32>], a: usize, b: usize) -> (&mut Vec<f32>, &Vec<f32>) {
    debug_assert_ne!(a, b, "a node never feeds itself");
    if a < b {
        let (lo, hi) = buffers.split_at_mut(b);
        (&mut lo[a], &hi[0])
    } else {
        let (lo, hi) = buffers.split_at_mut(a);
        (&mut hi[0], &lo[b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cantina_dsp::Gain;

    fn config() -> GraphConfig {
        GraphConfig {
            sample_rate: 48_000.0,
            max_block_frames: 64,
            channels: 1,
        }
    }

    fn gain(db: f32) -> Box<dyn Processor> {
        Box::new(Gain::new(db))
    }

    fn ctx(frames: usize, tempo: &TempoMap) -> BlockContext<'_> {
        BlockContext {
            sample_rate: 48_000.0,
            frames,
            playing: false,
            position: 0,
            loop_range: None,
            tempo,
        }
    }

    fn run(schedule: &mut Schedule, input: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; input.len()];
        let tempo = TempoMap::default();
        let ctx = ctx(input.len(), &tempo);
        let stats = RuntimeStats::default();
        schedule.process(&ctx, &[input], &mut [&mut output[..]], &stats);
        output
    }

    #[test]
    fn edges_are_validated() {
        let mut g = Graph::new(config()).unwrap();
        let a = g.add_processor(gain(0.0));
        let b = g.add_processor(gain(0.0));
        assert!(matches!(
            g.connect(Endpoint::Output, a),
            Err(GraphError::InvalidEdge { .. })
        ));
        assert!(matches!(
            g.connect(a, Endpoint::Input),
            Err(GraphError::InvalidEdge { .. })
        ));
        assert!(matches!(
            g.connect(a, a),
            Err(GraphError::InvalidEdge { .. })
        ));
        g.connect(a, b).unwrap();
        assert_eq!(
            g.connect(b, a),
            Err(GraphError::Cycle {
                from: b.into(),
                to: a.into()
            })
        );
        g.connect(a, b).unwrap();
        assert_eq!(g.edges().len(), 1, "duplicate edges collapse");
        g.remove(a).unwrap();
        assert!(g.edges().is_empty(), "removing a node removes its edges");
        assert_eq!(g.connect(a, b), Err(GraphError::UnknownNode(a)));
        assert!(!g.disconnect(a, b));
        assert!(Graph::new(GraphConfig {
            channels: 9,
            ..config()
        })
        .is_err());
    }

    #[test]
    fn chain_runs_in_order() {
        let mut g = Graph::new(config()).unwrap();
        let a = g.add_processor(gain(-6.0206));
        let b = g.add_processor(gain(-6.0206));
        g.set_chain(&[a, b]).unwrap();
        let mut s = g.compile().unwrap();
        let out = run(&mut s, &[1.0; 8]);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-4), "{out:?}");
    }

    #[test]
    fn parallel_branches_sum_and_generators_start_silent() {
        let mut g = Graph::new(config()).unwrap();
        let a = g.add_processor(gain(0.0));
        let b = g.add_processor(gain(-6.0206));
        let lonely = g.add_processor(gain(12.0));
        g.connect(Endpoint::Input, a).unwrap();
        g.connect(Endpoint::Input, b).unwrap();
        g.connect(a, Endpoint::Output).unwrap();
        g.connect(b, Endpoint::Output).unwrap();
        g.connect(lonely, Endpoint::Output).unwrap();
        let mut s = g.compile().unwrap();
        let out = run(&mut s, &[1.0; 8]);
        assert!(out.iter().all(|v| (v - 1.5).abs() < 1e-4), "{out:?}");
    }

    #[test]
    fn disconnected_output_is_silent_and_empty_chain_passes_through() {
        let mut g = Graph::new(config()).unwrap();
        let mut s = g.compile().unwrap();
        assert!(run(&mut s, &[1.0; 4]).iter().all(|v| *v == 0.0));
        g.set_chain(&[]).unwrap();
        let mut s = g.compile().unwrap();
        assert_eq!(run(&mut s, &[0.5; 4]), vec![0.5; 4]);
    }

    #[test]
    fn params_apply_directly_until_live() {
        let mut g = Graph::new(config()).unwrap();
        let a = g.add_processor(gain(0.0));
        assert_eq!(g.set_param(a, 0, -200.0), ParamRoute::Applied);
        assert_eq!(g.node(a).unwrap().values[0], -60.0, "clamped to the range");
        assert_eq!(g.set_param(a, 7, 1.0), ParamRoute::Unknown);
        let _schedule = g.compile().unwrap();
        assert_eq!(g.set_param(a, 0, -12.0), ParamRoute::Live);
        assert_eq!(g.node(a).unwrap().values[0], -12.0);
    }

    #[test]
    fn insertion_order_breaks_ties() {
        let mut g = Graph::new(config()).unwrap();
        let ids: Vec<_> = (0..5).map(|_| g.add_processor(gain(0.0))).collect();
        // Wire backwards so the order is forced by edges, not insertion.
        g.set_chain(&[ids[4], ids[3], ids[2], ids[1], ids[0]])
            .unwrap();
        let s = g.compile().unwrap();
        let order: Vec<_> = s.steps.iter().map(|s| s.id).collect();
        assert_eq!(order, vec![ids[4], ids[3], ids[2], ids[1], ids[0]]);
    }

    #[test]
    fn stereo_lanes_are_independent() {
        let mut g = Graph::new(GraphConfig {
            channels: 2,
            ..config()
        })
        .unwrap();
        let a = g.add_processor(gain(-6.0206));
        g.set_chain(&[a]).unwrap();
        let mut s = g.compile().unwrap();
        let (left, right) = ([1.0f32; 4], [0.5f32; 4]);
        let (mut out_l, mut out_r) = ([0.0f32; 4], [0.0f32; 4]);
        let tempo = TempoMap::default();
        s.process(
            &ctx(4, &tempo),
            &[&left, &right],
            &mut [&mut out_l, &mut out_r],
            &RuntimeStats::default(),
        );
        assert!(out_l.iter().all(|v| (v - 0.5).abs() < 1e-4));
        assert!(out_r.iter().all(|v| (v - 0.25).abs() < 1e-4));
    }
}
