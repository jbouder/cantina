use crate::envelope::Envelope;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::readout::Readout;
use crate::{db_to_linear, linear_to_db};

static PARAMS: [ParamInfo; 6] = [
    ParamInfo::new("Threshold", Unit::Decibels, -60.0, 0.0, -20.0),
    ParamInfo::new("Ratio", Unit::Generic, 1.0, 20.0, 4.0),
    ParamInfo::new("Attack", Unit::Milliseconds, 0.1, 200.0, 10.0),
    ParamInfo::new("Release", Unit::Milliseconds, 10.0, 2000.0, 100.0),
    ParamInfo::new("Knee", Unit::Decibels, 0.0, 24.0, 6.0),
    ParamInfo::new("Makeup", Unit::Decibels, 0.0, 24.0, 0.0),
];

const THRESHOLD: ParamId = 0;
const RATIO: ParamId = 1;
const ATTACK: ParamId = 2;
const RELEASE: ParamId = 3;
const KNEE: ParamId = 4;
const MAKEUP: ParamId = 5;

static READOUTS: [&str; 1] = ["Reduction"];

/// The level detector catches a peak at once and lets go of it over
/// this long, so a steady note reads as a steady level rather than a
/// rectified wave that would ripple the gain at the note's frequency.
const DETECTOR_RELEASE_SECONDS: f32 = 0.02;

/// A feed-forward compressor with a soft knee. The level is measured in
/// decibels on the loudest channel, the gain computer draws the static
/// curve, and the reduction it asks for is smoothed with the attack and
/// release times, so a stereo pair is compressed together and the
/// reduction never steps.
#[derive(Debug, Clone)]
pub struct Compressor {
    values: [f32; 6],
    /// Peak level of the loudest channel, linear.
    detector: Envelope,
    /// Smoothed gain reduction in decibels, zero or more.
    reduction: Envelope,
    sample_rate: f32,
    readout: Readout,
}

impl Default for Compressor {
    fn default() -> Self {
        let values = std::array::from_fn(|i| PARAMS[i].default);
        Self {
            values,
            detector: Envelope::new(),
            reduction: Envelope::new(),
            sample_rate: 48_000.0,
            readout: Readout::new(&READOUTS),
        }
    }
}

impl Compressor {
    /// Gain reduction in decibels for an input level, from the static
    /// curve: zero below the knee, the full ratio above it, a parabola
    /// between.
    pub fn reduction_for(&self, level_db: f32) -> f32 {
        let threshold = self.values[THRESHOLD];
        let ratio = self.values[RATIO];
        let knee = self.values[KNEE];
        let over = level_db - threshold;
        let slope = 1.0 / ratio - 1.0;
        let gain = if 2.0 * over <= -knee {
            0.0
        } else if 2.0 * over.abs() <= knee {
            let x = over + knee / 2.0;
            slope * x * x / (2.0 * knee)
        } else {
            slope * over
        };
        -gain
    }

    /// The gain reduction being applied now, in decibels (zero or more).
    pub fn reduction(&self) -> f32 {
        self.reduction.value()
    }

    fn update_times(&mut self) {
        self.reduction.set_times(
            self.values[ATTACK] / 1000.0,
            self.values[RELEASE] / 1000.0,
            self.sample_rate,
        );
    }
}

impl Processor for Compressor {
    fn name(&self) -> &str {
        "Compressor"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        self.values.get(id).copied().unwrap_or(0.0)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if let Some(info) = PARAMS.get(id) {
            self.values[id] = info.clamp(value);
            self.update_times();
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        self.detector
            .set_times(0.0, DETECTOR_RELEASE_SECONDS, self.sample_rate);
        self.update_times();
        self.reset();
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.reduction.reset();
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let makeup = self.values[MAKEUP];
        let channels = buffer.channels().min(MAX_CHANNELS);
        let mut most = 0.0f32;
        for frame in 0..buffer.frames() {
            let mut peak = 0.0f32;
            for ch in 0..channels {
                peak = peak.max(buffer.channel(ch)[frame].abs());
            }
            let level = self.detector.tick(peak);
            let wanted = self.reduction_for(linear_to_db(level));
            let reduction = self.reduction.tick(wanted);
            most = most.max(reduction);
            let gain = db_to_linear(makeup - reduction);
            for ch in 0..channels {
                buffer.channel_mut(ch)[frame] *= gain;
            }
        }
        self.readout.set(0, -most);
    }

    fn readout(&self) -> Option<Readout> {
        Some(self.readout.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    fn sine(hz: f32, seconds: f32, peak: f32) -> Vec<f32> {
        let sr = 48_000.0;
        (0..(seconds * sr) as usize)
            .map(|i| peak * (TAU * hz * i as f32 / sr).sin())
            .collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn hard(threshold: f32, ratio: f32) -> Compressor {
        let mut c = Compressor::default();
        c.set_param(THRESHOLD, threshold);
        c.set_param(RATIO, ratio);
        c.set_param(KNEE, 0.0);
        c
    }

    #[test]
    fn static_curve_follows_the_ratio_above_the_threshold() {
        let c = hard(-20.0, 4.0);
        assert_eq!(c.reduction_for(-30.0), 0.0);
        assert_eq!(c.reduction_for(-20.0), 0.0);
        assert!((c.reduction_for(-10.0) - 7.5).abs() < 1e-5);
        assert!((c.reduction_for(0.0) - 15.0).abs() < 1e-5);
        // A soft knee starts early and is continuous with both sides.
        let mut soft = hard(-20.0, 4.0);
        soft.set_param(KNEE, 10.0);
        assert_eq!(soft.reduction_for(-25.0), 0.0);
        assert!(soft.reduction_for(-22.0) > 0.0);
        assert!(soft.reduction_for(-22.0) < 0.5);
        assert!((soft.reduction_for(-15.0) - 3.75).abs() < 1e-4);
        // Inside the knee the reduction is a little more than the hard
        // curve's; past it the two agree.
        assert!((soft.reduction_for(-17.0) - 2.4).abs() < 1e-4);
        assert!((soft.reduction_for(-10.0) - 7.5).abs() < 1e-4);
    }

    #[test]
    fn steady_tone_is_reduced_by_the_curve_and_lifted_by_makeup() {
        let mut c = hard(-20.0, 4.0);
        c.set_param(ATTACK, 1.0);
        // A sine 10 dB over the threshold: 7.5 dB of reduction.
        let out = Renderer::default().render(&mut c, &[sine(440.0, 1.0, db_to_linear(-10.0))]);
        let settled = peak(&out[0][24_000..]);
        let expected = db_to_linear(-17.5);
        assert!(
            (linear_to_db(settled) - linear_to_db(expected)).abs() < 0.3,
            "settled at {} dB",
            linear_to_db(settled)
        );
        assert!((c.reduction() - 7.5).abs() < 0.3, "{}", c.reduction());
        assert!((c.readout().unwrap().get(0) + 7.5).abs() < 0.3);

        let mut lifted = hard(-20.0, 4.0);
        lifted.set_param(MAKEUP, 6.0);
        let out = Renderer::default().render(&mut lifted, &[sine(440.0, 1.0, db_to_linear(-10.0))]);
        let settled = peak(&out[0][24_000..]);
        assert!(
            (linear_to_db(settled) + 11.5).abs() < 0.3,
            "{}",
            linear_to_db(settled)
        );
    }

    #[test]
    fn below_the_threshold_nothing_changes() {
        let mut c = hard(-20.0, 4.0);
        let input = sine(440.0, 0.5, db_to_linear(-30.0));
        let out = Renderer::default().render(&mut c, std::slice::from_ref(&input));
        assert_eq!(out[0], input);
        assert_eq!(c.reduction(), 0.0);
    }

    #[test]
    fn attack_is_faster_than_release() {
        let mut c = hard(-20.0, 10.0);
        c.set_param(ATTACK, 5.0);
        c.set_param(RELEASE, 200.0);
        c.prepare(48_000.0, 64);
        // Loud for half a second, then quiet. Track the reduction.
        let input: Vec<f32> = sine(440.0, 0.5, 1.0)
            .into_iter()
            .chain(sine(440.0, 0.5, 0.001))
            .collect();
        let mut trace = Vec::new();
        let mut lanes = vec![input];
        Renderer::default().run(&mut lanes, |buffer| {
            c.process(buffer);
            trace.push(c.reduction());
        });
        let full = 18.0; // (0 - -20) * (1 - 1/10)
        let reached = trace.iter().position(|r| *r > full * 0.9).unwrap();
        // 90% of a step takes 2.3 time constants: about 12 ms, 9 blocks.
        assert!(reached < 20, "attack took {reached} blocks");
        let drop = 375; // the block where the input goes quiet
        let released = trace[drop..].iter().position(|r| *r < full * 0.1).unwrap();
        // 2.3 release time constants is 460 ms, 345 blocks.
        assert!(released > 200, "release took only {released} blocks");
        assert!(released < 500, "release took {released} blocks");
    }
}
