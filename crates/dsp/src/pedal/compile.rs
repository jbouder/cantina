//! Building a pedal: every rule the format has, checked with the node
//! and field named in each problem, and the compiled processor that
//! runs the graph on the audio thread.
//!
//! Compilation is the only place that allocates. It orders the nodes so
//! that everything a node reads (its audio input, the signals its
//! fields follow) comes earlier, gives each node an output lane sized
//! for the largest block at `prepare`, and turns every field into a
//! [`Number`], [`Choice`] or [`Flag`] that knows where its value comes
//! from. `process` then walks the nodes in order, each reading the
//! lanes before it and writing its own.

// The engines index a lane by channel and frame alongside their input
// and the signals they follow; the iterator forms read worse.
#![allow(clippy::needless_range_loop)]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::kinds::{self, FieldKind, KindInfo};
use super::{
    Binding, NodeDef, ParamDef, PedalDef, PedalError, Problem, Value, MAX_NODES, MAX_PARAMS,
    PEDAL_VERSION,
};
use crate::biquad::Biquad;
use crate::db_to_linear;
use crate::delay_line::DelayLine;
use crate::lfo::{note_beats, Lfo, Shape};
use crate::modulation::Allpass;
use crate::oversample::{Oversampler, LATENCY as OVERSAMPLE_LATENCY};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Transport, Unit, MAX_CHANNELS};
use crate::shaper::Curve;
use crate::smooth::Smoother;

/// Most allpass stages one node may cascade.
const MAX_STAGES: usize = 16;

/// A block of output per channel.
type Lane = [Vec<f32>; MAX_CHANNELS];

/// Checks `def` against every rule, reporting every problem found.
pub(super) fn check(def: &PedalDef) -> Result<(), PedalError> {
    build(def).map(|_| ())
}

/// Validates `def` and builds a processor from it. The processor is
/// unprepared and at the parameters' defaults.
pub fn compile(def: &PedalDef) -> Result<PedalProcessor, PedalError> {
    build(def)
}

// --- fields at run time ---

/// Maps a source value onto a field.
#[derive(Debug, Clone)]
enum Scale {
    /// The value as it is.
    Raw,
    /// `lo` at the bottom of the source's range, `hi` at the top.
    Linear { lo: f32, hi: f32 },
    /// As `Linear`, interpolated exponentially.
    Log { lo: f32, ratio: f32 },
    /// By index, for a choice or toggle parameter.
    Table(Vec<f32>),
}

impl Scale {
    /// The field's value for `t`: a position from 0 to 1 for the
    /// mapped scales, the source's own value otherwise.
    #[inline]
    fn at(&self, t: f32) -> f32 {
        match self {
            Scale::Raw => t,
            Scale::Linear { lo, hi } => lo + (hi - lo) * t,
            Scale::Log { lo, ratio } => lo * ratio.powf(t),
            Scale::Table(table) => {
                let i = (t.round().max(0.0) as usize).min(table.len() - 1);
                table[i]
            }
        }
    }

    fn is_positional(&self) -> bool {
        matches!(self, Scale::Linear { .. } | Scale::Log { .. })
    }
}

/// What a field does with a mapped value before using it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Transform {
    None,
    /// Decibels in, a multiplier out: the gain eases in the linear
    /// domain, as the stock gain does.
    DbToLinear,
}

impl Transform {
    #[inline]
    fn apply(self, value: f32) -> f32 {
        match self {
            Transform::None => value,
            Transform::DbToLinear => db_to_linear(value),
        }
    }
}

#[derive(Debug, Clone)]
enum Source {
    Const(f32),
    /// A parameter, read at block rate.
    Param {
        index: usize,
        min: f32,
        span: f32,
        scale: Scale,
    },
    /// Another node's lane, read sample by sample.
    Signal {
        node: usize,
        scale: Scale,
    },
}

/// A number field: where its value comes from and how it eases.
#[derive(Debug, Clone)]
struct Number {
    source: Source,
    transform: Transform,
    /// For a parameter the field eases toward over `smooth` seconds.
    smoother: Option<Smoother>,
    smooth: f32,
    /// The value in effect, for sources without a smoother.
    value: f32,
}

impl Number {
    fn constant(value: f32) -> Self {
        Self {
            source: Source::Const(value),
            transform: Transform::None,
            smoother: None,
            smooth: 0.0,
            value,
        }
    }

    /// The lane this field follows, if it follows one.
    #[inline]
    fn signal(&self) -> Option<usize> {
        match self.source {
            Source::Signal { node, .. } => Some(node),
            _ => None,
        }
    }

    /// The field's value for a sample of the signal it follows.
    #[inline]
    fn map_signal(&self, sample: f32) -> f32 {
        match &self.source {
            Source::Signal { scale, .. } => self.transform.apply(scale.at(sample)),
            _ => self.value,
        }
    }

    /// Where a parameter source stands now.
    fn target(&self, values: &[f32]) -> f32 {
        match &self.source {
            Source::Const(v) => *v,
            Source::Param {
                index,
                min,
                span,
                scale,
            } => {
                let v = values[*index];
                let t = if scale.is_positional() {
                    (v - min) / span
                } else {
                    v
                };
                self.transform.apply(scale.at(t))
            }
            Source::Signal { .. } => self.value,
        }
    }

    /// Lands on the current target, with the smoothing time set for
    /// `sample_rate`. Off the audio thread.
    fn prepare(&mut self, values: &[f32], sample_rate: f32) {
        self.value = self.target(values);
        if let Some(s) = &mut self.smoother {
            s.set_time(self.smooth, sample_rate);
            s.snap(self.value);
        }
    }

    /// Points the field at the parameter's current value. Once per
    /// block, before reading.
    #[inline]
    fn begin(&mut self, values: &[f32]) {
        if let Source::Param { .. } = self.source {
            let t = self.target(values);
            match &mut self.smoother {
                Some(s) => s.set_target(t),
                None => self.value = t,
            }
        }
    }

    /// The value for a whole block of `frames`, for fields read once
    /// per block: the smoother jumps the block in one step.
    #[inline]
    fn block(&mut self, frames: usize) -> f32 {
        match &mut self.smoother {
            Some(s) => s.advance_by(frames),
            None => self.value,
        }
    }

    /// The value for the next sample.
    #[inline]
    fn advance(&mut self) -> f32 {
        match &mut self.smoother {
            Some(s) => s.advance(),
            None => self.value,
        }
    }
}

/// A field that picks one of a kind's options.
#[derive(Debug, Clone)]
enum Choice {
    Const(usize),
    Param {
        index: usize,
        /// The option for each parameter index; the index itself
        /// when empty.
        table: Vec<usize>,
    },
}

impl Choice {
    #[inline]
    fn get(&self, values: &[f32]) -> usize {
        match self {
            Choice::Const(i) => *i,
            Choice::Param { index, table } => {
                let i = values[*index].round().max(0.0) as usize;
                if table.is_empty() {
                    i
                } else {
                    table[i.min(table.len() - 1)]
                }
            }
        }
    }
}

/// A switch field.
#[derive(Debug, Clone)]
enum Flag {
    Const(bool),
    Param { index: usize, table: Vec<bool> },
}

impl Flag {
    #[inline]
    fn get(&self, values: &[f32]) -> bool {
        match self {
            Flag::Const(b) => *b,
            Flag::Param { index, table } => {
                let v = values[*index];
                if table.is_empty() {
                    v >= 0.5
                } else {
                    table[(v.round().max(0.0) as usize).min(table.len() - 1)]
                }
            }
        }
    }
}

// --- node engines ---

/// What a node sees while it processes a block.
struct Ctx<'a> {
    values: &'a [f32],
    /// The lanes of every node before this one.
    before: &'a [Lane],
    /// This node's audio input, summed if it has several.
    input: &'a Lane,
    channels: usize,
    frames: usize,
    sample_rate: f32,
    transport: &'a Transport,
}

struct GainNode {
    amount: Number,
}

impl GainNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        let frames = cx.frames;
        if let Some(k) = self.amount.signal() {
            for ch in 0..cx.channels {
                let sig = &cx.before[k][ch][..frames];
                let input = &cx.input[ch][..frames];
                for ((o, x), s) in out[ch][..frames].iter_mut().zip(input).zip(sig) {
                    *o = x * self.amount.map_signal(*s);
                }
            }
            return;
        }
        self.amount.begin(cx.values);
        for frame in 0..frames {
            let g = self.amount.advance();
            for ch in 0..cx.channels {
                out[ch][frame] = cx.input[ch][frame] * g;
            }
        }
    }
}

struct BiquadNode {
    kind: Choice,
    freq: Number,
    gain: Number,
    q: Number,
    filters: [Biquad; MAX_CHANNELS],
    /// What the coefficients were last designed for.
    designed: Option<(usize, f32, f32, f32)>,
}

fn design(kind: usize, sample_rate: f32, hz: f32, gain: f32, q: f32) -> Biquad {
    let mut f = Biquad::identity();
    match kind {
        0 => f.set_lowpass(sample_rate, hz, q),
        1 => f.set_highpass(sample_rate, hz, q),
        2 => f.set_peaking(sample_rate, hz, gain, q),
        3 => f.set_lowshelf(sample_rate, hz, gain, q),
        4 => f.set_highshelf(sample_rate, hz, gain, q),
        _ => f.set_notch(sample_rate, hz, q),
    }
    f
}

impl BiquadNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        let frames = cx.frames;
        self.freq.begin(cx.values);
        self.gain.begin(cx.values);
        self.q.begin(cx.values);
        let kind = self.kind.get(cx.values);
        let gain = self.gain.block(frames);
        let q = self.q.block(frames);
        if let Some(k) = self.freq.signal() {
            // Redesigned every sample: a swept filter.
            for ch in 0..cx.channels {
                let sig = &cx.before[k][ch][..frames];
                let input = &cx.input[ch][..frames];
                let filter = &mut self.filters[ch];
                for ((o, x), s) in out[ch][..frames].iter_mut().zip(input).zip(sig) {
                    let hz = self.freq.map_signal(*s);
                    filter.set_coefficients_from(&design(kind, cx.sample_rate, hz, gain, q));
                    *o = filter.tick(*x);
                }
            }
            self.designed = None;
            return;
        }
        let hz = self.freq.block(frames);
        if self.designed != Some((kind, hz, gain, q)) {
            let d = design(kind, cx.sample_rate, hz, gain, q);
            for f in &mut self.filters {
                f.set_coefficients_from(&d);
            }
            self.designed = Some((kind, hz, gain, q));
        }
        for ch in 0..cx.channels {
            let filter = &mut self.filters[ch];
            for (o, x) in out[ch][..frames].iter_mut().zip(&cx.input[ch][..frames]) {
                *o = filter.tick(*x);
            }
        }
    }
}

/// The oversampling factor of a waveshaper.
#[derive(Debug, Clone)]
enum Factor {
    Const(usize),
    Param { index: usize, table: Vec<usize> },
}

impl Factor {
    fn get(&self, values: &[f32]) -> usize {
        match self {
            Factor::Const(f) => *f,
            Factor::Param { index, table } => {
                let i = (values[*index].round().max(0.0) as usize).min(table.len() - 1);
                table[i]
            }
        }
    }
}

struct ShaperNode {
    curve: Choice,
    factor: Factor,
    /// None when the factor is fixed at 1: no latency, no filters.
    oversamplers: Option<[Oversampler; MAX_CHANNELS]>,
    current: usize,
}

const CURVE_LIST: [Curve; 7] = [
    Curve::Soft,
    Curve::Hard,
    Curve::Asymmetric,
    Curve::Fold,
    Curve::Silicon,
    Curve::Germanium,
    Curve::Octave,
];

impl ShaperNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        let frames = cx.frames;
        let curve = CURVE_LIST[self.curve.get(cx.values).min(CURVE_LIST.len() - 1)];
        for ch in 0..cx.channels {
            out[ch][..frames].copy_from_slice(&cx.input[ch][..frames]);
        }
        match &mut self.oversamplers {
            Some(oversamplers) => {
                let factor = self.factor.get(cx.values);
                if factor != self.current {
                    self.current = factor;
                    for os in oversamplers.iter_mut() {
                        os.set_factor(factor);
                    }
                }
                for ch in 0..cx.channels {
                    oversamplers[ch].process(&mut out[ch][..frames], |up| {
                        for s in up.iter_mut() {
                            *s = curve.shape(*s);
                        }
                    });
                }
            }
            None => {
                for ch in 0..cx.channels {
                    for s in &mut out[ch][..frames] {
                        *s = curve.shape(*s);
                    }
                }
            }
        }
    }
}

struct DelayNode {
    time: Number,
    feedback: Number,
    mix: Number,
    max_ms: f32,
    lines: [DelayLine; MAX_CHANNELS],
}

impl DelayNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        self.time.begin(cx.values);
        self.feedback.begin(cx.values);
        self.mix.begin(cx.values);
        let per_ms = cx.sample_rate / 1000.0;
        let signal = self.time.signal();
        for frame in 0..cx.frames {
            let feedback = self.feedback.advance();
            let mix = self.mix.advance();
            let ms = self.time.advance();
            for ch in 0..cx.channels {
                let ms = match signal {
                    Some(k) => self.time.map_signal(cx.before[k][ch][frame]),
                    None => ms,
                };
                let delay = ms * per_ms;
                let x = cx.input[ch][frame];
                // Read before the push, so the newest sample in the
                // line is the previous frame's: one less to go back.
                let wet = self.lines[ch].read(delay - 1.0);
                self.lines[ch].push(x + wet * feedback);
                out[ch][frame] = x * (1.0 - mix) + wet * mix;
            }
        }
    }
}

struct LfoNode {
    rate: Number,
    shape: Choice,
    sync: Flag,
    note: Choice,
    depth: Number,
    phase: Number,
    spread: Number,
    unipolar: Flag,
    lfo: Lfo,
}

impl LfoNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        let frames = cx.frames;
        self.rate.begin(cx.values);
        self.depth.begin(cx.values);
        self.phase.begin(cx.values);
        self.spread.begin(cx.values);
        self.lfo
            .set_shape(Shape::from_index(self.shape.get(cx.values)));
        if self.sync.get(cx.values) {
            self.lfo
                .sync(cx.transport, note_beats(self.note.get(cx.values)));
        } else {
            self.lfo.set_rate_hz(self.rate.block(frames));
        }
        let phase = self.phase.block(frames);
        let spread = self.spread.block(frames);
        let unipolar = self.unipolar.get(cx.values);
        for frame in 0..frames {
            let depth = self.depth.advance();
            for ch in 0..cx.channels {
                let offset = if ch == 0 { phase } else { phase + spread };
                let mut v = self.lfo.value(offset);
                if unipolar {
                    v = (v + 1.0) * 0.5;
                }
                out[ch][frame] = v * depth;
            }
            self.lfo.advance();
        }
    }
}

struct AllpassNode {
    stages: usize,
    freq: Number,
    feedback: Number,
    mix: Number,
    filters: [[Allpass; MAX_STAGES]; MAX_CHANNELS],
    last: [f32; MAX_CHANNELS],
    /// The frequency the stages are set to, when not following a signal.
    hz: f32,
}

impl AllpassNode {
    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        self.freq.begin(cx.values);
        self.feedback.begin(cx.values);
        self.mix.begin(cx.values);
        let signal = self.freq.signal();
        if signal.is_none() {
            let hz = self.freq.block(cx.frames);
            if hz != self.hz {
                self.hz = hz;
                for stages in &mut self.filters {
                    for stage in &mut stages[..self.stages] {
                        stage.set_frequency(hz, cx.sample_rate);
                    }
                }
            }
        }
        for frame in 0..cx.frames {
            let feedback = self.feedback.advance();
            let mix = self.mix.advance();
            for ch in 0..cx.channels {
                let x = cx.input[ch][frame];
                let mut y = x + self.last[ch] * feedback;
                let stages = &mut self.filters[ch][..self.stages];
                if let Some(k) = signal {
                    let hz = self.freq.map_signal(cx.before[k][ch][frame]);
                    for stage in stages.iter_mut() {
                        stage.set_frequency(hz, cx.sample_rate);
                        y = stage.tick(y);
                    }
                } else {
                    for stage in stages.iter_mut() {
                        y = stage.tick(y);
                    }
                }
                self.last[ch] = y;
                out[ch][frame] = x * (1.0 - mix) + y * mix;
            }
        }
    }
}

enum Engine {
    /// Its lane is filled before the walk; nothing to do.
    Input,
    /// Copies its input: the output, the split and the mix (whose
    /// input is already the sum).
    Copy,
    Gain(GainNode),
    Biquad(BiquadNode),
    Shaper(ShaperNode),
    Delay(DelayNode),
    Lfo(LfoNode),
    Allpass(AllpassNode),
}

impl Engine {
    fn latency(&self) -> usize {
        match self {
            Engine::Shaper(s) if s.oversamplers.is_some() => OVERSAMPLE_LATENCY,
            _ => 0,
        }
    }

    /// Reserves buffers and lands every field on its target.
    fn prepare(&mut self, values: &[f32], sample_rate: f32, max_block: usize) {
        match self {
            Engine::Input | Engine::Copy => {}
            Engine::Gain(n) => n.amount.prepare(values, sample_rate),
            Engine::Biquad(n) => {
                n.freq.prepare(values, sample_rate);
                n.gain.prepare(values, sample_rate);
                n.q.prepare(values, sample_rate);
                n.designed = None;
            }
            Engine::Shaper(n) => {
                if let Some(os) = &mut n.oversamplers {
                    for o in os.iter_mut() {
                        o.prepare(max_block);
                    }
                }
            }
            Engine::Delay(n) => {
                n.time.prepare(values, sample_rate);
                n.feedback.prepare(values, sample_rate);
                n.mix.prepare(values, sample_rate);
                let longest = (n.max_ms / 1000.0 * sample_rate) as usize + 2;
                n.lines = std::array::from_fn(|_| DelayLine::new(longest));
            }
            Engine::Lfo(n) => {
                n.rate.prepare(values, sample_rate);
                n.depth.prepare(values, sample_rate);
                n.phase.prepare(values, sample_rate);
                n.spread.prepare(values, sample_rate);
                n.lfo.prepare(sample_rate);
            }
            Engine::Allpass(n) => {
                n.freq.prepare(values, sample_rate);
                n.feedback.prepare(values, sample_rate);
                n.mix.prepare(values, sample_rate);
                n.hz = f32::NAN;
            }
        }
    }

    /// Clears memory without touching fields.
    fn reset(&mut self) {
        match self {
            Engine::Input | Engine::Copy | Engine::Gain(_) => {}
            Engine::Biquad(n) => {
                for f in &mut n.filters {
                    f.reset();
                }
            }
            Engine::Shaper(n) => {
                if let Some(os) = &mut n.oversamplers {
                    for o in os.iter_mut() {
                        o.reset();
                    }
                }
            }
            Engine::Delay(n) => {
                for line in &mut n.lines {
                    line.reset();
                }
            }
            Engine::Lfo(n) => n.lfo.reset(),
            Engine::Allpass(n) => {
                n.filters = [[Allpass::default(); MAX_STAGES]; MAX_CHANNELS];
                n.last = [0.0; MAX_CHANNELS];
            }
        }
    }

    fn process(&mut self, cx: &Ctx, out: &mut Lane) {
        match self {
            Engine::Input => {}
            Engine::Copy => {
                for ch in 0..cx.channels {
                    out[ch][..cx.frames].copy_from_slice(&cx.input[ch][..cx.frames]);
                }
            }
            Engine::Gain(n) => n.process(cx, out),
            Engine::Biquad(n) => n.process(cx, out),
            Engine::Shaper(n) => n.process(cx, out),
            Engine::Delay(n) => n.process(cx, out),
            Engine::Lfo(n) => n.process(cx, out),
            Engine::Allpass(n) => n.process(cx, out),
        }
    }
}

/// Where a node's audio comes from, as indices into the ordered nodes.
enum Input {
    None,
    One(usize),
    Many(Vec<usize>),
}

struct Node {
    engine: Engine,
    input: Input,
}

/// A compiled pedal. See the module docs.
pub struct PedalProcessor {
    def: PedalDef,
    params: Vec<ParamInfo>,
    values: Vec<f32>,
    /// In an order where everything a node reads comes before it.
    nodes: Vec<Node>,
    lanes: Vec<Lane>,
    /// The sum of a mix node's inputs.
    scratch: Lane,
    input: usize,
    output: usize,
    latency: usize,
    transport: Transport,
    sample_rate: f32,
    max_block: usize,
}

impl PedalProcessor {
    /// The definition this was built from.
    pub fn definition(&self) -> &PedalDef {
        &self.def
    }
}

impl Processor for PedalProcessor {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn param(&self, id: ParamId) -> f32 {
        self.values.get(id).copied().unwrap_or(0.0)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if let Some(p) = self.params.get(id) {
            self.values[id] = p.clamp(value);
        }
    }

    fn latency(&self) -> usize {
        self.latency
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        self.max_block = max_block_frames;
        self.lanes = (0..self.nodes.len())
            .map(|_| std::array::from_fn(|_| vec![0.0; max_block_frames]))
            .collect();
        self.scratch = std::array::from_fn(|_| vec![0.0; max_block_frames]);
        for node in &mut self.nodes {
            node.engine
                .prepare(&self.values, self.sample_rate, max_block_frames);
        }
        self.reset();
    }

    fn reset(&mut self) {
        for node in &mut self.nodes {
            node.engine.reset();
        }
    }

    fn set_transport(&mut self, transport: &Transport) {
        self.transport = *transport;
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames == 0 || frames > self.max_block {
            // Unprepared, or a block larger than prepared for: bypass.
            return;
        }
        let channels = buffer.channels().min(MAX_CHANNELS);
        for ch in 0..channels {
            self.lanes[self.input][ch][..frames].copy_from_slice(buffer.channel(ch));
        }
        let Self {
            nodes,
            lanes,
            scratch,
            values,
            transport,
            sample_rate,
            ..
        } = self;
        for (i, node) in nodes.iter_mut().enumerate() {
            let (before, rest) = lanes.split_at_mut(i);
            let out = &mut rest[0];
            let input: &Lane = match &node.input {
                Input::None => &*scratch,
                Input::One(j) => &before[*j],
                Input::Many(sources) => {
                    for ch in 0..channels {
                        let sum = &mut scratch[ch][..frames];
                        sum.fill(0.0);
                        for j in sources {
                            for (s, x) in sum.iter_mut().zip(&before[*j][ch][..frames]) {
                                *s += x;
                            }
                        }
                    }
                    &*scratch
                }
            };
            let cx = Ctx {
                values,
                before,
                input,
                channels,
                frames,
                sample_rate: *sample_rate,
                transport,
            };
            node.engine.process(&cx, out);
        }
        for ch in 0..channels {
            buffer
                .channel_mut(ch)
                .copy_from_slice(&self.lanes[self.output][ch][..frames]);
        }
    }
}

// --- building ---

/// Choice names handed to `Unit::Choice`, which wants them `'static`.
/// Each distinct list is leaked once and shared after that, so a
/// designer recompiling a pedal a thousand times leaks nothing new.
fn intern(names: &[String]) -> &'static [&'static str] {
    static POOL: OnceLock<Mutex<Vec<&'static [&'static str]>>> = OnceLock::new();
    let mut pool = POOL
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(found) = pool
        .iter()
        .find(|list| list.len() == names.len() && list.iter().zip(names).all(|(a, b)| a == b))
    {
        return found;
    }
    let leaked: Vec<&'static str> = names
        .iter()
        .map(|n| &*Box::leak(n.clone().into_boxed_str()))
        .collect();
    let list: &'static [&'static str] = Box::leak(leaked.into_boxed_slice());
    pool.push(list);
    list
}

fn unit_for(name: &str) -> Option<Unit> {
    Some(match name {
        "" => Unit::Generic,
        "dB" => Unit::Decibels,
        "Hz" => Unit::Hertz,
        "s" => Unit::Seconds,
        "ms" => Unit::Milliseconds,
        "%" => Unit::Percent,
        _ => return None,
    })
}

fn param_info(p: &ParamDef, problems: &mut Vec<Problem>) -> ParamInfo {
    let label = if p.label.is_empty() {
        p.id.clone()
    } else {
        p.label.clone()
    };
    let bad = |message: String| Problem::pedal(format!("parameter {:?}: {message}", p.id));
    if !p.choices.is_empty() && p.toggle {
        problems.push(bad("is both a choice and a toggle".into()));
    }
    let (unit, min, max) = if !p.choices.is_empty() {
        if p.choices.iter().any(|c| c.is_empty()) {
            problems.push(bad("has an empty choice".into()));
        }
        (
            Unit::Choice(intern(&p.choices)),
            0.0,
            (p.choices.len() - 1) as f32,
        )
    } else if p.toggle {
        (Unit::Toggle, 0.0, 1.0)
    } else {
        let unit = unit_for(&p.unit).unwrap_or_else(|| {
            problems.push(bad(format!(
                "unit {:?} is not one of dB, Hz, s, ms or %",
                p.unit
            )));
            Unit::Generic
        });
        if !(p.min.is_finite() && p.max.is_finite() && p.min < p.max) {
            problems.push(bad(format!("min {} must be below max {}", p.min, p.max)));
        }
        (unit, p.min, p.max)
    };
    if !(p.default.is_finite() && p.default >= min && p.default <= max) {
        problems.push(bad(format!(
            "default {} is outside {min} to {max}",
            p.default
        )));
    }
    ParamInfo {
        name: Cow::Owned(label),
        unit,
        min,
        max,
        default: p.default.clamp(min, max.max(min)),
    }
}

/// Reads a node's fields against its kind, recording problems as it
/// goes and remembering which fields were read so the rest can be
/// reported as unknown.
struct Fields<'a> {
    node: &'a NodeDef,
    kind: &'static KindInfo,
    params: &'a [ParamDef],
    param_index: &'a HashMap<String, usize>,
    node_index: &'a HashMap<String, usize>,
    defaults: &'a [f32],
    problems: &'a mut Vec<Problem>,
    read: HashSet<&'static str>,
    smooth: f32,
}

impl<'a> Fields<'a> {
    fn new(
        node: &'a NodeDef,
        kind: &'static KindInfo,
        params: &'a [ParamDef],
        param_index: &'a HashMap<String, usize>,
        node_index: &'a HashMap<String, usize>,
        defaults: &'a [f32],
        problems: &'a mut Vec<Problem>,
    ) -> Self {
        let mut fields = Self {
            node,
            kind,
            params,
            param_index,
            node_index,
            defaults,
            problems,
            read: HashSet::new(),
            smooth: 0.0,
        };
        if kind.field("smooth").is_some() {
            fields.smooth = fields.constant("smooth");
        }
        fields
    }

    fn info(&mut self, name: &'static str) -> &'static kinds::FieldInfo {
        self.read.insert(name);
        self.kind
            .field(name)
            .unwrap_or_else(|| panic!("{} has no field {name}", self.kind.kind))
    }

    fn problem(&mut self, field: &str, message: impl Into<String>) {
        self.problems
            .push(Problem::field(&self.node.id, field, message));
    }

    /// The parameter a binding names, if it exists.
    fn param_of(&mut self, field: &str, binding: &Binding) -> Option<usize> {
        match (&binding.param, &binding.node) {
            (Some(p), None) => match self.param_index.get(p) {
                Some(i) => Some(*i),
                None => {
                    self.problem(field, format!("no parameter {p:?}"));
                    None
                }
            },
            (None, Some(_)) => None,
            (Some(_), Some(_)) => {
                self.problem(field, "a binding follows a param or a node, not both");
                None
            }
            (None, None) => {
                self.problem(field, "a binding names a param or a node");
                None
            }
        }
    }

    /// Marks a field read without parsing it.
    fn skip(&mut self, name: &'static str) {
        self.read.insert(name);
    }

    /// A number field that may be bound.
    fn number(&mut self, name: &'static str) -> Number {
        self.number_with(name, Transform::None)
    }

    fn number_with(&mut self, name: &'static str, transform: Transform) -> Number {
        let info = self.info(name);
        let FieldKind::Number {
            min, max, default, ..
        } = info.kind
        else {
            unreachable!("{name} is a number");
        };
        let range = format!("{name} takes {min} to {max}");
        let mut number = Number::constant(transform.apply(default));
        let Some(value) = self.node.fields.get(name) else {
            return number;
        };
        match value {
            Value::Number(n) => {
                let n = *n as f32;
                if !(n.is_finite() && n >= min && n <= max) {
                    self.problem(name, format!("{n} is out of range; {range}"));
                } else {
                    number = Number::constant(transform.apply(n));
                }
            }
            Value::Bool(_) | Value::Name(_) => {
                self.problem(name, format!("takes a number; {range}"));
            }
            Value::Bound(binding) => {
                let scale = self.scale(name, binding, min, max);
                if let Some(index) = self.param_of(name, binding) {
                    let p = &self.params[index];
                    let (pmin, pmax) = param_range(p);
                    if matches!(scale, Scale::Raw) && (pmin < min || pmax > max) {
                        self.problem(
                            name,
                            format!(
                                "parameter {:?} runs {pmin} to {pmax} but {range}; add a map",
                                p.id
                            ),
                        );
                    }
                    number = Number {
                        source: Source::Param {
                            index,
                            min: pmin,
                            span: (pmax - pmin).max(f32::MIN_POSITIVE),
                            scale,
                        },
                        transform,
                        smoother: None,
                        smooth: 0.0,
                        value: 0.0,
                    };
                    if info.smoothed && self.smooth > 0.0 {
                        number.smooth = self.smooth;
                        number.smoother = Some(Smoother::new(number.target(self.defaults)));
                    }
                } else if let Some(node) = &binding.node {
                    if !info.modulatable {
                        self.problem(name, "cannot follow a signal");
                    } else if !binding.values.is_empty() {
                        self.problem(name, "a signal binding takes a map, not values");
                    } else if node == &self.node.id {
                        self.problem(name, "a node cannot follow itself");
                    } else if let Some(k) = self.node_index.get(node) {
                        number = Number {
                            source: Source::Signal { node: *k, scale },
                            transform,
                            smoother: None,
                            smooth: 0.0,
                            value: 0.0,
                        };
                    } else {
                        self.problem(name, format!("no node {node:?}"));
                    }
                }
            }
        }
        number
    }

    /// The scale a binding asks for, checked against the field's range.
    fn scale(&mut self, name: &str, binding: &Binding, min: f32, max: f32) -> Scale {
        if !binding.values.is_empty() {
            if binding.map.is_some() || binding.log {
                self.problem(name, "values and a map do not go together");
            }
            let mut table = Vec::with_capacity(binding.values.len());
            for v in &binding.values {
                match v {
                    Value::Number(n) if (*n as f32).is_finite() => {
                        let n = *n as f32;
                        if n < min || n > max {
                            self.problem(name, format!("value {n} is out of range {min} to {max}"));
                        }
                        table.push(n);
                    }
                    _ => {
                        self.problem(name, "values must be numbers here");
                        table.push(min);
                    }
                }
            }
            return Scale::Table(table);
        }
        match binding.map {
            Some([lo, hi]) => {
                let (lo, hi) = (lo.0 as f32, hi.0 as f32);
                for end in [lo, hi] {
                    if !(end.is_finite() && end >= min && end <= max) {
                        self.problem(
                            name,
                            format!("map end {end} is out of range {min} to {max}"),
                        );
                    }
                }
                if binding.log {
                    if lo <= 0.0 || hi <= 0.0 {
                        self.problem(name, "a log map needs ends above zero");
                        Scale::Linear { lo, hi }
                    } else {
                        Scale::Log { lo, ratio: hi / lo }
                    }
                } else {
                    Scale::Linear { lo, hi }
                }
            }
            None => {
                if binding.log {
                    self.problem(name, "log needs a map");
                }
                Scale::Raw
            }
        }
    }

    /// A number field that must be a constant.
    fn constant(&mut self, name: &'static str) -> f32 {
        let info = self.info(name);
        let FieldKind::Number {
            min, max, default, ..
        } = info.kind
        else {
            unreachable!("{name} is a number");
        };
        match self.node.fields.get(name) {
            None => default,
            Some(Value::Number(n)) => {
                let n = *n as f32;
                if n.is_finite() && n >= min && n <= max {
                    n
                } else {
                    self.problem(name, format!("{n} is out of range; takes {min} to {max}"));
                    default
                }
            }
            Some(_) => {
                self.problem(name, format!("takes a plain number from {min} to {max}"));
                default
            }
        }
    }

    /// A pick from the field's options, fixed or following a choice
    /// parameter.
    fn choice(&mut self, name: &'static str) -> Choice {
        let info = self.info(name);
        let FieldKind::Name { options, default } = info.kind else {
            unreachable!("{name} is a name");
        };
        let list = options.join(", ");
        let Some(value) = self.node.fields.get(name) else {
            return Choice::Const(default);
        };
        match value {
            Value::Name(n) => match options.iter().position(|o| o == n) {
                Some(i) => Choice::Const(i),
                None => {
                    self.problem(name, format!("{n:?} is not one of {list}"));
                    Choice::Const(default)
                }
            },
            Value::Number(_) | Value::Bool(_) => {
                self.problem(name, format!("takes one of {list}"));
                Choice::Const(default)
            }
            Value::Bound(binding) => {
                if binding.node.is_some() && binding.param.is_none() {
                    self.problem(name, "cannot follow a signal; bind a parameter");
                    return Choice::Const(default);
                }
                let Some(index) = self.param_of(name, binding) else {
                    return Choice::Const(default);
                };
                if binding.map.is_some() || binding.log {
                    self.problem(name, "a choice takes values, not a map");
                }
                let mut table = Vec::with_capacity(binding.values.len());
                for v in &binding.values {
                    match v {
                        Value::Name(n) => match options.iter().position(|o| o == n) {
                            Some(i) => table.push(i),
                            None => {
                                self.problem(name, format!("value {n:?} is not one of {list}"));
                                table.push(default);
                            }
                        },
                        _ => {
                            self.problem(name, format!("values must be names from {list}"));
                            table.push(default);
                        }
                    }
                }
                let (_, pmax) = param_range(&self.params[index]);
                if table.is_empty() && pmax as usize >= options.len() {
                    self.problem(
                        name,
                        format!(
                            "parameter {:?} runs past the {} options; add values",
                            self.params[index].id,
                            options.len()
                        ),
                    );
                }
                Choice::Param { index, table }
            }
        }
    }

    /// A switch, fixed or following a parameter.
    fn flag(&mut self, name: &'static str) -> Flag {
        let info = self.info(name);
        let FieldKind::Bool { default } = info.kind else {
            unreachable!("{name} is a flag");
        };
        let Some(value) = self.node.fields.get(name) else {
            return Flag::Const(default);
        };
        match value {
            Value::Bool(b) => Flag::Const(*b),
            Value::Number(n) if *n == 0.0 || *n == 1.0 => Flag::Const(*n == 1.0),
            Value::Number(_) | Value::Name(_) => {
                self.problem(name, "takes true or false");
                Flag::Const(default)
            }
            Value::Bound(binding) => {
                if binding.node.is_some() && binding.param.is_none() {
                    self.problem(name, "cannot follow a signal; bind a parameter");
                    return Flag::Const(default);
                }
                let Some(index) = self.param_of(name, binding) else {
                    return Flag::Const(default);
                };
                if binding.map.is_some() || binding.log {
                    self.problem(name, "a switch takes values, not a map");
                }
                let mut table = Vec::with_capacity(binding.values.len());
                for v in &binding.values {
                    match v {
                        Value::Bool(b) => table.push(*b),
                        Value::Number(n) if *n == 0.0 || *n == 1.0 => table.push(*n == 1.0),
                        _ => {
                            self.problem(name, "values must be true or false");
                            table.push(default);
                        }
                    }
                }
                Flag::Param { index, table }
            }
        }
    }

    /// Reports every field the kind does not have.
    fn finish(self) {
        for name in self.node.fields.keys() {
            if !self.read.contains(name.as_str()) {
                self.problems.push(Problem::field(
                    &self.node.id,
                    name,
                    format!("no such field on a {}", self.kind.kind),
                ));
            }
        }
    }
}

fn param_range(p: &ParamDef) -> (f32, f32) {
    if !p.choices.is_empty() {
        (0.0, (p.choices.len() - 1) as f32)
    } else if p.toggle {
        (0.0, 1.0)
    } else {
        (p.min, p.max)
    }
}

fn build_engine(fields: &mut Fields) -> Engine {
    match fields.kind.kind {
        "input" => Engine::Input,
        "output" | "mix" | "split" => Engine::Copy,
        "gain" => {
            let has_db = fields.node.fields.contains_key("db");
            let has_linear = fields.node.fields.contains_key("linear");
            if has_db && has_linear {
                fields.problem("linear", "set db or linear, not both");
            }
            let amount = if has_linear {
                fields.skip("db");
                fields.number("linear")
            } else {
                fields.skip("linear");
                fields.number_with("db", Transform::DbToLinear)
            };
            Engine::Gain(GainNode { amount })
        }
        "biquad" => Engine::Biquad(BiquadNode {
            kind: fields.choice("type"),
            freq: fields.number("freq"),
            gain: fields.number("gain"),
            q: fields.number("q"),
            filters: [Biquad::identity(); MAX_CHANNELS],
            designed: None,
        }),
        "waveshaper" => {
            let curve = fields.choice("curve");
            let factor = fields.number("oversample");
            let valid = |f: f32| f == 1.0 || f == 2.0 || f == 4.0 || f == 8.0;
            let factor = match factor.source {
                Source::Const(f) => {
                    if !valid(f) {
                        fields.problem("oversample", "takes 1, 2, 4 or 8");
                    }
                    Factor::Const(f as usize)
                }
                Source::Param {
                    index,
                    scale: Scale::Table(table),
                    ..
                } => {
                    if !table.iter().all(|f| valid(*f)) {
                        fields.problem("oversample", "values must each be 1, 2, 4 or 8");
                    }
                    Factor::Param {
                        index,
                        table: table.iter().map(|f| *f as usize).collect(),
                    }
                }
                Source::Param { .. } => {
                    fields.problem(
                        "oversample",
                        "takes 1, 2, 4 or 8; bind it to a choice with values",
                    );
                    Factor::Const(4)
                }
                Source::Signal { .. } => Factor::Const(4),
            };
            let oversamplers = match factor {
                Factor::Const(1) => None,
                Factor::Const(f) => Some(std::array::from_fn(|_| Oversampler::new(f))),
                Factor::Param { .. } => Some(std::array::from_fn(|_| Oversampler::new(4))),
            };
            let current = match &factor {
                Factor::Const(f) => *f,
                Factor::Param { .. } => 4,
            };
            Engine::Shaper(ShaperNode {
                curve,
                factor,
                oversamplers,
                current,
            })
        }
        "delay" => Engine::Delay(DelayNode {
            time: fields.number("time"),
            feedback: fields.number("feedback"),
            mix: fields.number("mix"),
            max_ms: fields.constant("max"),
            lines: std::array::from_fn(|_| DelayLine::new(2)),
        }),
        "lfo" => Engine::Lfo(LfoNode {
            rate: fields.number("rate"),
            shape: fields.choice("shape"),
            sync: fields.flag("sync"),
            note: fields.choice("note"),
            depth: fields.number("depth"),
            phase: fields.number("phase"),
            spread: fields.number("spread"),
            unipolar: fields.flag("unipolar"),
            lfo: Lfo::new(),
        }),
        "allpass" => Engine::Allpass(AllpassNode {
            stages: fields.constant("stages").round() as usize,
            freq: fields.number("freq"),
            feedback: fields.number("feedback"),
            mix: fields.number("mix"),
            filters: [[Allpass::default(); MAX_STAGES]; MAX_CHANNELS],
            last: [0.0; MAX_CHANNELS],
            hz: f32::NAN,
        }),
        other => unreachable!("kind {other} is in the catalog but not built"),
    }
}

/// The nodes a field binding of `node` reads.
fn signal_sources(node: &NodeDef) -> impl Iterator<Item = &str> {
    node.fields.values().filter_map(|v| match v {
        Value::Bound(b) if b.param.is_none() => b.node.as_deref(),
        _ => None,
    })
}

fn build(def: &PedalDef) -> Result<PedalProcessor, PedalError> {
    let mut problems = Vec::new();

    // The pedal.
    if def.version != PEDAL_VERSION {
        problems.push(Problem::pedal(format!(
            "version {} is not the version this build reads ({PEDAL_VERSION})",
            def.version
        )));
    }
    if def.name.trim().is_empty() {
        problems.push(Problem::pedal("the pedal has no name"));
    }

    // Parameters.
    if def.params.len() > MAX_PARAMS {
        problems.push(Problem::pedal(format!(
            "{} parameters; at most {MAX_PARAMS}",
            def.params.len()
        )));
    }
    let mut param_index = HashMap::new();
    let mut params = Vec::with_capacity(def.params.len());
    for (i, p) in def.params.iter().enumerate() {
        if p.id.is_empty() {
            problems.push(Problem::pedal(format!("parameter {i} has no id")));
        } else if param_index.insert(p.id.clone(), i).is_some() {
            problems.push(Problem::pedal(format!(
                "two parameters are called {:?}",
                p.id
            )));
        }
        params.push(param_info(p, &mut problems));
    }
    let defaults: Vec<f32> = params.iter().map(|p| p.default).collect();

    // Nodes: ids and kinds.
    if def.nodes.len() > MAX_NODES {
        problems.push(Problem::pedal(format!(
            "{} nodes; at most {MAX_NODES}",
            def.nodes.len()
        )));
    }
    let mut node_index: HashMap<String, usize> = HashMap::new();
    let mut kinds_of: Vec<Option<&'static KindInfo>> = Vec::with_capacity(def.nodes.len());
    for (i, n) in def.nodes.iter().enumerate() {
        if n.id.is_empty() {
            problems.push(Problem::pedal(format!("node {i} has no id")));
        } else if node_index.insert(n.id.clone(), i).is_some() {
            problems.push(Problem::node(&n.id, "two nodes have this id"));
        }
        let kind = kinds::kind(&n.kind);
        if kind.is_none() {
            problems.push(Problem::node(&n.id, format!("unknown kind {:?}", n.kind)));
        }
        kinds_of.push(kind);
    }
    let count = |name: &str| kinds_of.iter().flatten().filter(|k| k.kind == name).count();
    for name in ["input", "output"] {
        match count(name) {
            1 => {}
            0 => problems.push(Problem::pedal(format!("there is no {name} node"))),
            n => problems.push(Problem::pedal(format!(
                "there are {n} {name} nodes; a pedal has one"
            ))),
        }
    }

    // Edges.
    let mut inputs: Vec<Vec<usize>> = vec![Vec::new(); def.nodes.len()];
    let mut feeds: Vec<bool> = vec![false; def.nodes.len()];
    let mut seen_edges = HashSet::new();
    for (from, to) in &def.edges {
        let ends = [from, to].map(|id| match node_index.get(id) {
            Some(i) => Some(*i),
            None => {
                problems.push(Problem::pedal(format!(
                    "edge {from:?} to {to:?}: no node {id:?}"
                )));
                None
            }
        });
        let (Some(a), Some(b)) = (ends[0], ends[1]) else {
            continue;
        };
        if !seen_edges.insert((a, b)) {
            problems.push(Problem::node(from, format!("is wired to {to:?} twice")));
            continue;
        }
        if let Some(k) = kinds_of[a] {
            if k.kind == "output" {
                problems.push(Problem::node(from, "the output feeds nothing further"));
                continue;
            }
            if k.control {
                problems.push(Problem::node(
                    from,
                    format!("an {} drives fields, not audio; bind a field to it", k.kind),
                ));
                continue;
            }
        }
        if let Some(k) = kinds_of[b] {
            if k.source {
                problems.push(Problem::node(to, format!("the {} takes no input", k.kind)));
                continue;
            }
        }
        if a == b {
            problems.push(Problem::node(from, "is wired to itself"));
            continue;
        }
        inputs[b].push(a);
        feeds[a] = true;
    }
    for (i, n) in def.nodes.iter().enumerate() {
        let Some(kind) = kinds_of[i] else {
            continue;
        };
        if !kind.sums && inputs[i].len() > 1 {
            problems.push(Problem::node(
                &n.id,
                "has more than one input; feed them into a mix",
            ));
        }
        if !kind.source && inputs[i].is_empty() {
            problems.push(Problem::node(&n.id, "has no input"));
        }
    }

    // Fields, which may also read other nodes' signals.
    let mut engines: Vec<Option<Engine>> = Vec::with_capacity(def.nodes.len());
    for (i, n) in def.nodes.iter().enumerate() {
        let Some(kind) = kinds_of[i] else {
            engines.push(None);
            continue;
        };
        let mut fields = Fields::new(
            n,
            kind,
            &def.params,
            &param_index,
            &node_index,
            &defaults,
            &mut problems,
        );
        let engine = build_engine(&mut fields);
        fields.finish();
        engines.push(Some(engine));
        for source in signal_sources(n) {
            if let Some(k) = node_index.get(source) {
                feeds[*k] = true;
            }
        }
    }
    for (i, n) in def.nodes.iter().enumerate() {
        if let Some(kind) = kinds_of[i] {
            if kind.kind != "output" && !feeds[i] {
                problems.push(Problem::node(&n.id, "feeds nothing"));
            }
        }
    }

    // Order: everything a node reads comes before it. Kahn's algorithm;
    // whatever is left over is in a cycle.
    let n = def.nodes.len();
    let mut deps: Vec<Vec<usize>> = inputs.clone();
    for (i, node) in def.nodes.iter().enumerate() {
        for source in signal_sources(node) {
            if let Some(k) = node_index.get(source) {
                deps[i].push(*k);
            }
        }
    }
    let mut remaining: Vec<usize> = deps.iter().map(|d| d.len()).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, d) in deps.iter().enumerate() {
        for k in d {
            dependents[*k].push(i);
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).filter(|i| remaining[*i] == 0).collect();
    ready.reverse();
    while let Some(i) = ready.pop() {
        order.push(i);
        for &d in &dependents[i] {
            remaining[d] -= 1;
            if remaining[d] == 0 {
                ready.push(d);
            }
        }
    }
    if order.len() < n {
        let stuck: Vec<&str> = (0..n)
            .filter(|i| remaining[*i] > 0)
            .map(|i| def.nodes[i].id.as_str())
            .collect();
        problems.push(Problem {
            node: stuck.first().map(|s| s.to_string()),
            field: None,
            message: format!(
                "a cycle runs through {}",
                stuck
                    .iter()
                    .map(|s| format!("{s:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    if !problems.is_empty() {
        return Err(PedalError::Invalid(problems));
    }

    // Assemble in order, with inputs and signal sources renumbered.
    let position: HashMap<usize, usize> = order.iter().enumerate().map(|(p, i)| (*i, p)).collect();
    let mut nodes = Vec::with_capacity(n);
    for &i in &order {
        let mut engine = engines[i].take().expect("every node built");
        renumber(&mut engine, &position);
        let input = match inputs[i].as_slice() {
            [] => Input::None,
            [one] => Input::One(position[one]),
            many => Input::Many(many.iter().map(|j| position[j]).collect()),
        };
        nodes.push(Node { engine, input });
    }
    let input = position[&def.nodes.iter().position(|n| n.kind == "input").unwrap()];
    let output = position[&def.nodes.iter().position(|n| n.kind == "output").unwrap()];

    // Latency: the longest path from the input to the output, since
    // every branch is summed as it arrives.
    let mut along: Vec<usize> = vec![0; n];
    for (p, node) in nodes.iter().enumerate() {
        let before = match &node.input {
            Input::None => 0,
            Input::One(j) => along[*j],
            Input::Many(js) => js.iter().map(|j| along[*j]).max().unwrap_or(0),
        };
        along[p] = before + node.engine.latency();
    }
    let latency = along[output];

    Ok(PedalProcessor {
        def: def.clone(),
        params,
        values: defaults,
        nodes,
        lanes: Vec::new(),
        scratch: std::array::from_fn(|_| Vec::new()),
        input,
        output,
        latency,
        transport: Transport::default(),
        sample_rate: 48_000.0,
        max_block: 0,
    })
}

/// Points an engine's signal bindings at the ordered positions.
fn renumber(engine: &mut Engine, position: &HashMap<usize, usize>) {
    let fix = |number: &mut Number| {
        if let Source::Signal { node, .. } = &mut number.source {
            *node = position[node];
        }
    };
    match engine {
        Engine::Input | Engine::Copy | Engine::Shaper(_) => {}
        Engine::Gain(n) => fix(&mut n.amount),
        Engine::Biquad(n) => {
            fix(&mut n.freq);
            fix(&mut n.gain);
            fix(&mut n.q);
        }
        Engine::Delay(n) => {
            fix(&mut n.time);
            fix(&mut n.feedback);
            fix(&mut n.mix);
        }
        Engine::Lfo(n) => {
            fix(&mut n.rate);
            fix(&mut n.depth);
            fix(&mut n.phase);
            fix(&mut n.spread);
        }
        Engine::Allpass(n) => {
            fix(&mut n.freq);
            fix(&mut n.feedback);
            fix(&mut n.mix);
        }
    }
}
