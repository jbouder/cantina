//! The drive pedals: overdrive, distortion and fuzz. One core with a
//! voicing each: a highpass to keep the low end out of the clipper, an
//! optional mid boost, the drive gain, the oversampled waveshaper, a
//! tone lowpass and the level.

use crate::biquad::Biquad;
use crate::db_to_linear;
use crate::oversample::{Oversampler, FACTOR_NAMES};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::shaper::Curve;
use crate::smooth::Smoother;

const DRIVE: ParamId = 0;
const TONE: ParamId = 1;
const LEVEL: ParamId = 2;
const CURVE: ParamId = 3;
const OVERSAMPLING: ParamId = 4;

const SMOOTHING_SECONDS: f32 = 0.01;
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// Default oversampling: index into [`FACTOR_NAMES`], 4×.
const DEFAULT_FACTOR_INDEX: usize = 2;

/// What makes one drive pedal different from another.
pub struct Voicing {
    pub name: &'static str,
    pub params: [ParamInfo; 5],
    /// The curves the Curve parameter picks from, in order.
    pub curves: &'static [Curve],
    /// Highpass before the clipper, in hertz.
    pub pre_highpass_hz: f32,
    /// A peaking boost before the clipper: (hertz, decibels, Q).
    pub mid_boost: Option<(f32, f32, f32)>,
    /// Gain always applied before the clipper, on top of Drive.
    pub extra_gain_db: f32,
}

const fn params(
    drive_max: f32,
    drive_default: f32,
    curves: &'static [&'static str],
    level_default: f32,
) -> [ParamInfo; 5] {
    [
        ParamInfo::new("Drive", Unit::Decibels, 0.0, drive_max, drive_default),
        ParamInfo::new("Tone", Unit::Hertz, 500.0, 8000.0, 3000.0),
        ParamInfo::new("Level", Unit::Decibels, -24.0, 6.0, level_default),
        ParamInfo::choice("Curve", curves, 0),
        ParamInfo::choice("Oversampling", &FACTOR_NAMES, DEFAULT_FACTOR_INDEX),
    ]
}

pub static OVERDRIVE: Voicing = Voicing {
    name: "Overdrive",
    params: params(40.0, 20.0, &["Soft", "Asymmetric", "Hard"], -6.0),
    curves: &[Curve::Soft, Curve::Asymmetric, Curve::Hard],
    pre_highpass_hz: 80.0,
    mid_boost: None,
    extra_gain_db: 0.0,
};

pub static DISTORTION: Voicing = Voicing {
    name: "Distortion",
    params: params(60.0, 30.0, &["Hard", "Soft", "Fold"], -12.0),
    curves: &[Curve::Hard, Curve::Soft, Curve::Fold],
    pre_highpass_hz: 60.0,
    mid_boost: Some((900.0, 6.0, 0.7)),
    extra_gain_db: 6.0,
};

pub static FUZZ: Voicing = Voicing {
    name: "Fuzz",
    params: params(60.0, 40.0, &["Silicon", "Germanium", "Octave"], -12.0),
    curves: &[Curve::Silicon, Curve::Germanium, Curve::Octave],
    pre_highpass_hz: 40.0,
    mid_boost: None,
    extra_gain_db: 12.0,
};

/// The drive core: see the module docs.
pub struct Drive {
    voicing: &'static Voicing,
    drive: Smoother,
    tone_hz: Smoother,
    level: Smoother,
    curve: usize,
    factor_index: usize,
    pre: [Biquad; MAX_CHANNELS],
    mid: [Biquad; MAX_CHANNELS],
    /// After the clipper: removes the offset a lopsided curve leaves.
    post: [Biquad; MAX_CHANNELS],
    tone: [Biquad; MAX_CHANNELS],
    oversamplers: [Oversampler; MAX_CHANNELS],
    /// Per-frame smoothed gains, shared by the channels.
    gains: Vec<(f32, f32)>,
    sample_rate: f32,
}

impl Drive {
    pub fn new(voicing: &'static Voicing) -> Self {
        let p = &voicing.params;
        Self {
            voicing,
            drive: Smoother::new(db_to_linear(p[DRIVE].default + voicing.extra_gain_db)),
            tone_hz: Smoother::new(p[TONE].default),
            level: Smoother::new(db_to_linear(p[LEVEL].default)),
            curve: 0,
            factor_index: DEFAULT_FACTOR_INDEX,
            pre: [Biquad::identity(); MAX_CHANNELS],
            mid: [Biquad::identity(); MAX_CHANNELS],
            post: [Biquad::identity(); MAX_CHANNELS],
            tone: [Biquad::identity(); MAX_CHANNELS],
            oversamplers: std::array::from_fn(|_| Oversampler::new(1 << DEFAULT_FACTOR_INDEX)),
            gains: Vec::new(),
            sample_rate: 48_000.0,
        }
    }

    pub fn curve(&self) -> Curve {
        self.voicing.curves[self.curve]
    }

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

impl Processor for Drive {
    fn name(&self) -> &str {
        self.voicing.name
    }

    fn params(&self) -> &[ParamInfo] {
        &self.voicing.params
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            DRIVE => crate::linear_to_db(self.drive.target()) - self.voicing.extra_gain_db,
            TONE => self.tone_hz.target(),
            LEVEL => crate::linear_to_db(self.level.target()),
            CURVE => self.curve as f32,
            OVERSAMPLING => self.factor_index as f32,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let p = &self.voicing.params;
        match id {
            DRIVE => self.drive.set_target(db_to_linear(
                p[DRIVE].clamp(value) + self.voicing.extra_gain_db,
            )),
            TONE => self.tone_hz.set_target(p[TONE].clamp(value)),
            LEVEL => self.level.set_target(db_to_linear(p[LEVEL].clamp(value))),
            CURVE => self.curve = p[CURVE].index(value),
            OVERSAMPLING => {
                let index = p[OVERSAMPLING].index(value);
                if index != self.factor_index {
                    self.factor_index = index;
                    for os in &mut self.oversamplers {
                        os.set_factor(1 << index);
                    }
                }
            }
            _ => {}
        }
    }

    fn latency(&self) -> usize {
        self.oversamplers[0].latency()
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.drive, &mut self.tone_hz, &mut self.level] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        let hz = self.tone_hz.target();
        let v = self.voicing;
        for ch in 0..MAX_CHANNELS {
            self.pre[ch].set_highpass(self.sample_rate, v.pre_highpass_hz, BUTTERWORTH_Q);
            match v.mid_boost {
                Some((f, db, q)) => self.mid[ch].set_peaking(self.sample_rate, f, db, q),
                None => self.mid[ch] = Biquad::identity(),
            }
            self.post[ch].set_highpass(self.sample_rate, 20.0, BUTTERWORTH_Q);
            self.tone[ch].set_lowpass(self.sample_rate, hz, BUTTERWORTH_Q);
            self.oversamplers[ch].prepare(max_block_frames);
        }
        self.gains = vec![(0.0, 0.0); max_block_frames];
        self.reset();
    }

    fn reset(&mut self) {
        for f in self
            .pre
            .iter_mut()
            .chain(self.mid.iter_mut())
            .chain(self.post.iter_mut())
            .chain(self.tone.iter_mut())
        {
            f.reset();
        }
        for os in &mut self.oversamplers {
            os.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames > self.gains.len() {
            // Larger than prepared for: leave the audio alone.
            return;
        }
        self.update_tone(frames);
        for g in &mut self.gains[..frames] {
            *g = (self.drive.advance(), self.level.advance());
        }
        let curve = self.curve();
        let channels = buffer.channels().min(MAX_CHANNELS);
        for ch in 0..channels {
            let lane = buffer.channel_mut(ch);
            for (s, (drive, _)) in lane.iter_mut().zip(&self.gains) {
                let x = self.mid[ch].tick(self.pre[ch].tick(*s));
                *s = x * drive;
            }
            self.oversamplers[ch].process(lane, |up| {
                for s in up.iter_mut() {
                    *s = curve.shape(*s);
                }
            });
            for (s, (_, level)) in lane.iter_mut().zip(&self.gains) {
                let y = self.tone[ch].tick(self.post[ch].tick(*s));
                *s = y * level;
            }
        }
    }
}

macro_rules! drive_pedal {
    ($(#[$doc:meta])* $name:ident, $voicing:ident) => {
        $(#[$doc])*
        pub struct $name(Drive);

        impl Default for $name {
            fn default() -> Self {
                Self(Drive::new(&$voicing))
            }
        }

        impl $name {
            pub fn curve(&self) -> Curve {
                self.0.curve()
            }
        }

        impl Processor for $name {
            fn name(&self) -> &str {
                self.0.name()
            }
            fn params(&self) -> &[ParamInfo] {
                self.0.params()
            }
            fn param(&self, id: ParamId) -> f32 {
                self.0.param(id)
            }
            fn set_param(&mut self, id: ParamId, value: f32) {
                self.0.set_param(id, value)
            }
            fn latency(&self) -> usize {
                self.0.latency()
            }
            fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
                self.0.prepare(sample_rate, max_block_frames)
            }
            fn reset(&mut self) {
                self.0.reset()
            }
            fn process(&mut self, buffer: &mut Buffer) {
                self.0.process(buffer)
            }
        }
    };
}

drive_pedal!(
    /// A soft overdrive: highpass, drive, a gentle curve, tone, level.
    Overdrive,
    OVERDRIVE
);
drive_pedal!(
    /// A distortion: more gain, a mid boost into a hard curve.
    Distortion,
    DISTORTION
);
drive_pedal!(
    /// A fuzz: the most gain into square, lopsided or octave curves.
    Fuzz,
    FUZZ
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    fn sine(frequency: f32, n: usize, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amplitude * (TAU * frequency * i as f32 / 48_000.0).sin())
            .collect()
    }

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|x| x * x).sum::<f32>() / buffer.len() as f32).sqrt()
    }

    fn pedals() -> Vec<Box<dyn Processor>> {
        vec![
            Box::new(Overdrive::default()),
            Box::new(Distortion::default()),
            Box::new(Fuzz::default()),
        ]
    }

    #[test]
    fn silence_stays_silent_and_latency_is_the_oversamplers() {
        for mut pedal in pedals() {
            let out = Renderer::default().render(pedal.as_mut(), &[vec![0.0; 2048]]);
            assert!(
                out[0].iter().all(|s| s.abs() < 1e-4),
                "{} is not silent on silence",
                pedal.name()
            );
            assert_eq!(pedal.latency(), crate::oversample::LATENCY);
            assert_eq!(pedal.params().len(), 5);
        }
    }

    #[test]
    fn output_is_bounded_by_level() {
        for mut pedal in pedals() {
            pedal.set_param(LEVEL, 0.0);
            let out = Renderer::default().render(pedal.as_mut(), &[sine(220.0, 9600, 10.0)]);
            let peak = out[0][4800..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
            // The curves stay under 1; the tone filter and the
            // oversampler's filters ring a little on the clipped edges.
            assert!(peak <= 1.3, "{} peak {peak}", pedal.name());
            assert!(
                peak > 0.7,
                "{} peak {peak}: should be driven hard",
                pedal.name()
            );
        }
    }

    #[test]
    fn more_drive_means_more_compression() {
        let mut quiet = Overdrive::default();
        quiet.set_param(DRIVE, 0.0);
        quiet.set_param(LEVEL, 0.0);
        let mut hot = Overdrive::default();
        hot.set_param(DRIVE, 40.0);
        hot.set_param(LEVEL, 0.0);
        let input = sine(220.0, 9600, 0.05);
        let a = Renderer::default().render(&mut quiet, std::slice::from_ref(&input));
        let b = Renderer::default().render(&mut hot, std::slice::from_ref(&input));
        let (a, b) = (&a[0][4800..], &b[0][4800..]);
        assert!(rms(b) > rms(a) * 5.0, "quiet {} hot {}", rms(a), rms(b));
        assert!(rms(b) < 1.0, "hot path {}", rms(b));
    }

    #[test]
    fn the_three_pedals_and_their_curves_sound_different() {
        let input = sine(110.0, 9600, 0.2);
        let mut renders: Vec<(String, Vec<f32>)> = Vec::new();
        for mut pedal in pedals() {
            pedal.set_param(LEVEL, 0.0);
            for curve in 0..3 {
                pedal.set_param(CURVE, curve as f32);
                let out = Renderer::default().render(pedal.as_mut(), std::slice::from_ref(&input));
                renders.push((
                    format!("{} curve {curve}", pedal.name()),
                    out[0][4800..].to_vec(),
                ));
            }
        }
        for (i, (a, x)) in renders.iter().enumerate() {
            for (b, y) in &renders[i + 1..] {
                let diff =
                    x.iter().zip(y).map(|(p, q)| (p - q).abs()).sum::<f32>() / x.len() as f32;
                assert!(diff > 0.01, "{a} and {b} render the same ({diff})");
            }
        }
    }

    #[test]
    fn oversampling_choice_changes_the_factor_but_not_the_latency() {
        let mut d = Distortion::default();
        assert_eq!(d.param(OVERSAMPLING), 2.0);
        d.prepare(48_000.0, 64);
        for index in [0.0, 1.0, 3.0] {
            d.set_param(OVERSAMPLING, index);
            assert_eq!(d.param(OVERSAMPLING), index);
            assert_eq!(d.latency(), crate::oversample::LATENCY);
        }
        // Off aliases audibly more than 8× on a bright tone through a
        // hard clip: compare the energy that is not at harmonics.
        let input = sine(5000.0, 48_000, 0.5);
        let render = |index: f32| {
            let mut d = Distortion::default();
            d.set_param(DRIVE, 40.0);
            d.set_param(TONE, 8000.0);
            d.set_param(OVERSAMPLING, index);
            Renderer::default().render(&mut d, std::slice::from_ref(&input))[0][4800..].to_vec()
        };
        let plain = render(0.0);
        let eight = render(3.0);
        let goertzel = |signal: &[f32], hz: f32| {
            let w = TAU * hz / 48_000.0;
            let coeff = 2.0 * w.cos() as f64;
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for (i, x) in signal.iter().enumerate() {
                let window = 0.5 - 0.5 * (TAU * i as f32 / signal.len() as f32).cos();
                let s0 = (x * window) as f64 + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            10.0 * (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(1e-30).log10() as f32
        };
        let alias_plain = goertzel(&plain, 3000.0) - goertzel(&plain, 5000.0);
        let alias_eight = goertzel(&eight, 3000.0) - goertzel(&eight, 5000.0);
        assert!(
            alias_eight < alias_plain - 20.0,
            "3 kHz alias: {alias_plain} dB off, {alias_eight} dB at 8x"
        );
    }

    #[test]
    fn tone_sweep_does_not_jump() {
        let mut od = Overdrive::default();
        od.prepare(48_000.0, 64);
        let mut buffer = sine(440.0, 48_000, 0.3);
        od.set_param(TONE, 500.0);
        for (i, block) in buffer.chunks_mut(64).enumerate() {
            if i == 100 {
                od.set_param(TONE, 8000.0);
            }
            let mut channels = [block];
            let mut b = Buffer::new(&mut channels);
            od.process(&mut b);
        }
        let max_step = buffer[6400..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.1, "largest step {max_step}");
    }

    #[test]
    fn saved_m0_overdrive_values_still_apply() {
        // A project saved before the drives had curves and oversampling
        // carries three values; the engine sets those and the rest
        // keep their defaults.
        let mut od = Overdrive::default();
        for (id, value) in [(0usize, 33.0f32), (1, 1234.0), (2, -3.0)] {
            od.set_param(id, value);
        }
        assert_eq!(od.param(0), 33.0);
        assert_eq!(od.param(1), 1234.0);
        assert_eq!(od.param(2), -3.0);
        assert_eq!(od.curve(), Curve::Soft);
        assert_eq!(od.param(OVERSAMPLING), 2.0);
    }
}
