//! Pure audio processing for Cantina.
//!
//! This crate knows nothing about devices, files, threads or the UI. It
//! holds the building blocks that run inside the audio callback, so every
//! type here obeys the real-time rules:
//!
//! - `process` never allocates, locks, blocks or performs I/O.
//! - All memory a processor needs is reserved when it is created or
//!   prepared for a sample rate and block size.
//! - Parameter changes are smoothed so a step change never clicks.
//!
//! The `Processor` trait that stock pedals, user pedals and third-party
//! plugins share lands in M0 issue 4. Until then this crate holds the
//! primitives the first passthrough needs.

/// Converts decibels to a linear amplitude multiplier.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Converts a linear amplitude multiplier to decibels, clamping silence to
/// a finite floor instead of returning negative infinity.
#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-9).log10()
}

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
}

/// A smoothed gain stage. The first and simplest processor.
#[derive(Debug, Clone)]
pub struct Gain {
    linear: Smoother,
    sample_rate: f32,
}

impl Gain {
    /// Default smoothing time for gain changes, in seconds.
    pub const SMOOTHING_SECONDS: f32 = 0.005;

    /// A unity gain prepared for `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let mut gain = Self {
            linear: Smoother::new(1.0),
            sample_rate,
        };
        gain.prepare(sample_rate);
        gain
    }

    /// Reconfigures for a sample rate. Called off the audio thread.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.linear.set_time(Self::SMOOTHING_SECONDS, sample_rate);
    }

    /// Sets the gain in decibels. Safe to call from the audio thread; the
    /// change is smoothed over [`Gain::SMOOTHING_SECONDS`].
    pub fn set_db(&mut self, db: f32) {
        self.linear.set_target(db_to_linear(db));
    }

    /// The target gain in decibels.
    pub fn db(&self) -> f32 {
        linear_to_db(self.linear.target())
    }

    /// Applies the gain in place. Real-time safe.
    #[inline]
    pub fn process(&mut self, buffer: &mut [f32]) {
        if self.linear.is_settled() {
            let g = self.linear.target();
            if g != 1.0 {
                for sample in buffer.iter_mut() {
                    *sample *= g;
                }
            }
            return;
        }
        for sample in buffer.iter_mut() {
            *sample *= self.linear.advance();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_conversions_round_trip() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 1e-4);
        assert!((linear_to_db(db_to_linear(-12.0)) + 12.0).abs() < 1e-4);
        assert!(linear_to_db(0.0).is_finite());
    }

    #[test]
    fn unity_gain_is_identity() {
        let mut gain = Gain::new(48_000.0);
        let mut buffer = vec![0.25, -0.5, 1.0, -1.0];
        let original = buffer.clone();
        gain.process(&mut buffer);
        assert_eq!(buffer, original);
    }

    #[test]
    fn gain_reaches_target_without_steps() {
        let mut gain = Gain::new(48_000.0);
        gain.set_db(-6.0206);
        // Ten smoothing time constants is plenty to settle.
        let mut buffer = vec![1.0f32; 4800];
        gain.process(&mut buffer);
        assert!((buffer[buffer.len() - 1] - 0.5).abs() < 1e-3);
        let max_step = buffer
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_step < 0.01,
            "largest step between samples was {max_step}"
        );
        assert!(buffer.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn smoother_snap_is_immediate() {
        let mut s = Smoother::new(0.0);
        s.set_time(0.1, 48_000.0);
        s.snap(0.7);
        assert_eq!(s.advance(), 0.7);
        assert!(s.is_settled());
    }
}
