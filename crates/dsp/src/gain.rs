use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit};
use crate::smooth::Smoother;
use crate::{db_to_linear, linear_to_db};

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Gain", Unit::Decibels, -60.0, 24.0, 0.0)];

/// A smoothed gain stage. The first and simplest processor.
#[derive(Debug, Clone)]
pub struct Gain {
    linear: Smoother,
}

impl Default for Gain {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl Gain {
    /// Default smoothing time for gain changes, in seconds.
    pub const SMOOTHING_SECONDS: f32 = 0.005;

    /// A gain at `db` decibels, unprepared.
    pub fn new(db: f32) -> Self {
        Self {
            linear: Smoother::new(db_to_linear(PARAMS[0].clamp(db))),
        }
    }

    /// Sets the gain in decibels. Smoothed on the audio thread.
    pub fn set_db(&mut self, db: f32) {
        self.linear.set_target(db_to_linear(PARAMS[0].clamp(db)));
    }

    /// The target gain in decibels.
    pub fn db(&self) -> f32 {
        linear_to_db(self.linear.target())
    }
}

impl Processor for Gain {
    fn name(&self) -> &str {
        "Gain"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            0 => self.db(),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == 0 {
            self.set_db(value);
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.linear
            .set_time(Self::SMOOTHING_SECONDS, sample_rate as f32);
        // Nothing is playing yet, so land on the target at once.
        self.linear.snap(self.linear.target());
    }

    fn process(&mut self, buffer: &mut Buffer) {
        if self.linear.is_settled() {
            let g = self.linear.target();
            if g != 1.0 {
                for channel in buffer.channels_mut() {
                    for sample in channel.iter_mut() {
                        *sample *= g;
                    }
                }
            }
            return;
        }
        for frame in 0..buffer.frames() {
            let g = self.linear.advance();
            for channel in buffer.channels_mut() {
                channel[frame] *= g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(gain: &mut Gain, mono: &mut [f32]) {
        let mut channels = [mono];
        let mut buffer = Buffer::new(&mut channels);
        gain.process(&mut buffer);
    }

    #[test]
    fn unity_gain_is_identity() {
        let mut gain = Gain::default();
        gain.prepare(48_000.0, 64);
        let mut buffer = vec![0.25, -0.5, 1.0, -1.0];
        let original = buffer.clone();
        run(&mut gain, &mut buffer);
        assert_eq!(buffer, original);
    }

    #[test]
    fn gain_reaches_target_without_steps() {
        let mut gain = Gain::default();
        gain.prepare(48_000.0, 4800);
        gain.set_param(0, -6.0206);
        let mut buffer = vec![1.0f32; 4800];
        run(&mut gain, &mut buffer);
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
    fn stereo_channels_get_the_same_gain() {
        let mut gain = Gain::new(-20.0);
        gain.prepare(48_000.0, 4);
        let mut left = [1.0f32; 4];
        let mut right = [0.5f32; 4];
        let mut channels = [&mut left[..], &mut right[..]];
        let mut buffer = Buffer::new(&mut channels);
        gain.process(&mut buffer);
        for i in 0..4 {
            assert!((left[i] - 0.1).abs() < 1e-5);
            assert!((right[i] - 0.05).abs() < 1e-5);
        }
    }
}
