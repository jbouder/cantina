/// A one-pole smoother that eases a value toward its target one sample at a
/// time. Used for every parameter the audio thread reads so UI changes
/// never produce a step in the output.
#[derive(Debug, Clone)]
pub struct Smoother {
    current: f32,
    target: f32,
    coefficient: f32,
}

impl Smoother {
    /// A smoother that starts settled at `value` with no smoothing until
    /// [`Smoother::set_time`] is called.
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            coefficient: 0.0,
        }
    }

    /// Sets the time constant in seconds at the given sample rate. After one
    /// time constant the value has covered about 63% of the way to the target.
    pub fn set_time(&mut self, seconds: f32, sample_rate: f32) {
        self.coefficient = if seconds <= 0.0 || sample_rate <= 0.0 {
            0.0
        } else {
            (-1.0 / (seconds * sample_rate)).exp()
        };
    }

    /// Starts easing toward a new target.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps to a value immediately with no smoothing.
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    /// The value the smoother is heading toward.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The value as of the last step.
    pub fn current(&self) -> f32 {
        self.current
    }

    /// True once the value has reached its target.
    pub fn is_settled(&self) -> bool {
        self.current == self.target
    }

    /// Advances one sample and returns the smoothed value.
    #[inline]
    pub fn advance(&mut self) -> f32 {
        if self.current != self.target {
            self.current = self.target + (self.current - self.target) * self.coefficient;
            if (self.current - self.target).abs() < 1e-6 {
                self.current = self.target;
            }
        }
        self.current
    }

    /// Advances `n` samples at once, for parameters that are only read once
    /// per block. Returns the value after the jump.
    pub fn advance_by(&mut self, n: usize) -> f32 {
        if self.current != self.target {
            let decay = self.coefficient.powi(n as i32);
            self.current = self.target + (self.current - self.target) * decay;
            if (self.current - self.target).abs() < 1e-6 {
                self.current = self.target;
            }
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_is_immediate() {
        let mut s = Smoother::new(0.0);
        s.set_time(0.1, 48_000.0);
        s.snap(0.7);
        assert_eq!(s.advance(), 0.7);
        assert!(s.is_settled());
    }

    #[test]
    fn advance_by_matches_repeated_advance() {
        let mut a = Smoother::new(0.0);
        let mut b = Smoother::new(0.0);
        for s in [&mut a, &mut b] {
            s.set_time(0.01, 48_000.0);
            s.set_target(1.0);
        }
        for _ in 0..100 {
            a.advance();
        }
        b.advance_by(100);
        assert!((a.current() - b.current()).abs() < 1e-5);
    }

    #[test]
    fn settles_eventually() {
        let mut s = Smoother::new(0.0);
        s.set_time(0.005, 48_000.0);
        s.set_target(1.0);
        s.advance_by(48_000);
        assert!(s.is_settled());
    }
}
