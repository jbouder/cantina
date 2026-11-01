//! Partitioned convolution for cabinet impulse responses, with no added
//! latency.
//!
//! The impulse response is cut into partitions one block long. The
//! first partition is convolved directly in the time domain, sample by
//! sample, so the output for a sample is ready the moment the sample
//! arrives. The rest are convolved in the frequency domain: each time
//! a block's worth of input is complete, its spectrum goes into a
//! frequency-domain delay line and is multiplied against every later
//! partition's spectrum, which yields the tail of the output for the
//! *next* block period, in time for it. Input blocks can be any size up
//! to the partition, so a short last block in a render works, and the
//! latency is zero whatever the block size.
//!
//! The spectra of a real block are conjugate-symmetric, so only the
//! lower half plus one bin is multiplied; the rest is mirrored before
//! the inverse transform.

use crate::fft::{Complex, Fft};

/// The convolver, one per channel. Set the impulse, prepare, process.
#[derive(Debug, Clone)]
pub struct Convolver {
    impulse: Vec<f32>,
    partition: usize,
    fft: Option<Fft>,
    /// The first partition's taps, for the direct part.
    direct: Vec<f32>,
    /// Ring of the last `partition` inputs for the direct part.
    history: Vec<f32>,
    history_at: usize,
    /// Spectra of partitions 1.., each `2 * partition` bins.
    partitions: Vec<Vec<Complex>>,
    /// Spectra of past input blocks, newest at `spectra_at`.
    spectra: Vec<Vec<Complex>>,
    spectra_at: usize,
    /// The previous block and the one being filled.
    chunk: Vec<f32>,
    chunk_fill: usize,
    /// The frequency-domain tail for the current block period.
    tail: Vec<f32>,
    tail_at: usize,
    scratch: Vec<Complex>,
    acc: Vec<Complex>,
}

impl Default for Convolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Convolver {
    /// A convolver with no impulse: it passes audio through.
    pub fn new() -> Self {
        Self {
            impulse: Vec::new(),
            partition: 0,
            fft: None,
            direct: Vec::new(),
            history: Vec::new(),
            history_at: 0,
            partitions: Vec::new(),
            spectra: Vec::new(),
            spectra_at: 0,
            chunk: Vec::new(),
            chunk_fill: 0,
            tail: Vec::new(),
            tail_at: 0,
            scratch: Vec::new(),
            acc: Vec::new(),
        }
    }

    /// Sets the impulse response at the rate the convolver will be
    /// prepared for. Off the audio thread; takes effect at `prepare`.
    pub fn set_impulse(&mut self, impulse: &[f32]) {
        self.impulse = impulse.to_vec();
        self.partition = 0;
    }

    pub fn impulse(&self) -> &[f32] {
        &self.impulse
    }

    /// Samples in the impulse response.
    pub fn length(&self) -> usize {
        self.impulse.len()
    }

    /// Reserves everything for blocks up to `max_block` frames and
    /// splits the impulse into partitions that long.
    pub fn prepare(&mut self, max_block: usize) {
        let partition = max_block.next_power_of_two().max(8);
        self.partition = partition;
        let n = self.impulse.len();
        let fft = Fft::new(2 * partition);
        self.direct = vec![0.0; partition];
        let first = n.min(partition);
        self.direct[..first].copy_from_slice(&self.impulse[..first]);
        let count = n.saturating_sub(partition).div_ceil(partition);
        self.partitions = (0..count)
            .map(|j| {
                let start = (j + 1) * partition;
                let end = (start + partition).min(n);
                let mut spectrum = vec![Complex::ZERO; 2 * partition];
                for (bin, tap) in spectrum.iter_mut().zip(&self.impulse[start..end]) {
                    bin.re = *tap;
                }
                fft.forward(&mut spectrum);
                spectrum
            })
            .collect();
        self.spectra = vec![vec![Complex::ZERO; 2 * partition]; count.max(1)];
        self.fft = Some(fft);
        self.history = vec![0.0; partition];
        self.chunk = vec![0.0; 2 * partition];
        self.tail = vec![0.0; partition];
        self.scratch = vec![Complex::ZERO; 2 * partition];
        self.acc = vec![Complex::ZERO; 2 * partition];
        self.reset();
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.history_at = 0;
        for s in &mut self.spectra {
            s.fill(Complex::ZERO);
        }
        self.spectra_at = 0;
        self.chunk.fill(0.0);
        self.chunk_fill = 0;
        self.tail.fill(0.0);
        self.tail_at = 0;
    }

    /// Convolves `block` in place. Blocks may be any length up to what
    /// `prepare` was given. Without an impulse, or unprepared, the
    /// audio passes through.
    pub fn process(&mut self, block: &mut [f32]) {
        let p = self.partition;
        if p == 0 || self.impulse.is_empty() {
            return;
        }
        for sample in block.iter_mut() {
            let x = *sample;
            self.history[self.history_at] = x;
            // Direct part: the first partition against the last p inputs.
            let mut out = 0.0f32;
            let (newer, older) = self.history.split_at(self.history_at + 1);
            // newer holds inputs up to now in order; older the ones before,
            // wrapping. Tap i pairs with the input i samples back.
            for (tap, value) in self.direct.iter().zip(newer.iter().rev()) {
                out += tap * value;
            }
            for (tap, value) in self.direct[newer.len()..].iter().zip(older.iter().rev()) {
                out += tap * value;
            }
            self.history_at = (self.history_at + 1) % p;
            // Tail from the partitions beyond the first.
            if !self.partitions.is_empty() {
                out += self.tail[self.tail_at];
                self.tail_at += 1;
                self.chunk[p + self.chunk_fill] = x;
                self.chunk_fill += 1;
                if self.chunk_fill == p {
                    self.advance_tail();
                }
            }
            *sample = out;
        }
    }

    /// A block of input is complete: transform it and compute the tail
    /// for the next block period.
    fn advance_tail(&mut self) {
        let p = self.partition;
        let fft = self.fft.as_ref().expect("prepared");
        for (bin, x) in self.scratch.iter_mut().zip(&self.chunk) {
            *bin = Complex { re: *x, im: 0.0 };
        }
        fft.forward(&mut self.scratch);
        let count = self.spectra.len();
        self.spectra_at = (self.spectra_at + 1) % count;
        self.spectra[self.spectra_at].copy_from_slice(&self.scratch);
        // acc = sum over partitions j of H_j * X_{now - (j - 1)}, lower
        // half plus the Nyquist bin only.
        self.acc.fill(Complex::ZERO);
        for (j, h) in self.partitions.iter().enumerate() {
            let x = &self.spectra[(self.spectra_at + count - j % count) % count];
            for k in 0..=p {
                self.acc[k] = self.acc[k] + h[k] * x[k];
            }
        }
        for k in 1..p {
            let c = self.acc[k];
            self.acc[2 * p - k] = Complex {
                re: c.re,
                im: -c.im,
            };
        }
        fft.inverse(&mut self.acc);
        for (t, bin) in self.tail.iter_mut().zip(&self.acc[p..]) {
            *t = bin.re;
        }
        self.tail_at = 0;
        let (previous, current) = self.chunk.split_at_mut(p);
        previous.copy_from_slice(current);
        self.chunk_fill = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    fn direct(signal: &[f32], impulse: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; signal.len()];
        for (n, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0f64;
            for (i, h) in impulse.iter().enumerate() {
                if i > n {
                    break;
                }
                acc += (*h as f64) * (signal[n - i] as f64);
            }
            *o = acc as f32;
        }
        out
    }

    fn run(impulse: &[f32], signal: &[f32], prepared: usize, block: usize) -> Vec<f32> {
        let mut c = Convolver::new();
        c.set_impulse(impulse);
        c.prepare(prepared);
        let mut out = signal.to_vec();
        for chunk in out.chunks_mut(block) {
            c.process(chunk);
        }
        out
    }

    #[test]
    fn matches_direct_convolution_with_no_latency() {
        let impulse = noise(1000, 1);
        let signal = noise(3000, 2);
        let reference = direct(&signal, &impulse);
        for (prepared, block) in [(64, 64), (64, 17), (128, 128), (256, 100), (16, 16)] {
            let out = run(&impulse, &signal, prepared, block);
            let worst = out
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(
                worst < 1e-4,
                "prepared {prepared}, block {block}: off by {worst}"
            );
        }
    }

    #[test]
    fn short_impulses_and_no_impulse() {
        let signal = noise(500, 3);
        let out = run(&[0.5, 0.25], &signal, 64, 64);
        let reference = direct(&signal, &[0.5, 0.25]);
        assert!(out
            .iter()
            .zip(&reference)
            .all(|(a, b)| (a - b).abs() < 1e-6));
        let out = run(&[], &signal, 64, 64);
        assert_eq!(out, signal);
        // Exactly one partition long, and one over.
        for n in [64, 65, 128, 129] {
            let impulse = noise(n, 9);
            let out = run(&impulse, &signal, 64, 64);
            let reference = direct(&signal, &impulse);
            let worst = out
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-4, "{n} taps: off by {worst}");
        }
    }

    #[test]
    fn a_two_second_impulse_at_64_frames_is_affordable() {
        let impulse = noise(96_000, 4);
        let seconds = 5;
        let signal = noise(48_000 * seconds, 5);
        let mut c = Convolver::new();
        c.set_impulse(&impulse);
        c.prepare(64);
        let mut out = signal;
        let start = Instant::now();
        for chunk in out.chunks_mut(64) {
            c.process(chunk);
        }
        let share = start.elapsed().as_secs_f64() / seconds as f64;
        eprintln!(
            "2 s impulse, 64-frame blocks: {:.1}% of real time per channel",
            share * 100.0
        );
        assert!(share < 0.5, "{share:.2} of real time");
    }
}
