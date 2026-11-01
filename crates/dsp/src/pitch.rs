//! Pitch detection for the tuner: the McLeod pitch method over a short
//! window, with the period refined over several cycles.
//!
//! The normalised square difference function (NSDF) of a window scores
//! every candidate period by how well the signal matches itself one
//! period later, from -1 to 1. Its peaks at multiples of the period are
//! the same height for a periodic signal, so the first peak that comes
//! close to the highest is the period, not an octave of it. The peak is
//! located between samples with a parabola, then the estimate is
//! refined by looking at the peak several periods out and dividing: an
//! error of a fraction of a sample at lag `m·τ` is `m` times smaller in
//! `τ`. That is what gets a high note, whose period is only a handful
//! of samples, to within a cent.
//!
//! The functions here are pure and take their scratch space, so the
//! tuner can run them on the audio thread a few lags at a time.

/// Samples per analysis window.
pub const WINDOW: usize = 1024;
/// Longest period examined, in samples. At the tuner's 12 kHz analysis
/// rate that is 23 Hz, below the lowest string of a five-string bass.
pub const MAX_LAG: usize = 512;

/// A peak this close to the highest NSDF peak is taken as the period
/// even though a multiple of it scores a little higher.
const PEAK_RATIO: f32 = 0.93;
/// Below this the window is not periodic enough to report.
const MIN_CLARITY: f32 = 0.5;
/// Most positive regions of the NSDF examined; beyond this the signal
/// is far above any guitar note.
const MAX_PEAKS: usize = 96;

/// A detected pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    pub hz: f32,
    /// How periodic the window was, 0 to 1.
    pub clarity: f32,
}

/// Cumulative energy of `window`: `prefix[i]` is the sum of squares of
/// the first `i` samples. `prefix` must be one longer than `window`.
pub fn energy_prefix(window: &[f32], prefix: &mut [f32]) {
    debug_assert_eq!(prefix.len(), window.len() + 1);
    let mut sum = 0.0f32;
    prefix[0] = 0.0;
    for (i, x) in window.iter().enumerate() {
        sum += x * x;
        prefix[i + 1] = sum;
    }
}

/// Computes `nsdf[lag]` for `lag` in `from..to` from `window` and its
/// [`energy_prefix`]. Each lag costs `window.len() - lag` multiplies.
pub fn nsdf_range(window: &[f32], prefix: &[f32], nsdf: &mut [f32], from: usize, to: usize) {
    let n = window.len();
    let total = prefix[n];
    for lag in from..to.min(n) {
        let (head, tail) = (&window[..n - lag], &window[lag..]);
        let acf: f32 = head.iter().zip(tail).map(|(a, b)| a * b).sum();
        let energy = prefix[n - lag] + (total - prefix[lag]);
        nsdf[lag] = if energy > 0.0 {
            2.0 * acf / energy
        } else {
            0.0
        };
    }
}

/// The peak of a parabola through three equally spaced samples, as an
/// offset from the middle one and its height there.
fn parabola(y0: f32, y1: f32, y2: f32) -> (f32, f32) {
    let denominator = y0 - 2.0 * y1 + y2;
    if denominator.abs() < 1e-12 {
        return (0.0, y1);
    }
    let offset = (0.5 * (y0 - y2) / denominator).clamp(-1.0, 1.0);
    (offset, y1 - 0.25 * (y0 - y2) * offset)
}

/// The interpolated peak around index `i`, which must have a neighbour
/// on each side.
fn peak_at(nsdf: &[f32], i: usize) -> (f32, f32) {
    let (offset, value) = parabola(nsdf[i - 1], nsdf[i], nsdf[i + 1]);
    (i as f32 + offset, value)
}

/// Picks the period from an NSDF: the first positive peak that comes
/// within [`PEAK_RATIO`] of the highest. Returns the lag and its value.
pub fn pick(nsdf: &[f32]) -> Option<(f32, f32)> {
    let n = nsdf.len();
    let mut peaks = [(0.0f32, 0.0f32); MAX_PEAKS];
    let mut count = 0;
    // Lag 0 scores 1 by definition; start after its region ends.
    let mut i = 1;
    while i < n && nsdf[i] > 0.0 {
        i += 1;
    }
    while i < n && count < MAX_PEAKS {
        while i < n && nsdf[i] <= 0.0 {
            i += 1;
        }
        let mut best = None::<usize>;
        while i < n && nsdf[i] > 0.0 {
            if best.is_none_or(|b| nsdf[i] > nsdf[b]) {
                best = Some(i);
            }
            i += 1;
        }
        if let Some(b) = best {
            if b >= 1 && b + 1 < n {
                peaks[count] = peak_at(nsdf, b);
                count += 1;
            }
        }
    }
    let peaks = &peaks[..count];
    let highest = peaks.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    if count == 0 || highest < MIN_CLARITY {
        return None;
    }
    peaks.iter().copied().find(|p| p.1 >= PEAK_RATIO * highest)
}

/// Refines a period by locating the NSDF peak `m` periods out for
/// growing `m` and dividing back, while such a peak fits in the NSDF
/// and is clear.
pub fn refine(nsdf: &[f32], period: f32, clarity: f32) -> f32 {
    let n = nsdf.len();
    let mut tau = period;
    let mut m = 2usize;
    while (m as f32 * tau) < (n - 2) as f32 {
        let center = m as f32 * tau;
        let half = (tau * 0.25).max(1.0);
        let lo = ((center - half).floor() as usize).max(1);
        let hi = ((center + half).ceil() as usize).min(n - 2);
        if lo >= hi {
            break;
        }
        let best = (lo..=hi).fold(lo, |b, i| if nsdf[i] > nsdf[b] { i } else { b });
        let (lag, value) = peak_at(nsdf, best);
        let is_local_max = nsdf[best] >= nsdf[best - 1] && nsdf[best] >= nsdf[best + 1];
        if !is_local_max || value < 0.5 * clarity {
            break;
        }
        tau = lag / m as f32;
        m *= 2;
    }
    tau
}

/// Detects the pitch of `window` sampled at `sample_rate`, computing
/// the whole NSDF into `nsdf` (of length [`MAX_LAG`] or less) with
/// `prefix` as scratch (one longer than the window).
pub fn detect(
    window: &[f32],
    sample_rate: f32,
    prefix: &mut [f32],
    nsdf: &mut [f32],
) -> Option<Pitch> {
    energy_prefix(window, prefix);
    nsdf_range(window, prefix, nsdf, 0, nsdf.len());
    finish(nsdf, sample_rate)
}

/// Turns a complete NSDF into a pitch.
pub fn finish(nsdf: &[f32], sample_rate: f32) -> Option<Pitch> {
    let (lag, clarity) = pick(nsdf)?;
    let period = refine(nsdf, lag, clarity);
    if period <= 0.0 {
        return None;
    }
    Some(Pitch {
        hz: sample_rate / period,
        clarity: clarity.clamp(0.0, 1.0),
    })
}

/// Cents from `reference` to `hz`.
pub fn cents(hz: f32, reference: f32) -> f32 {
    1200.0 * (hz / reference).log2()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: f32 = 12_000.0;

    fn run(mut signal: impl FnMut(f32) -> f32) -> Option<Pitch> {
        let window: Vec<f32> = (0..WINDOW).map(|i| signal(i as f32 / RATE)).collect();
        let mut prefix = vec![0.0; WINDOW + 1];
        let mut nsdf = vec![0.0; MAX_LAG];
        detect(&window, RATE, &mut prefix, &mut nsdf)
    }

    #[test]
    fn pure_tones_across_the_fretboard_are_within_a_cent() {
        for hz in [
            41.2, 55.0, 82.4, 110.0, 146.8, 196.0, 246.9, 329.6, 440.0, 659.3, 1318.5,
        ] {
            let pitch = run(|t| 0.3 * (TAU * hz * t + 0.7).sin()).expect("a pitch");
            let off = cents(pitch.hz, hz);
            assert!(
                off.abs() < 1.0,
                "{hz} Hz read as {} Hz, {off} cents",
                pitch.hz
            );
            assert!(pitch.clarity > 0.9, "{hz} Hz clarity {}", pitch.clarity);
        }
    }

    #[test]
    fn a_rich_tone_is_not_read_an_octave_off() {
        for hz in [82.4, 110.0, 330.0, 1000.0] {
            let pitch = run(|t| {
                (1..=6)
                    .map(|n| (TAU * hz * n as f32 * t).sin() / n as f32)
                    .sum::<f32>()
                    * 0.2
            })
            .expect("a pitch");
            let off = cents(pitch.hz, hz);
            assert!(
                off.abs() < 1.0,
                "{hz} Hz read as {} Hz, {off} cents",
                pitch.hz
            );
        }
    }

    #[test]
    fn noise_and_silence_read_nothing() {
        assert!(run(|_| 0.0).is_none());
        let mut seed = 12345u32;
        let noise = run(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        });
        assert!(noise.is_none_or(|p| p.clarity < 0.9), "{noise:?}");
    }

    #[test]
    fn cents_are_signed() {
        assert!((cents(440.0, 440.0)).abs() < 1e-6);
        assert!((cents(880.0, 440.0) - 1200.0).abs() < 1e-3);
        assert!(cents(430.0, 440.0) < 0.0);
    }
}
