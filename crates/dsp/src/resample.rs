//! Sample-rate conversion of whole signals, offline: a windowed-sinc
//! interpolator at any ratio, for mixdowns at a rate other than the
//! project's.
//!
//! Each output sample is the input convolved with a Kaiser-windowed sinc
//! centred on the output's position in the input. The sinc's cutoff sits
//! just below the lower of the two Nyquist frequencies, so going down
//! nothing above the new Nyquist folds back, and going up no images of
//! the old spectrum appear. The kernel is tabulated once per conversion
//! and read with linear interpolation between table points.
//!
//! Allocates; not for the audio thread.

/// Zero crossings of the sinc on each side of the centre, at the cutoff.
/// More is a steeper transition band and a longer kernel.
const ZEROS: usize = 32;
/// Table points per input sample.
const RESOLUTION: usize = 512;
/// Kaiser window shape: about 90 dB of stopband attenuation.
const BETA: f64 = 9.0;
/// Cutoff as a fraction of the lower Nyquist frequency.
const CUTOFF: f64 = 0.95;

/// Converts `input` sampled at `from` Hz to `to` Hz. The output is
/// `ceil(len * to / from)` samples long and starts at the same instant
/// as the input; past either end the input is taken as silence. Equal
/// rates return a copy.
pub fn resample(input: &[f32], from: f64, to: f64) -> Vec<f32> {
    assert!(from > 0.0 && to > 0.0, "sample rates must be positive");
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to / from;
    // Multiplied before dividing, so whole-second lengths come out exact.
    let out_len = (input.len() as f64 * to / from).ceil() as usize;
    // Cutoff in cycles per input sample, relative to the input Nyquist.
    let fc = CUTOFF * ratio.min(1.0);
    let kernel = Kernel::new(fc);
    let half = kernel.half_width;
    let step = 1.0 / ratio;
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let t = n as f64 * step;
        let centre = t.floor() as isize;
        let first = (centre - half as isize + 1).max(0);
        let last = (centre + half as isize).min(input.len() as isize - 1);
        let mut acc = 0.0f64;
        let mut k = first;
        while k <= last {
            acc += input[k as usize] as f64 * kernel.at(t - k as f64);
            k += 1;
        }
        out.push(acc as f32);
    }
    out
}

/// The windowed sinc, tabulated over one side.
struct Kernel {
    /// Input samples from the centre to the last non-zero tap.
    half_width: usize,
    table: Vec<f64>,
}

impl Kernel {
    fn new(fc: f64) -> Self {
        let half_width = (ZEROS as f64 / fc).ceil() as usize;
        let points = half_width * RESOLUTION + 2;
        let i0_beta = bessel_i0(BETA);
        let table = (0..points)
            .map(|i| {
                let x = i as f64 / RESOLUTION as f64;
                if x >= half_width as f64 {
                    return 0.0;
                }
                let r = x / half_width as f64;
                let window = bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0_beta;
                fc * sinc(fc * x) * window
            })
            .collect();
        Self { half_width, table }
    }

    /// The kernel at `x` input samples from the centre.
    fn at(&self, x: f64) -> f64 {
        let pos = x.abs() * RESOLUTION as f64;
        let i = pos as usize;
        if i + 1 >= self.table.len() {
            return 0.0;
        }
        let frac = pos - i as f64;
        self.table[i] + (self.table[i + 1] - self.table[i]) * frac
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// The modified Bessel function of the first kind, order zero, by its
/// power series.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..64 {
        term *= half / k as f64;
        let t = term * term;
        sum += t;
        if t < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, rate: f64, n: usize, amp: f64) -> Vec<f32> {
        (0..n)
            .map(|i| (amp * (std::f64::consts::TAU * hz * i as f64 / rate).sin()) as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn equal_rates_copy() {
        let x = sine(440.0, 48_000.0, 100, 0.5);
        assert_eq!(resample(&x, 48_000.0, 48_000.0), x);
        assert!(resample(&[], 48_000.0, 44_100.0).is_empty());
    }

    #[test]
    fn lengths_follow_the_ratio() {
        let x = vec![0.0; 48_000];
        assert_eq!(resample(&x, 48_000.0, 44_100.0).len(), 44_100);
        assert_eq!(resample(&x[..44_100], 44_100.0, 48_000.0).len(), 48_000);
        assert_eq!(resample(&x[..1001], 48_000.0, 44_100.0).len(), 920);
    }

    #[test]
    fn a_tone_survives_both_ways_in_level_and_shape() {
        for (from, to) in [(48_000.0, 44_100.0), (44_100.0, 48_000.0)] {
            let n = from as usize / 2;
            let x = sine(1_000.0, from, n, 0.5);
            let y = resample(&x, from, to);
            let ideal = sine(1_000.0, to, y.len(), 0.5);
            // Away from the edges, where the kernel runs off the input.
            let edge = 200;
            let worst = y[edge..y.len() - edge]
                .iter()
                .zip(&ideal[edge..])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-3, "{from} -> {to}: worst error {worst}");
        }
    }

    #[test]
    fn content_above_the_new_nyquist_does_not_fold_back() {
        // 23 kHz is above 44.1 kHz's Nyquist; it would alias to 21.1 kHz.
        let x = sine(23_000.0, 48_000.0, 48_000, 0.5);
        let y = resample(&x, 48_000.0, 44_100.0);
        let level = rms(&y[500..y.len() - 500]) / rms(&x);
        let db = 20.0 * level.log10();
        assert!(db < -60.0, "aliased at {db:.1} dB");
    }

    #[test]
    fn the_passband_is_flat() {
        for hz in [50.0, 5_000.0, 18_000.0] {
            let x = sine(hz, 48_000.0, 48_000, 0.5);
            let y = resample(&x, 48_000.0, 44_100.0);
            let db = 20.0 * (rms(&y[500..y.len() - 500]) / rms(&x[500..x.len() - 500])).log10();
            assert!(db.abs() < 0.05, "{hz} Hz moved {db:.3} dB");
        }
    }
}
