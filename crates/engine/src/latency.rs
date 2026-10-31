//! Round-trip latency measurement.
//!
//! Connect an output of the interface to an input with a cable. The
//! [`ClickTrain`] processor plays a click every `period` samples and
//! captures everything the input hears into a lock-free ring. The main
//! thread drains the ring and [`analyze`] finds, for every click, how many
//! samples later it came back. That is the real round trip: driver,
//! converters, cable and our own buffering, as the player hears it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::stream::BlockProcessor;

/// The click: a few samples of alternating full-scale, which has energy at
/// every frequency and a sharp onset.
pub const CLICK: [f32; 4] = [1.0, -1.0, 1.0, -1.0];

/// Emits clicks and captures the input. Runs on the audio thread.
pub struct ClickTrain {
    position: usize,
    lead: usize,
    period: usize,
    amplitude: f32,
    capture: Producer<f32>,
    dropped: Arc<AtomicU64>,
}

/// The main-thread end of a [`ClickTrain`].
pub struct Capture {
    consumer: Consumer<f32>,
    dropped: Arc<AtomicU64>,
    pub lead: usize,
    pub period: usize,
    samples: Vec<f32>,
}

impl ClickTrain {
    /// Clicks start after `lead` samples of silence, used to measure the
    /// noise floor, then repeat every `period` samples. The ring holds
    /// `capacity` samples of captured input; the main thread must drain it
    /// faster than that fills.
    pub fn new(lead: usize, period: usize, amplitude: f32, capacity: usize) -> (Self, Capture) {
        let (capture, consumer) = RingBuffer::new(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        (
            Self {
                position: 0,
                lead,
                period,
                amplitude,
                capture,
                dropped: Arc::clone(&dropped),
            },
            Capture {
                consumer,
                dropped,
                lead,
                period,
                samples: Vec::with_capacity(capacity),
            },
        )
    }
}

impl BlockProcessor for ClickTrain {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let writable = input.len().min(self.capture.slots());
        if let Ok(chunk) = self.capture.write_chunk_uninit(writable) {
            chunk.fill_from_iter(input.iter().copied());
        }
        if writable < input.len() {
            self.dropped
                .fetch_add((input.len() - writable) as u64, Ordering::Relaxed);
        }
        for sample in output.iter_mut() {
            *sample = if self.position >= self.lead {
                let phase = (self.position - self.lead) % self.period;
                CLICK.get(phase).map_or(0.0, |c| c * self.amplitude)
            } else {
                0.0
            };
            self.position += 1;
        }
    }
}

impl Capture {
    /// Moves everything captured so far out of the ring. Call often.
    pub fn drain(&mut self) {
        let n = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            let (first, second) = chunk.as_slices();
            self.samples.extend_from_slice(first);
            self.samples.extend_from_slice(second);
            chunk.commit_all();
        }
    }

    /// Samples captured so far.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Input samples the audio thread could not store because the ring was
    /// full. Any value other than zero invalidates the measurement.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Finds the delay of each click captured so far.
    pub fn analyze(&self, clicks: usize) -> Result<Measurement> {
        if self.dropped() > 0 {
            bail!(
                "{} input samples were dropped; the measurement is invalid",
                self.dropped()
            );
        }
        analyze(&self.samples, self.lead, self.period, clicks)
    }
}

/// The result of a measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Round-trip delay of each detected click, in samples.
    pub delays: Vec<usize>,
    /// RMS of the input during the silent lead-in.
    pub noise_rms: f32,
    /// Level a sample had to exceed to count as a click arriving.
    pub threshold: f32,
}

impl Measurement {
    pub fn min(&self) -> usize {
        self.delays.iter().copied().min().unwrap_or(0)
    }

    pub fn max(&self) -> usize {
        self.delays.iter().copied().max().unwrap_or(0)
    }

    pub fn median(&self) -> usize {
        let mut sorted = self.delays.clone();
        sorted.sort_unstable();
        sorted.get(sorted.len() / 2).copied().unwrap_or(0)
    }
}

/// Finds each click in `captured`. Click `k` was played at sample
/// `lead + k * period`; the delay is the distance to the first sample in
/// the following window that rises clearly above the noise floor.
pub fn analyze(captured: &[f32], lead: usize, period: usize, clicks: usize) -> Result<Measurement> {
    let needed = lead + clicks * period;
    if captured.len() < needed {
        bail!(
            "captured {} samples but {clicks} clicks need {needed}",
            captured.len()
        );
    }
    // Noise floor from the lead-in, skipping the first tenth where the
    // converters may still be settling.
    let quiet = &captured[lead / 10..lead];
    let noise_rms = (quiet.iter().map(|x| x * x).sum::<f32>() / quiet.len().max(1) as f32).sqrt();
    let threshold = (noise_rms * 8.0).max(0.01);

    let mut delays = Vec::with_capacity(clicks);
    for k in 0..clicks {
        let start = lead + k * period;
        let window = &captured[start..start + period];
        let peak = window.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if peak < threshold {
            bail!(
                "click {k} not heard: peak {peak:.4} is below the threshold {threshold:.4}. \
                 Is the loopback cable connected and the input gain up?"
            );
        }
        let onset = threshold.max(peak * 0.3);
        let delay = window
            .iter()
            .position(|x| x.abs() >= onset)
            .expect("the peak itself exceeds the onset level");
        delays.push(delay);
    }
    Ok(Measurement {
        delays,
        noise_rms,
        threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a capture as a loopback would hear it: noise, and each click
    /// returned `delay` samples later, smeared by a converter's filter.
    fn synthetic(lead: usize, period: usize, clicks: usize, delay: usize, noise: f32) -> Vec<f32> {
        let len = lead + clicks * period + period;
        let mut seed = 12345u32;
        let mut captured: Vec<f32> = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * noise
            })
            .collect();
        let smeared = [0.3f32, 0.7, -0.5, 0.4, -0.3, 0.1];
        for k in 0..clicks {
            let at = lead + k * period + delay;
            for (i, s) in smeared.iter().enumerate() {
                captured[at + i] += s * 0.5;
            }
        }
        captured
    }

    #[test]
    fn finds_every_click() {
        let captured = synthetic(24_000, 24_000, 8, 311, 0.001);
        let m = analyze(&captured, 24_000, 24_000, 8).unwrap();
        assert_eq!(m.delays, vec![311; 8]);
        assert_eq!(m.median(), 311);
        assert_eq!(m.min(), m.max());
    }

    #[test]
    fn tolerates_a_noisy_input() {
        let captured = synthetic(24_000, 24_000, 4, 97, 0.02);
        let m = analyze(&captured, 24_000, 24_000, 4).unwrap();
        assert!(
            m.delays.iter().all(|d| (*d as i64 - 97).abs() <= 1),
            "{:?}",
            m.delays
        );
    }

    #[test]
    fn complains_when_nothing_comes_back() {
        let captured = synthetic(24_000, 24_000, 4, 97, 0.0005);
        let silent: Vec<f32> = captured.iter().map(|x| x * 0.001).collect();
        let err = analyze(&silent, 24_000, 24_000, 4).unwrap_err();
        assert!(err.to_string().contains("not heard"), "{err}");
    }

    #[test]
    fn complains_when_too_short() {
        let err = analyze(&[0.0; 100], 50, 50, 4).unwrap_err();
        assert!(err.to_string().contains("need"), "{err}");
    }

    #[test]
    fn click_train_plays_clicks_and_captures_input() {
        let (mut train, mut capture) = ClickTrain::new(8, 16, 0.5, 1024);
        let input: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let mut output = vec![0.0f32; 64];
        train.process(&input, &mut output);
        assert!(output[..8].iter().all(|s| *s == 0.0));
        assert_eq!(&output[8..12], &[0.5, -0.5, 0.5, -0.5]);
        assert!(output[12..24].iter().all(|s| *s == 0.0));
        assert_eq!(&output[24..28], &[0.5, -0.5, 0.5, -0.5]);
        capture.drain();
        assert_eq!(capture.len(), 64);
        assert_eq!(capture.dropped(), 0);
    }
}
