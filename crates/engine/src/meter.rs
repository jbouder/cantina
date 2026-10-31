//! Level metering as a graph node.
//!
//! A [`Meter`] sits anywhere in the graph as a sink: it reads the buffer
//! it is given, publishes peak and RMS per channel through atomics, and
//! leaves the audio alone. The UI side reads them through a
//! [`MeterHandle`] at display rate. Peak is held between reads so a
//! transient shorter than a frame still shows; RMS is a running power
//! average with a short time constant so it reads steadily.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cantina_dsp::{Buffer, MAX_CHANNELS};

use crate::graph::{BlockContext, Node};

/// Time constant of the RMS average, in seconds.
const RMS_WINDOW: f64 = 0.05;

/// One channel's levels, linear. 0 is silence, 1 is full scale.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    /// Highest absolute sample since the last read.
    pub peak: f32,
    pub rms: f32,
}

#[derive(Default)]
struct Cell {
    peak_bits: AtomicU32,
    rms_bits: AtomicU32,
}

/// The audio-thread half: a node that measures what flows through it.
pub struct Meter {
    cells: Arc<[Cell; MAX_CHANNELS]>,
    /// Running mean square per channel.
    power: [f64; MAX_CHANNELS],
    /// Per-block smoothing coefficient, set in `prepare`.
    coeff: f64,
    sample_rate: f64,
}

/// The UI-thread half. Clone it freely.
#[derive(Clone)]
pub struct MeterHandle {
    cells: Arc<[Cell; MAX_CHANNELS]>,
}

impl Meter {
    pub fn new() -> (Meter, MeterHandle) {
        let cells: Arc<[Cell; MAX_CHANNELS]> = Arc::new(Default::default());
        (
            Meter {
                cells: Arc::clone(&cells),
                power: [0.0; MAX_CHANNELS],
                coeff: 1.0,
                sample_rate: 48_000.0,
            },
            MeterHandle { cells },
        )
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new().0
    }
}

impl Node for Meter {
    fn name(&self) -> &str {
        "Meter"
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate;
        self.coeff = 1.0;
    }

    fn reset(&mut self) {
        self.power = [0.0; MAX_CHANNELS];
    }

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames == 0 {
            return;
        }
        // The window is a fixed time, so the per-block coefficient
        // depends on the block length: coeff = 1 - exp(-frames / (sr * T)).
        let coeff = 1.0 - (-(frames as f64) / (self.sample_rate * RMS_WINDOW)).exp();
        for c in 0..buffer.channels().min(MAX_CHANNELS) {
            let lane = buffer.channel(c);
            let mut peak = 0.0f32;
            let mut sum = 0.0f64;
            for &s in lane {
                peak = peak.max(s.abs());
                sum += (s as f64) * (s as f64);
            }
            let mean = sum / frames as f64;
            self.power[c] += (mean - self.power[c]) * coeff;
            let cell = &self.cells[c];
            // Positive f32 bit patterns order like the floats, so max on
            // the bits is max on the values.
            cell.peak_bits.fetch_max(peak.to_bits(), Ordering::Relaxed);
            cell.rms_bits
                .store((self.power[c].sqrt() as f32).to_bits(), Ordering::Relaxed);
        }
    }
}

impl MeterHandle {
    /// The levels of one channel. Peak resets to zero on read.
    pub fn take(&self, channel: usize) -> Level {
        let cell = &self.cells[channel.min(MAX_CHANNELS - 1)];
        Level {
            peak: f32::from_bits(cell.peak_bits.swap(0, Ordering::Relaxed)),
            rms: f32::from_bits(cell.rms_bits.load(Ordering::Relaxed)),
        }
    }

    /// The levels of one channel without resetting the peak hold.
    pub fn peek(&self, channel: usize) -> Level {
        let cell = &self.cells[channel.min(MAX_CHANNELS - 1)];
        Level {
            peak: f32::from_bits(cell.peak_bits.load(Ordering::Relaxed)),
            rms: f32::from_bits(cell.rms_bits.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoMap;
    use crate::transport::LoopRange;

    fn ctx(tempo: &TempoMap, frames: usize) -> BlockContext<'_> {
        BlockContext {
            sample_rate: 48_000.0,
            frames,
            playing: false,
            position: 0,
            loop_range: None::<LoopRange>,
            tempo,
        }
    }

    #[test]
    fn measures_peak_and_settles_rms() {
        let tempo = TempoMap::default();
        let (mut meter, handle) = Meter::new();
        meter.prepare(48_000.0, 64);
        // A full-scale square wave has peak 1 and RMS 1.
        let mut lane = vec![1.0f32; 64];
        lane[10] = -1.0;
        // Twelve time constants, so the average has settled.
        for _ in 0..600 {
            let mut lanes: [&mut [f32]; 1] = [&mut lane[..]];
            let mut buffer = Buffer::new(&mut lanes);
            meter.process(&ctx(&tempo, 64), &mut buffer);
        }
        let level = handle.take(0);
        assert_eq!(level.peak, 1.0);
        assert!((level.rms - 1.0).abs() < 1e-3, "rms {}", level.rms);
        // Peak is held between reads, then cleared by the read.
        assert_eq!(handle.take(0).peak, 0.0);
    }

    #[test]
    fn silence_reads_zero_and_does_not_touch_audio() {
        let tempo = TempoMap::default();
        let (mut meter, handle) = Meter::new();
        meter.prepare(48_000.0, 64);
        let mut lane = [0.25f32; 32];
        let mut lanes: [&mut [f32]; 1] = [&mut lane[..]];
        let mut buffer = Buffer::new(&mut lanes);
        meter.process(&ctx(&tempo, 32), &mut buffer);
        assert!(lane.iter().all(|&s| s == 0.25));
        let (mut quiet, handle2) = Meter::new();
        quiet.prepare(48_000.0, 64);
        let mut silence = [0.0f32; 32];
        let mut lanes: [&mut [f32]; 1] = [&mut silence[..]];
        let mut buffer = Buffer::new(&mut lanes);
        quiet.process(&ctx(&tempo, 32), &mut buffer);
        assert_eq!(handle2.take(0), Level::default());
        assert_eq!(handle.take(0).peak, 0.25);
    }
}
