//! A guitar amplifier: a driven preamp, a three-knob tone stack and a
//! presence control, meant to feed the cabinet.

use crate::biquad::Biquad;
use crate::db_to_linear;
use crate::oversample::Oversampler;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::shaper::Curve;
use crate::smooth::Smoother;

static PARAMS: [ParamInfo; 6] = [
    ParamInfo::new("Gain", Unit::Decibels, 0.0, 40.0, 15.0),
    ParamInfo::new("Bass", Unit::Decibels, -12.0, 12.0, 0.0),
    ParamInfo::new("Mid", Unit::Decibels, -12.0, 12.0, 0.0),
    ParamInfo::new("Treble", Unit::Decibels, -12.0, 12.0, 0.0),
    ParamInfo::new("Presence", Unit::Decibels, -12.0, 12.0, 0.0),
    ParamInfo::new("Level", Unit::Decibels, -24.0, 12.0, -6.0),
];

const GAIN: ParamId = 0;
const BASS: ParamId = 1;
const MID: ParamId = 2;
const TREBLE: ParamId = 3;
const PRESENCE: ParamId = 4;
const LEVEL: ParamId = 5;

/// Where the tone stack's bands sit.
const BASS_HZ: f32 = 120.0;
const MID_HZ: f32 = 700.0;
const MID_Q: f32 = 0.8;
const TREBLE_HZ: f32 = 3000.0;
const PRESENCE_HZ: f32 = 5000.0;
const SHELF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
const PRE_HIGHPASS_HZ: f32 = 40.0;
const SMOOTHING_SECONDS: f32 = 0.01;
const OVERSAMPLING: usize = 4;

/// The amp. Highpass, gain, an oversampled lopsided clipper, bass, mid,
/// treble and presence, then the level.
pub struct Amp {
    gain: Smoother,
    level: Smoother,
    tone_db: [f32; 4],
    tone_dirty: bool,
    pre: [Biquad; MAX_CHANNELS],
    post: [Biquad; MAX_CHANNELS],
    /// Bass, mid, treble, presence per channel.
    stack: [[Biquad; 4]; MAX_CHANNELS],
    oversamplers: [Oversampler; MAX_CHANNELS],
    gains: Vec<(f32, f32)>,
    sample_rate: f32,
}

impl Default for Amp {
    fn default() -> Self {
        Self {
            gain: Smoother::new(db_to_linear(PARAMS[GAIN].default)),
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
            tone_db: [0.0; 4],
            tone_dirty: true,
            pre: [Biquad::identity(); MAX_CHANNELS],
            post: [Biquad::identity(); MAX_CHANNELS],
            stack: [[Biquad::identity(); 4]; MAX_CHANNELS],
            oversamplers: std::array::from_fn(|_| Oversampler::new(OVERSAMPLING)),
            gains: Vec::new(),
            sample_rate: 48_000.0,
        }
    }
}

impl Amp {
    fn update_stack(&mut self) {
        let sr = self.sample_rate;
        let [bass, mid, treble, presence] = self.tone_db;
        let mut designed = [Biquad::identity(); 4];
        designed[0].set_lowshelf(sr, BASS_HZ, bass, SHELF_Q);
        designed[1].set_peaking(sr, MID_HZ, mid, MID_Q);
        designed[2].set_highshelf(sr, TREBLE_HZ, treble, SHELF_Q);
        designed[3].set_highshelf(sr, PRESENCE_HZ, presence, 0.5);
        for stack in &mut self.stack {
            for (f, d) in stack.iter_mut().zip(&designed) {
                f.set_coefficients_from(d);
            }
        }
        self.tone_dirty = false;
    }
}

impl Processor for Amp {
    fn name(&self) -> &str {
        "Amp"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            GAIN => crate::linear_to_db(self.gain.target()),
            BASS | MID | TREBLE | PRESENCE => self.tone_db[id - BASS],
            LEVEL => crate::linear_to_db(self.level.target()),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = PARAMS.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            GAIN => self.gain.set_target(db_to_linear(value)),
            BASS | MID | TREBLE | PRESENCE => {
                if self.tone_db[id - BASS] != value {
                    self.tone_db[id - BASS] = value;
                    self.tone_dirty = true;
                }
            }
            LEVEL => self.level.set_target(db_to_linear(value)),
            _ => {}
        }
    }

    fn latency(&self) -> usize {
        self.oversamplers[0].latency()
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.gain, &mut self.level] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        for ch in 0..MAX_CHANNELS {
            self.pre[ch].set_highpass(self.sample_rate, PRE_HIGHPASS_HZ, SHELF_Q);
            self.post[ch].set_highpass(self.sample_rate, 20.0, SHELF_Q);
            self.oversamplers[ch].prepare(max_block_frames);
        }
        self.update_stack();
        self.gains = vec![(0.0, 0.0); max_block_frames];
        self.reset();
    }

    fn reset(&mut self) {
        for f in self.pre.iter_mut().chain(self.post.iter_mut()) {
            f.reset();
        }
        for f in self.stack.iter_mut().flatten() {
            f.reset();
        }
        for os in &mut self.oversamplers {
            os.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames > self.gains.len() {
            return;
        }
        if self.tone_dirty {
            self.update_stack();
        }
        for g in &mut self.gains[..frames] {
            *g = (self.gain.advance(), self.level.advance());
        }
        let channels = buffer.channels().min(MAX_CHANNELS);
        for ch in 0..channels {
            let lane = buffer.channel_mut(ch);
            for (s, (gain, _)) in lane.iter_mut().zip(&self.gains) {
                *s = self.pre[ch].tick(*s) * gain;
            }
            self.oversamplers[ch].process(lane, |up| {
                for s in up.iter_mut() {
                    *s = Curve::Asymmetric.shape(*s);
                }
            });
            for (s, (_, level)) in lane.iter_mut().zip(&self.gains) {
                let mut y = self.post[ch].tick(*s);
                for f in &mut self.stack[ch] {
                    y = f.tick(y);
                }
                *s = y * level;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    fn tone(hz: f32, amplitude: f32) -> Vec<f32> {
        (0..24_000)
            .map(|i| amplitude * (TAU * hz * i as f32 / 48_000.0).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn has_the_knobs_and_the_oversamplers_latency() {
        let amp = Amp::default();
        let names: Vec<&str> = amp.params().iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(
            names,
            ["Gain", "Bass", "Mid", "Treble", "Presence", "Level"]
        );
        assert_eq!(amp.latency(), crate::oversample::LATENCY);
    }

    #[test]
    fn tone_knobs_act_in_their_bands() {
        // Quiet input and no gain, so the clipper is linear and the
        // stack can be measured.
        let quiet = |hz: f32| tone(hz, 0.01);
        let measure = |knob: ParamId, db: f32, hz: f32| {
            let mut amp = Amp::default();
            amp.set_param(GAIN, 0.0);
            amp.set_param(LEVEL, 0.0);
            amp.set_param(knob, db);
            let out = Renderer::default().render(&mut amp, &[quiet(hz)]);
            let mut flat = Amp::default();
            flat.set_param(GAIN, 0.0);
            flat.set_param(LEVEL, 0.0);
            let reference = Renderer::default().render(&mut flat, &[quiet(hz)]);
            20.0 * (rms(&out[0][12_000..]) / rms(&reference[0][12_000..])).log10()
        };
        assert!((measure(BASS, 9.0, 60.0) - 9.0).abs() < 1.0);
        assert!(measure(BASS, 9.0, 3000.0).abs() < 0.5);
        assert!((measure(MID, -9.0, 700.0) + 9.0).abs() < 1.0);
        assert!((measure(TREBLE, 6.0, 10_000.0) - 6.0).abs() < 1.0);
        assert!(measure(TREBLE, 6.0, 100.0).abs() < 0.5);
        assert!(measure(PRESENCE, 6.0, 12_000.0) > 4.0);
        assert!(measure(PRESENCE, 6.0, 2000.0) < 2.5);
    }

    #[test]
    fn gain_drives_the_clipper() {
        let mut clean = Amp::default();
        clean.set_param(GAIN, 0.0);
        clean.set_param(LEVEL, 0.0);
        let mut hot = Amp::default();
        hot.set_param(GAIN, 40.0);
        hot.set_param(LEVEL, 0.0);
        let input = tone(220.0, 0.05);
        let a = Renderer::default().render(&mut clean, std::slice::from_ref(&input));
        let b = Renderer::default().render(&mut hot, std::slice::from_ref(&input));
        let (a, b) = (rms(&a[0][12_000..]), rms(&b[0][12_000..]));
        assert!(b > a * 5.0 && b < 1.0, "clean {a} hot {b}");
        let silence = Renderer::default().render(&mut hot, &[vec![0.0; 4096]]);
        assert!(silence[0].iter().all(|s| s.abs() < 1e-4));
    }
}
