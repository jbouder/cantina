/// A circular delay line read at a fractional delay with linear
/// interpolation. The modulation pedals and the delay use one per
/// channel.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    /// A line that can delay by up to `max_samples`.
    pub fn new(max_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_samples.max(2) + 1],
            write: 0,
        }
    }

    pub fn max_delay(&self) -> usize {
        self.buffer.len() - 1
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    /// Stores the newest sample.
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.buffer[self.write] = x;
        self.write += 1;
        if self.write == self.buffer.len() {
            self.write = 0;
        }
    }

    /// The sample pushed `delay` samples ago: 0 is the newest.
    #[inline]
    pub fn read_at(&self, delay: usize) -> f32 {
        let delay = delay.min(self.max_delay());
        let len = self.buffer.len();
        self.buffer[(self.write + len - 1 - delay) % len]
    }

    /// The signal `delay` samples ago, between samples.
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(0.0, (self.max_delay() - 1) as f32);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = self.read_at(whole);
        let b = self.read_at(whole + 1);
        a + (b - a) * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_what_was_pushed() {
        let mut line = DelayLine::new(8);
        for i in 0..20 {
            line.push(i as f32);
        }
        assert_eq!(line.read_at(0), 19.0);
        assert_eq!(line.read_at(3), 16.0);
        assert_eq!(line.read_at(8), 11.0);
        // Past the end clamps to the oldest sample held.
        assert_eq!(line.read_at(50), 11.0);
    }

    #[test]
    fn fractional_reads_interpolate() {
        let mut line = DelayLine::new(16);
        for i in 0..32 {
            line.push(i as f32 * 2.0);
        }
        assert!((line.read(2.5) - 57.0).abs() < 1e-6);
        assert!((line.read(0.0) - 62.0).abs() < 1e-6);
        line.reset();
        assert_eq!(line.read(2.5), 0.0);
    }
}
