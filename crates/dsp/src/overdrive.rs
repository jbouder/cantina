use crate::biquad::Biquad;
use crate::db_to_linear;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;

static PARAMS: [ParamInfo; 3] = [
    ParamInfo::new("Drive", Unit::Decibels, 0.0, 40.0, 20.0),
    ParamInfo::new("Tone", Unit::Hertz, 500.0, 8000.0, 3000.0),
    ParamInfo::new("Level", Unit::Decibels, -24.0, 6.0, -6.0),
];

const DRIVE: ParamId = 0;
const TONE: ParamId = 1;
const LEVEL: ParamId = 2;

/// Cut below this before the clipper so low end does not turn to mud.
const PRE_HIGHPASS_HZ: f32 = 80.0;
const SMOOTHING_SECONDS: f32 = 0.01;
/// Q of a second-order Butterworth section.
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// A soft-clipping overdrive: highpass, drive, `tanh`, tone, level.
///
/// Oversampling is deliberately absent in this first version; M6 adds the
/// shared oversampling stage every nonlinear pedal will use.
#[derive(Debug, Clone)]
pub struct Overdrive {
    drive: Smoother,
    tone_hz: Smoother,
    level: Smoother,
    pre: [Biquad; MAX_CHANNELS],
    tone: [Biquad; MAX_CHANNELS],
    sample_rate: f32,
}

impl Default for Overdrive {
    fn default() -> Self {
        Self {
            drive: Smoother::new(db_to_linear(PARAMS[DRIVE].default)),
            tone_hz: Smoother::new(PARAMS[TONE].default),
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
            pre: [Biquad::identity(); MAX_CHANNELS],
            tone: [Biquad::identity(); MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }
}

impl Overdrive {
    fn update_tone(&mut self, frames: usize) {
        if self.tone_hz.is_settled() {
            return;
        }
        let hz = self.tone_hz.advance_by(frames);
        for filter in &mut self.tone {
            filter.set_lowpass(self.sample_rate, hz, BUTTERWORTH_Q);
        }
    }
}

impl Processor for Overdrive {
    fn name(&self) -> &str {
        "Overdrive"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            DRIVE => crate::linear_to_db(self.drive.target()),
            TONE => self.tone_hz.target(),
            LEVEL => crate::linear_to_db(self.level.target()),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        match id {
            DRIVE => self
                .drive
                .set_target(db_to_linear(PARAMS[DRIVE].clamp(value))),
            TONE => self.tone_hz.set_target(PARAMS[TONE].clamp(value)),
            LEVEL => self
                .level
                .set_target(db_to_linear(PARAMS[LEVEL].clamp(value))),
            _ => {}
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.drive, &mut self.tone_hz, &mut self.level] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            // Nothing is playing yet, so land on the target at once.
            s.snap(s.target());
        }
        let hz = self.tone_hz.target();
        for (pre, tone) in self.pre.iter_mut().zip(&mut self.tone) {
            pre.set_highpass(self.sample_rate, PRE_HIGHPASS_HZ, BUTTERWORTH_Q);
            tone.set_lowpass(self.sample_rate, hz, BUTTERWORTH_Q);
        }
        self.reset();
    }

    fn reset(&mut self) {
        for f in self.pre.iter_mut().chain(self.tone.iter_mut()) {
            f.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        self.update_tone(frames);
        let channels = buffer.channels().min(MAX_CHANNELS);
        for frame in 0..frames {
            let drive = self.drive.advance();
            let level = self.level.advance();
            for ch in 0..channels {
                let x = buffer.channel_mut(ch)[frame];
                let pre = self.pre[ch].tick(x);
                let clipped = (pre * drive).tanh();
                let toned = self.tone[ch].tick(clipped);
                buffer.channel_mut(ch)[frame] = toned * level;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(od: &mut Overdrive, mono: &mut [f32]) {
        let mut channels = [mono];
        let mut buffer = Buffer::new(&mut channels);
        od.process(&mut buffer);
    }

    fn sine(frequency: f32, sample_rate: f32, n: usize, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                amplitude * (2.0 * std::f32::consts::PI * frequency * i as f32 / sample_rate).sin()
            })
            .collect()
    }

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|x| x * x).sum::<f32>() / buffer.len() as f32).sqrt()
    }

    #[test]
    fn silence_stays_silent() {
        let mut od = Overdrive::default();
        od.prepare(48_000.0, 512);
        let mut buffer = vec![0.0f32; 512];
        run(&mut od, &mut buffer);
        assert!(buffer.iter().all(|s| s.abs() < 1e-7));
    }

    #[test]
    fn output_is_bounded_by_level() {
        let mut od = Overdrive::default();
        od.set_param(LEVEL, 0.0);
        od.prepare(48_000.0, 4800);
        let mut buffer = sine(220.0, 48_000.0, 4800, 10.0);
        run(&mut od, &mut buffer);
        let peak = buffer.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        // tanh keeps the clipper under 1.0; the Butterworth tone filter
        // rings a little on the clipped edges, so allow a small overshoot.
        assert!(peak <= 1.15, "peak {peak}");
        assert!(
            peak > 0.9,
            "peak {peak}: the clipper should be driven hard here"
        );
    }

    #[test]
    fn more_drive_means_more_compression() {
        let sample_rate = 48_000.0f32;
        let mut quiet = Overdrive::default();
        quiet.set_param(DRIVE, 0.0);
        quiet.set_param(LEVEL, 0.0);
        quiet.prepare(sample_rate as f64, 9600);
        let mut hot = Overdrive::default();
        hot.set_param(DRIVE, 40.0);
        hot.set_param(LEVEL, 0.0);
        hot.prepare(sample_rate as f64, 9600);

        let input = sine(220.0, sample_rate, 9600, 0.05);
        let mut a = input.clone();
        let mut b = input.clone();
        run(&mut quiet, &mut a);
        run(&mut hot, &mut b);
        let (a, b) = (&a[4800..], &b[4800..]);
        // A sine at 0.05 through 40 dB of drive hits the clipper hard, so
        // it comes out far louder than the unity-drive path but well under
        // the gain alone would give.
        assert!(rms(b) > rms(a) * 5.0, "quiet {} hot {}", rms(a), rms(b));
        assert!(rms(b) < 1.0, "hot path {}", rms(b));
    }

    #[test]
    fn tone_sweep_does_not_jump() {
        let mut od = Overdrive::default();
        od.prepare(48_000.0, 64);
        let mut buffer = sine(440.0, 48_000.0, 48_000, 0.3);
        // Process in blocks while the tone sweeps from 500 Hz to 8 kHz.
        od.set_param(TONE, 500.0);
        for (i, block) in buffer.chunks_mut(64).enumerate() {
            if i == 100 {
                od.set_param(TONE, 8000.0);
            }
            run(&mut od, block);
        }
        let max_step = buffer[6400..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // A 440 Hz tone at this level moves at most about 0.06 per sample;
        // a click would be far larger.
        assert!(max_step < 0.1, "largest step {max_step}");
    }
}
