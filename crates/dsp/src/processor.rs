//! The abstraction every pedal shares.

use std::borrow::Cow;
use std::fmt;

/// Most channels a stock processor allocates state for. Plugins may have
/// more; the hosting crate handles that.
pub const MAX_CHANNELS: usize = 2;

/// Index of a parameter in a processor's [`Processor::params`] list.
pub type ParamId = usize;

/// How a parameter's value should be displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Generic,
    Decibels,
    Hertz,
    Seconds,
    Percent,
}

/// Describes one parameter. Values are plain `f32` in the stated range;
/// mapping to a knob position is the UI's job.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    /// Borrowed for stock processors, owned for plugins that report theirs
    /// at load time.
    pub name: Cow<'static, str>,
    pub unit: Unit,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl ParamInfo {
    pub const fn new(name: &'static str, unit: Unit, min: f32, max: f32, default: f32) -> Self {
        Self {
            name: Cow::Borrowed(name),
            unit,
            min,
            max,
            default,
        }
    }

    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

/// Why a state blob could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    WrongLength { expected: usize, found: usize },
    Invalid(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::WrongLength { expected, found } => {
                write!(f, "state is {found} bytes, expected {expected}")
            }
            StateError::Invalid(why) => write!(f, "invalid state: {why}"),
        }
    }
}

impl std::error::Error for StateError {}

/// A block of audio as channel slices, processed in place.
pub struct Buffer<'a, 'b> {
    channels: &'a mut [&'b mut [f32]],
    frames: usize,
}

impl<'a, 'b> Buffer<'a, 'b> {
    /// Wraps channel slices. All channels must have the same length.
    pub fn new(channels: &'a mut [&'b mut [f32]]) -> Self {
        let frames = channels.first().map_or(0, |c| c.len());
        debug_assert!(channels.iter().all(|c| c.len() == frames));
        Self { channels, frames }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, index: usize) -> &[f32] {
        self.channels[index]
    }

    pub fn channel_mut(&mut self, index: usize) -> &mut [f32] {
        self.channels[index]
    }

    pub fn channels_mut(&mut self) -> &mut [&'b mut [f32]] {
        self.channels
    }

    pub fn fill(&mut self, value: f32) {
        for channel in self.channels.iter_mut() {
            channel.fill(value);
        }
    }
}

/// Anything that sits in a pedal chain: stock pedals, user-designed pedals
/// and third-party plugins all implement this, so the chain, the mixer,
/// automation, latency compensation and the project file never ask which
/// kind they hold.
///
/// # Invariants
///
/// - **Real-time safety.** `process`, `set_param` and `param` run on the
///   audio thread and never allocate, lock, block or perform I/O. Every
///   buffer `process` touches is reserved in `prepare` or the constructor.
/// - **Smoothing.** A `set_param` call changes a target; `process` eases
///   the audible value toward it so a step never clicks. `prepare` snaps
///   smoothers to their targets because nothing is playing yet.
/// - **One owner at a time.** A processor lives on the UI thread while it
///   is being built, prepared or saved, and on the audio thread while it
///   is in a running graph. Graph edits move it between the two; nothing
///   touches it from both at once. Plugin wrappers rely on this: their
///   main-thread operations (`prepare`, `state`, `set_state`) are only
///   valid outside a running graph. See `docs/spike.md` for the decision
///   this leaves to M4.
/// - **Clamping.** `set_param` clamps to the parameter's range and ignores
///   unknown ids rather than panicking.
/// - **Failure is bypass.** A processor that cannot prepare or process
///   leaves the buffer untouched and counts the failure; it never panics
///   on the audio thread.
///
/// # Threading
///
/// `prepare`, `reset`, `state` and `set_state` run off the audio thread and
/// may allocate. `process`, `set_param` and `param` run on the audio thread.
pub trait Processor: Send {
    /// Display name.
    fn name(&self) -> &str;

    /// The parameters, indexed by [`ParamId`].
    fn params(&self) -> &[ParamInfo];

    /// Current target value of a parameter, in its own units.
    fn param(&self, id: ParamId) -> f32;

    /// Sets a parameter's target. Out-of-range values are clamped; unknown
    /// ids are ignored.
    fn set_param(&mut self, id: ParamId, value: f32);

    /// Samples of delay this processor adds. Zero for most pedals.
    fn latency(&self) -> usize {
        0
    }

    /// Readies the processor for a sample rate and the largest block it
    /// will see. Called before the first `process` and again on any change.
    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize);

    /// Clears delay lines and filter memory without touching parameters.
    fn reset(&mut self) {}

    /// Processes a block in place.
    fn process(&mut self, buffer: &mut Buffer);

    /// Opaque state for the project file. The default stores every
    /// parameter as a little-endian `f32`. Takes `&mut self` because plugin
    /// APIs hand out state through mutable handles.
    fn state(&mut self) -> Vec<u8> {
        (0..self.params().len())
            .flat_map(|id| self.param(id).to_le_bytes())
            .collect()
    }

    /// Restores state saved by [`Processor::state`].
    fn set_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        let expected = self.params().len() * 4;
        if state.len() != expected {
            return Err(StateError::WrongLength {
                expected,
                found: state.len(),
            });
        }
        for (id, bytes) in state.chunks_exact(4).enumerate() {
            let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !value.is_finite() {
                return Err(StateError::Invalid(format!("parameter {id} is not finite")));
            }
            self.set_param(id, value);
        }
        Ok(())
    }
}

/// An ordered list of processors run back to back, in place.
#[derive(Default)]
pub struct Chain {
    processors: Vec<Box<dyn Processor>>,
}

impl Chain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a processor. Off the audio thread.
    pub fn push(&mut self, processor: Box<dyn Processor>) {
        self.processors.push(processor);
    }

    pub fn len(&self) -> usize {
        self.processors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.processors.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&dyn Processor> {
        self.processors.get(index).map(|p| p.as_ref())
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Box<dyn Processor>> {
        self.processors.get_mut(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Processor> {
        self.processors.iter().map(|p| p.as_ref())
    }

    /// Total latency of the chain in samples.
    pub fn latency(&self) -> usize {
        self.processors.iter().map(|p| p.latency()).sum()
    }

    pub fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        for p in &mut self.processors {
            p.prepare(sample_rate, max_block_frames);
        }
    }

    pub fn reset(&mut self) {
        for p in &mut self.processors {
            p.reset();
        }
    }

    /// Runs every processor over the buffer in order. Real-time safe.
    pub fn process(&mut self, buffer: &mut Buffer) {
        for p in &mut self.processors {
            p.process(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Gain, Overdrive};

    #[test]
    fn buffer_wraps_channels() {
        let mut left = [1.0f32, 2.0];
        let mut right = [3.0f32, 4.0];
        let mut channels = [&mut left[..], &mut right[..]];
        let mut buffer = Buffer::new(&mut channels);
        assert_eq!(buffer.frames(), 2);
        assert_eq!(buffer.channels(), 2);
        buffer.channel_mut(1)[0] = 9.0;
        assert_eq!(buffer.channel(1), &[9.0, 4.0]);
        buffer.fill(0.0);
        assert_eq!(left, [0.0, 0.0]);
    }

    #[test]
    fn default_state_round_trips_parameters() {
        let mut a = Overdrive::default();
        a.set_param(0, 33.0);
        a.set_param(1, 1234.0);
        a.set_param(2, -3.0);
        let blob = a.state();
        let mut b = Overdrive::default();
        b.set_state(&blob).unwrap();
        for id in 0..a.params().len() {
            assert_eq!(a.param(id), b.param(id), "parameter {id}");
        }
        assert!(matches!(
            b.set_state(&blob[1..]),
            Err(StateError::WrongLength { .. })
        ));
    }

    #[test]
    fn set_param_clamps_and_ignores_unknown() {
        let mut g = Gain::default();
        g.set_param(0, 1000.0);
        assert_eq!(g.param(0), g.params()[0].max);
        g.set_param(99, 1.0);
        assert_eq!(g.param(99), 0.0);
    }

    #[test]
    fn chain_runs_in_order_and_sums_latency() {
        struct Delay(usize);
        impl Processor for Delay {
            fn name(&self) -> &str {
                "delay"
            }
            fn params(&self) -> &[ParamInfo] {
                &[]
            }
            fn param(&self, _: ParamId) -> f32 {
                0.0
            }
            fn set_param(&mut self, _: ParamId, _: f32) {}
            fn latency(&self) -> usize {
                self.0
            }
            fn prepare(&mut self, _: f64, _: usize) {}
            fn process(&mut self, buffer: &mut Buffer) {
                for s in buffer.channel_mut(0) {
                    *s += 1.0;
                }
            }
        }
        let mut chain = Chain::new();
        chain.push(Box::new(Delay(10)));
        let mut gain = Gain::default();
        gain.set_param(0, -6.0206);
        chain.push(Box::new(gain));
        chain.push(Box::new(Delay(5)));
        chain.prepare(48_000.0, 64);
        assert_eq!(chain.latency(), 15);
        assert_eq!(chain.len(), 3);

        let mut mono = [0.0f32; 4];
        let mut channels = [&mut mono[..]];
        let mut buffer = Buffer::new(&mut channels);
        chain.process(&mut buffer);
        // (0 + 1) * 0.5 + 1 = 1.5
        assert!(mono.iter().all(|s| (s - 1.5).abs() < 1e-5), "{mono:?}");
    }
}
