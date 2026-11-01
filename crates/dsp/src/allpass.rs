//! A first-order allpass: flat in level, it shifts phase around its
//! corner frequency. A cascade of them mixed with the dry signal is a
//! phaser; the pedal format's `allpass` node runs one.

/// One stage.
#[derive(Debug, Clone, Copy, Default)]
pub struct Allpass {
    coefficient: f32,
    x1: f32,
    y1: f32,
}

impl Allpass {
    /// Puts the 90° point at `hz`.
    pub fn set_frequency(&mut self, hz: f32, sample_rate: f32) {
        let t = (std::f32::consts::PI * hz / sample_rate).tan();
        self.coefficient = (1.0 - t) / (1.0 + t);
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = -self.coefficient * x + self.x1 + self.coefficient * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn flat_in_level_and_a_quarter_turn_at_the_corner() {
        let mut stage = Allpass::default();
        stage.set_frequency(1000.0, 48_000.0);
        let n = 48_000;
        let input: Vec<f32> = (0..n)
            .map(|i| (TAU * 1000.0 * i as f32 / 48_000.0).sin())
            .collect();
        let out: Vec<f32> = input.iter().map(|x| stage.tick(*x)).collect();
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        assert!((rms(&out[n / 2..]) / rms(&input[n / 2..]) - 1.0).abs() < 1e-3);
        // 90° behind: the output is a negative cosine, zero where the
        // input peaks.
        let i = n / 2 + 12;
        assert!((input[i] - 1.0).abs() < 1e-3, "{}", input[i]);
        assert!(out[i].abs() < 0.02, "{}", out[i]);
    }
}
