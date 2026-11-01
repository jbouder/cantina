use std::f32::consts::PI;

#[cfg(test)]
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// A second-order IIR filter in transposed direct form II, with the
/// classic cookbook designs. One instance per channel.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::identity()
    }
}

impl Biquad {
    /// A filter that passes its input unchanged.
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Replaces the coefficients, keeping the filter state so a sweep does
    /// not click.
    pub fn set_lowpass(&mut self, sample_rate: f32, frequency: f32, q: f32) {
        let (sin, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        let _ = sin;
        let b1 = 1.0 - cos;
        let b0 = b1 / 2.0;
        self.set_normalized(b0, b1, b0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    pub fn set_highpass(&mut self, sample_rate: f32, frequency: f32, q: f32) {
        let (_, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        let b1 = -(1.0 + cos);
        let b0 = (1.0 + cos) / 2.0;
        self.set_normalized(b0, b1, b0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    /// A peaking (bell) boost or cut of `gain_db` around `frequency`.
    pub fn set_peaking(&mut self, sample_rate: f32, frequency: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        self.set_normalized(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        );
    }

    /// A low shelf: `gain_db` below `frequency`, flat above.
    pub fn set_lowshelf(&mut self, sample_rate: f32, frequency: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        let beta = 2.0 * a.sqrt() * alpha;
        self.set_normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + beta),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - beta),
            (a + 1.0) + (a - 1.0) * cos + beta,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - beta,
        );
    }

    /// A high shelf: `gain_db` above `frequency`, flat below.
    pub fn set_highshelf(&mut self, sample_rate: f32, frequency: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (_, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        let beta = 2.0 * a.sqrt() * alpha;
        self.set_normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + beta),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - beta),
            (a + 1.0) - (a - 1.0) * cos + beta,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - beta,
        );
    }

    /// A notch: removes `frequency`, narrower with higher `q`.
    pub fn set_notch(&mut self, sample_rate: f32, frequency: f32, q: f32) {
        let (_, cos, alpha) = Self::prewarp(sample_rate, frequency, q);
        self.set_normalized(1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    /// Takes another filter's coefficients, keeping this one's state.
    pub fn set_coefficients_from(&mut self, other: &Biquad) {
        self.b0 = other.b0;
        self.b1 = other.b1;
        self.b2 = other.b2;
        self.a1 = other.a1;
        self.a2 = other.a2;
    }

    /// Clears the delay state. Call when a stream restarts.
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Filters one sample.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    /// Filters a block in place.
    pub fn process(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.tick(*sample);
        }
    }

    fn prewarp(sample_rate: f32, frequency: f32, q: f32) -> (f32, f32, f32) {
        let frequency = frequency.clamp(1.0, sample_rate * 0.49);
        let w0 = 2.0 * PI * frequency / sample_rate;
        let (sin, cos) = w0.sin_cos();
        (sin, cos, sin / (2.0 * q.max(1e-3)))
    }

    #[allow(clippy::too_many_arguments)]
    fn set_normalized(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_at(filter: &mut Biquad, sample_rate: f32, frequency: f32) -> f32 {
        filter.reset();
        let n = (sample_rate as usize) / 4;
        let mut sum = 0.0;
        for i in 0..n {
            let x = (2.0 * PI * frequency * i as f32 / sample_rate).sin();
            let y = filter.tick(x);
            // Skip the first part while the filter settles.
            if i > n / 2 {
                sum += y * y;
            }
        }
        (sum / (n - n / 2) as f32).sqrt()
    }

    #[test]
    fn identity_passes_through() {
        let mut f = Biquad::identity();
        let mut buf = vec![0.1, -0.2, 0.3];
        f.process(&mut buf);
        assert_eq!(buf, vec![0.1, -0.2, 0.3]);
    }

    #[test]
    fn lowpass_attenuates_highs_and_keeps_lows() {
        let mut f = Biquad::identity();
        f.set_lowpass(48_000.0, 1_000.0, BUTTERWORTH_Q);
        let low = rms_at(&mut f, 48_000.0, 100.0);
        let high = rms_at(&mut f, 48_000.0, 10_000.0);
        assert!((low - BUTTERWORTH_Q).abs() < 0.02, "low band rms {low}");
        assert!(high < 0.01, "high band rms {high}");
    }

    #[test]
    fn peaking_and_shelves_boost_where_they_say() {
        let mut peak = Biquad::identity();
        peak.set_peaking(48_000.0, 1_000.0, 6.0, 1.0);
        let at = rms_at(&mut peak, 48_000.0, 1_000.0);
        let away = rms_at(&mut peak, 48_000.0, 100.0);
        assert!(
            (20.0 * (at / BUTTERWORTH_Q).log10() - 6.0).abs() < 0.2,
            "{at}"
        );
        assert!((away - BUTTERWORTH_Q).abs() < 0.02, "{away}");

        let mut low = Biquad::identity();
        low.set_lowshelf(48_000.0, 500.0, -12.0, BUTTERWORTH_Q);
        let below = rms_at(&mut low, 48_000.0, 50.0);
        let above = rms_at(&mut low, 48_000.0, 8_000.0);
        assert!(
            (20.0 * (below / BUTTERWORTH_Q).log10() + 12.0).abs() < 0.3,
            "{below}"
        );
        assert!((above - BUTTERWORTH_Q).abs() < 0.02, "{above}");

        let mut high = Biquad::identity();
        high.set_highshelf(48_000.0, 4_000.0, 9.0, BUTTERWORTH_Q);
        let below = rms_at(&mut high, 48_000.0, 200.0);
        let above = rms_at(&mut high, 48_000.0, 15_000.0);
        assert!((below - BUTTERWORTH_Q).abs() < 0.02, "{below}");
        assert!(
            (20.0 * (above / BUTTERWORTH_Q).log10() - 9.0).abs() < 0.3,
            "{above}"
        );
    }

    #[test]
    fn highpass_attenuates_lows_and_keeps_highs() {
        let mut f = Biquad::identity();
        f.set_highpass(48_000.0, 1_000.0, BUTTERWORTH_Q);
        let low = rms_at(&mut f, 48_000.0, 50.0);
        let high = rms_at(&mut f, 48_000.0, 8_000.0);
        assert!(low < 0.01, "low band rms {low}");
        assert!((high - BUTTERWORTH_Q).abs() < 0.02, "high band rms {high}");
    }
}
