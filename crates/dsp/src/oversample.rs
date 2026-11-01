//! Oversampling for nonlinear pedals: run the waveshaper at two, four or
//! eight times the sample rate so the harmonics it creates land below
//! the real Nyquist frequency, instead of folding back as aliases.
//!
//! Each doubling is a halfband FIR stage: zero-stuff and filter on the
//! way up, filter and drop every other sample on the way down. Halfband
//! filters have every other tap at zero, so a stage costs about a
//! quarter of its length per sample, and in polyphase form the
//! interpolator's even outputs are a plain delay. The filters are
//! linear phase, so the round trip is a fixed delay the pedal reports
//! as latency; the stages are sized so that delay is a whole number of
//! samples at every factor, and the oversampler pads lower factors to
//! the delay of 8×, so switching factors never moves the signal in time
//! and the chain's compensation never has to change.

use std::f32::consts::PI;

/// Largest factor.
pub const MAX_FACTOR: usize = 8;
/// Samples of delay at the base rate, whatever the factor.
pub const LATENCY: usize = 22;
/// Names for a `Unit::Choice` of factors; index `i` is factor `1 << i`.
pub static FACTOR_NAMES: [&str; 4] = ["Off", "2×", "4×", "8×"];

/// Half the length of each stage's halfband filter, at that stage's
/// rate: the first stage (to 2×) is 31 taps, then 21, then 17. The
/// stage's round trip is twice this at its rate, so 15, 5 and 2
/// samples at the base rate: 22 in all.
const STAGE_DELAY: [usize; 3] = [15, 10, 8];
/// Kaiser window shape for each stage. The higher stages have a wider
/// transition band to play with, so shorter filters do.
const STAGE_BETA: [f32; 3] = [7.0, 6.0, 5.0];

/// A halfband FIR used both as an interpolator and a decimator.
#[derive(Debug, Clone)]
struct Halfband {
    /// Nonzero taps as (index, coefficient), the centre included.
    taps: Vec<(usize, f32)>,
    /// Interpolation branch for the odd outputs: (input offset, coefficient).
    odd_branch: Vec<(usize, f32)>,
    /// Input offset the even outputs copy from (the centre tap).
    even_offset: usize,
    /// Which output parity the centre tap serves.
    centre_parity: usize,
    /// The last samples of the previous block, oldest first.
    history: Vec<f32>,
    /// History followed by the current block.
    line: Vec<f32>,
}

fn bessel_i0(x: f32) -> f32 {
    let mut sum = 1.0f32;
    let mut term = 1.0f32;
    let half = x / 2.0;
    for k in 1..30 {
        term *= half / k as f32;
        sum += term * term;
        if term * term < 1e-10 * sum {
            break;
        }
    }
    sum
}

impl Halfband {
    fn design(delay: usize, beta: f32) -> Self {
        let length = 2 * delay + 1;
        let mut taps = Vec::new();
        for k in 0..length {
            let n = k as f32 - delay as f32;
            let value = if k == delay {
                0.5
            } else if (k + delay) % 2 == 0 {
                0.0
            } else {
                let sinc = (PI * n / 2.0).sin() / (PI * n / 2.0) / 2.0;
                let r = n / delay as f32;
                let window = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
                sinc * window
            };
            if value != 0.0 {
                taps.push((k, value));
            }
        }
        let centre_parity = delay % 2;
        let odd_parity = 1 - centre_parity;
        let odd_branch = taps
            .iter()
            .filter(|(k, _)| k % 2 == odd_parity)
            .map(|(k, c)| ((k - odd_parity) / 2, 2.0 * c))
            .collect();
        Self {
            taps,
            odd_branch,
            even_offset: delay / 2,
            centre_parity,
            history: vec![0.0; length],
            line: Vec::new(),
        }
    }

    fn prepare(&mut self, max_input: usize) {
        self.line = vec![0.0; self.history.len() + max_input];
        self.reset();
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
    }

    /// Doubles the rate of `input` into `output`, which is twice as long.
    fn interpolate(&mut self, input: &[f32], output: &mut [f32]) {
        let h = self.history.len();
        let n = input.len();
        debug_assert_eq!(output.len(), 2 * n);
        self.line[..h].copy_from_slice(&self.history);
        self.line[h..h + n].copy_from_slice(input);
        let line = &self.line[..h + n];
        for i in 0..n {
            let at = h + i;
            // The branch holding the centre tap is a delay; the other
            // sums the nonzero taps.
            output[2 * i + self.centre_parity] = line[at - self.even_offset];
            let mut acc = 0.0f32;
            for &(offset, c) in &self.odd_branch {
                acc += c * line[at - offset];
            }
            output[2 * i + 1 - self.centre_parity] = acc;
        }
        self.history.copy_from_slice(&line[n..n + h]);
    }

    /// Halves the rate of `input` into `output`, which is half as long.
    fn decimate(&mut self, input: &[f32], output: &mut [f32]) {
        let h = self.history.len();
        let n = input.len();
        debug_assert_eq!(output.len() * 2, n);
        self.line[..h].copy_from_slice(&self.history);
        self.line[h..h + n].copy_from_slice(input);
        let line = &self.line[..h + n];
        for (i, out) in output.iter_mut().enumerate() {
            let at = h + 2 * i;
            let mut acc = 0.0f32;
            for &(k, c) in &self.taps {
                acc += c * line[at - k];
            }
            *out = acc;
        }
        self.history.copy_from_slice(&line[n..n + h]);
    }
}

/// A fixed delay that pads a lower factor to the latency of the
/// highest.
#[derive(Debug, Clone)]
struct Pad {
    buffer: Vec<f32>,
    at: usize,
}

impl Pad {
    fn new(samples: usize) -> Self {
        Self {
            buffer: vec![0.0; samples],
            at: 0,
        }
    }

    fn process(&mut self, block: &mut [f32]) {
        if self.buffer.is_empty() {
            return;
        }
        for s in block {
            std::mem::swap(&mut self.buffer[self.at], s);
            self.at = (self.at + 1) % self.buffer.len();
        }
    }
}

/// Runs a nonlinear function on one channel at a multiple of the sample
/// rate. One per channel.
#[derive(Debug, Clone)]
pub struct Oversampler {
    factor: usize,
    up: Vec<Halfband>,
    down: Vec<Halfband>,
    /// `work[k]` holds a block at `2^k` times the rate.
    work: Vec<Vec<f32>>,
    pad: Pad,
    max_block: usize,
}

impl Default for Oversampler {
    fn default() -> Self {
        Self::new(4)
    }
}

impl Oversampler {
    /// An oversampler at `factor` (1, 2, 4 or 8; others round down to
    /// the nearest), unprepared.
    pub fn new(factor: usize) -> Self {
        let mut os = Self {
            factor: 1,
            up: (0..3)
                .map(|k| Halfband::design(STAGE_DELAY[k], STAGE_BETA[k]))
                .collect(),
            down: (0..3)
                .map(|k| Halfband::design(STAGE_DELAY[k], STAGE_BETA[k]))
                .collect(),
            work: Vec::new(),
            pad: Pad::new(LATENCY),
            max_block: 0,
        };
        os.set_factor(factor);
        os
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    fn stages(factor: usize) -> usize {
        match factor {
            8.. => 3,
            4..=7 => 2,
            2..=3 => 1,
            _ => 0,
        }
    }

    /// Delay of the filters alone at the base rate, before padding.
    fn stage_latency(stages: usize) -> usize {
        (0..stages).map(|k| (2 * STAGE_DELAY[k]) >> (k + 1)).sum()
    }

    /// Changes the factor. The filters restart, so do this between
    /// notes if it can be heard; the latency does not change.
    pub fn set_factor(&mut self, factor: usize) {
        let stages = Self::stages(factor);
        self.factor = 1 << stages;
        self.pad = Pad::new(LATENCY - Self::stage_latency(stages));
        self.reset();
    }

    /// Samples of delay at the base rate, the same at every factor.
    pub fn latency(&self) -> usize {
        LATENCY
    }

    /// Reserves the buffers for blocks up to `max_block`.
    pub fn prepare(&mut self, max_block: usize) {
        self.max_block = max_block;
        self.work = (0..=3).map(|k| vec![0.0; max_block << k]).collect();
        for k in 0..3 {
            self.up[k].prepare(max_block << k);
            self.down[k].prepare(max_block << (k + 1));
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for stage in self.up.iter_mut().chain(self.down.iter_mut()) {
            stage.reset();
        }
        self.pad = Pad::new(self.pad.buffer.len());
    }

    /// Runs `shape` over `block` at the oversampled rate, in place.
    pub fn process(&mut self, block: &mut [f32], mut shape: impl FnMut(&mut [f32])) {
        let stages = Self::stages(self.factor);
        let n = block.len();
        debug_assert!(n <= self.max_block, "block larger than prepared for");
        if stages == 0 {
            shape(block);
            self.pad.process(block);
            return;
        }
        self.work[0][..n].copy_from_slice(block);
        for k in 0..stages {
            let (lower, upper) = self.work.split_at_mut(k + 1);
            self.up[k].interpolate(&lower[k][..n << k], &mut upper[0][..n << (k + 1)]);
        }
        shape(&mut self.work[stages][..n << stages]);
        for k in (0..stages).rev() {
            let (lower, upper) = self.work.split_at_mut(k + 1);
            self.down[k].decimate(&upper[0][..n << (k + 1)], &mut lower[k][..n << k]);
        }
        block.copy_from_slice(&self.work[0][..n]);
        self.pad.process(block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: f32 = 48_000.0;

    fn sine(hz: f32, n: usize, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amplitude * (TAU * hz * i as f32 / RATE).sin())
            .collect()
    }

    fn run(
        os: &mut Oversampler,
        input: &[f32],
        block: usize,
        mut shape: impl FnMut(f32) -> f32,
    ) -> Vec<f32> {
        os.prepare(block);
        let mut out = input.to_vec();
        for chunk in out.chunks_mut(block) {
            os.process(chunk, |up| {
                for s in up.iter_mut() {
                    *s = shape(*s);
                }
            });
        }
        out
    }

    /// Power at `hz` in `signal`, in dB.
    fn goertzel_db(signal: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let coeff = 2.0 * w.cos() as f64;
        for (i, x) in signal.iter().enumerate() {
            // Hann window against leakage from the fundamental.
            let window = 0.5 - 0.5 * (TAU * i as f32 / signal.len() as f32).cos();
            let s0 = (x * window) as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        10.0 * (power.max(1e-30)).log10() as f32
    }

    #[test]
    fn filters_are_halfband_with_the_planned_delay() {
        for (k, delay) in STAGE_DELAY.iter().enumerate() {
            let hb = Halfband::design(*delay, STAGE_BETA[k]);
            assert_eq!(hb.history.len(), 2 * delay + 1);
            let sum: f32 = hb.taps.iter().map(|(_, c)| c).sum();
            assert!((sum - 1.0).abs() < 0.01, "stage {k} DC gain {sum}");
            assert!(hb
                .taps
                .iter()
                .all(|(i, _)| i == delay || (i + delay) % 2 == 1));
        }
        assert_eq!(Oversampler::stage_latency(3), LATENCY);
        assert_eq!(Oversampler::stage_latency(2), 20);
        assert_eq!(Oversampler::stage_latency(1), 15);
    }

    #[test]
    fn passes_audio_through_with_a_fixed_delay_at_every_factor() {
        let input = sine(1000.0, 4800, 0.5);
        for factor in [1, 2, 4, 8] {
            for block in [64, 37] {
                let mut os = Oversampler::new(factor);
                assert_eq!(os.factor(), factor);
                assert_eq!(os.latency(), LATENCY);
                let out = run(&mut os, &input, block, |x| x);
                let worst = (500..4800)
                    .map(|i| (out[i] - input[i - LATENCY]).abs())
                    .fold(0.0f32, f32::max);
                assert!(
                    worst < 2e-3,
                    "factor {factor}, block {block}: off by {worst}"
                );
            }
        }
    }

    #[test]
    fn oversampling_removes_aliases_of_a_clipped_tone() {
        // A 5 kHz tone clipped hard makes odd harmonics at 15, 25, 35,
        // 45 kHz... At 48 kHz those above 24 kHz fold back to 23, 13, 3
        // and 7 kHz, where no harmonic belongs.
        let input = sine(5000.0, 48_000, 1.0);
        let clip = |x: f32| x.clamp(-0.3, 0.3);
        let plain = run(&mut Oversampler::new(1), &input, 64, clip);
        let eight = run(&mut Oversampler::new(8), &input, 64, clip);
        let fundamental = goertzel_db(&plain, 5000.0);
        for alias in [3000.0, 7000.0, 13_000.0] {
            let before = goertzel_db(&plain, alias) - fundamental;
            let after = goertzel_db(&eight, alias) - fundamental;
            eprintln!("alias at {alias} Hz: {before:.1} dB plain, {after:.1} dB at 8x");
            assert!(
                before > -60.0,
                "the plain path should alias at {alias} Hz: {before} dB"
            );
            assert!(
                after < before - 20.0,
                "8x should cut the alias at {alias} Hz by 20 dB: {before} -> {after}"
            );
        }
        // The harmonic that belongs is kept.
        let kept = goertzel_db(&eight, 15_000.0) - goertzel_db(&plain, 15_000.0);
        assert!(kept.abs() < 3.0, "15 kHz harmonic moved by {kept} dB");
    }

    #[test]
    fn switching_factor_keeps_the_latency() {
        let input = sine(500.0, 2048, 0.5);
        let mut os = Oversampler::new(8);
        os.prepare(64);
        let mut out = input.clone();
        for (i, chunk) in out.chunks_mut(64).enumerate() {
            if i == 16 {
                os.set_factor(2);
            }
            os.process(chunk, |_| {});
        }
        // After the switch settles, the delay is the same 22 samples.
        let worst = (1500..2048)
            .map(|i| (out[i] - input[i - LATENCY]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 2e-3, "{worst}");
        assert_eq!(os.factor(), 2);
    }
}
