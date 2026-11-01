/// A peak envelope follower: rises toward the input with one time
/// constant and falls with another. The level detector behind the gate
/// and the smoother behind the compressor's gain computer.
#[derive(Debug, Clone)]
pub struct Envelope {
    value: f32,
    attack: f32,
    release: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    /// A follower at zero that tracks its input exactly until
    /// [`Envelope::set_times`] is called.
    pub fn new() -> Self {
        Self {
            value: 0.0,
            attack: 0.0,
            release: 0.0,
        }
    }

    /// Per-sample coefficient for a time constant: after `seconds` the
    /// follower has covered about 63% of a step.
    pub fn coefficient(seconds: f32, sample_rate: f32) -> f32 {
        if seconds <= 0.0 || sample_rate <= 0.0 {
            0.0
        } else {
            (-1.0 / (seconds * sample_rate)).exp()
        }
    }

    pub fn set_times(&mut self, attack_seconds: f32, release_seconds: f32, sample_rate: f32) {
        self.attack = Self::coefficient(attack_seconds, sample_rate);
        self.release = Self::coefficient(release_seconds, sample_rate);
    }

    pub fn reset(&mut self) {
        self.value = 0.0;
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Follows one (non-negative) sample and returns the envelope.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let c = if x > self.value {
            self.attack
        } else {
            self.release
        };
        self.value = x + (self.value - x) * c;
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rises_at_the_attack_rate_and_falls_at_the_release_rate() {
        let mut e = Envelope::new();
        e.set_times(0.001, 0.1, 48_000.0);
        // One attack time constant: about 63% of the way up.
        for _ in 0..48 {
            e.tick(1.0);
        }
        assert!((e.value() - 0.632).abs() < 0.02, "{}", e.value());
        for _ in 0..4800 {
            e.tick(1.0);
        }
        assert!(e.value() > 0.999);
        // One release time constant: about 37% left.
        for _ in 0..4800 {
            e.tick(0.0);
        }
        assert!((e.value() - 0.368).abs() < 0.02, "{}", e.value());
    }

    #[test]
    fn without_times_it_tracks_exactly() {
        let mut e = Envelope::new();
        assert_eq!(e.tick(0.5), 0.5);
        assert_eq!(e.tick(0.1), 0.1);
    }
}
