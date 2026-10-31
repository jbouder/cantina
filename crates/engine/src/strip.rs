//! A mixer strip: gain, pan and mute on one node, each smoothed so a
//! fader move or a mute never clicks. The shell puts one at the end of
//! every track and one on the master.

use cantina_dsp::{db_to_linear, Buffer, ParamId, ParamInfo, Smoother, Unit};

use crate::graph::{BlockContext, Node};

static PARAMS: [ParamInfo; 3] = [
    ParamInfo::new("Gain", Unit::Decibels, -60.0, 12.0, 0.0),
    ParamInfo::new("Pan", Unit::Generic, -1.0, 1.0, 0.0),
    ParamInfo::new("Mute", Unit::Generic, 0.0, 1.0, 0.0),
];

pub const GAIN: ParamId = 0;
pub const PAN: ParamId = 1;
pub const MUTE: ParamId = 2;

/// Time constant of the gain and pan smoothing, in seconds.
const SMOOTHING: f32 = 0.01;
/// Time constant of the mute fade.
const MUTE_FADE: f32 = 0.004;

/// Gain in decibels (parameter 0), pan from -1 left to 1 right (1) and
/// mute (2, 0 or 1). Pan is equal power: a centred signal sits 3 dB down
/// on each side so it is as loud as when panned hard. On a one-channel
/// buffer pan does nothing.
pub struct Strip {
    gain: Smoother,
    pan: Smoother,
    open: Smoother,
    db: f32,
}

impl Strip {
    pub fn new(db: f32, pan: f32, muted: bool) -> Self {
        let db = PARAMS[GAIN].clamp(db);
        Self {
            gain: Smoother::new(db_to_linear(db)),
            pan: Smoother::new(PARAMS[PAN].clamp(pan)),
            open: Smoother::new(if muted { 0.0 } else { 1.0 }),
            db,
        }
    }
}

impl Default for Strip {
    fn default() -> Self {
        Self::new(0.0, 0.0, false)
    }
}

/// Left and right factors for a pan position.
#[inline]
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
    (angle.cos(), angle.sin())
}

impl Node for Strip {
    fn name(&self) -> &str {
        "Strip"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            GAIN => self.db,
            PAN => self.pan.target(),
            MUTE => 1.0 - self.open.target(),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        match id {
            GAIN => {
                self.db = PARAMS[GAIN].clamp(value);
                self.gain.set_target(db_to_linear(self.db));
            }
            PAN => self.pan.set_target(PARAMS[PAN].clamp(value)),
            MUTE => self.open.set_target(if value >= 0.5 { 0.0 } else { 1.0 }),
            _ => {}
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        let sr = sample_rate as f32;
        self.gain.set_time(SMOOTHING, sr);
        self.pan.set_time(SMOOTHING, sr);
        self.open.set_time(MUTE_FADE, sr);
    }

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let stereo = buffer.channels() >= 2;
        if self.gain.is_settled() && self.pan.is_settled() && self.open.is_settled() {
            let g = self.gain.current() * self.open.current();
            let (l, r) = if stereo {
                pan_gains(self.pan.current())
            } else {
                (1.0, 1.0)
            };
            for (c, factor) in [l, r].into_iter().enumerate().take(buffer.channels()) {
                for s in buffer.channel_mut(c) {
                    *s *= g * factor;
                }
            }
            return;
        }
        for i in 0..frames {
            let g = self.gain.advance() * self.open.advance();
            let (l, r) = if stereo {
                pan_gains(self.pan.advance())
            } else {
                (1.0, 1.0)
            };
            buffer.channel_mut(0)[i] *= g * l;
            if stereo {
                buffer.channel_mut(1)[i] *= g * r;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoMap;

    fn ctx(tempo: &TempoMap, frames: usize) -> BlockContext<'_> {
        BlockContext {
            sample_rate: 48_000.0,
            frames,
            playing: false,
            position: 0,
            loop_range: None,
            tempo,
        }
    }

    fn run(strip: &mut Strip, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let tempo = TempoMap::default();
        let mut l = Vec::new();
        let mut r = Vec::new();
        for _ in 0..blocks {
            let mut a = vec![1.0f32; 64];
            let mut b = vec![1.0f32; 64];
            let mut lanes: [&mut [f32]; 2] = [&mut a, &mut b];
            let mut buffer = Buffer::new(&mut lanes);
            strip.process(&ctx(&tempo, 64), &mut buffer);
            l.extend_from_slice(&a);
            r.extend_from_slice(&b);
        }
        (l, r)
    }

    #[test]
    fn centre_is_equal_power_and_hard_pan_is_one_sided() {
        let mut strip = Strip::default();
        strip.prepare(48_000.0, 64);
        let (l, r) = run(&mut strip, 1);
        assert!((l[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert_eq!(l[0], r[0]);
        strip.set_param(PAN, 1.0);
        let (l, r) = run(&mut strip, 200);
        assert!(l.last().unwrap().abs() < 1e-4, "{:?}", l.last());
        assert!((r.last().unwrap() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn gain_and_mute_move_without_steps() {
        let mut strip = Strip::default();
        strip.prepare(48_000.0, 64);
        strip.set_param(GAIN, -20.0);
        strip.set_param(MUTE, 1.0);
        let (l, _) = run(&mut strip, 60);
        let max_step = l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(max_step < 0.02, "a sample jumped by {max_step}");
        assert!(l.last().unwrap().abs() < 1e-4);
        assert_eq!(strip.param(GAIN), -20.0);
        assert_eq!(strip.param(MUTE), 1.0);
        strip.set_param(MUTE, 0.0);
        let (l, _) = run(&mut strip, 200);
        let expected = db_to_linear(-20.0) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((l.last().unwrap() - expected).abs() < 1e-4);
    }

    #[test]
    fn mono_ignores_pan() {
        let mut strip = Strip::new(0.0, -1.0, false);
        strip.prepare(48_000.0, 64);
        let tempo = TempoMap::default();
        let mut a = vec![0.5f32; 16];
        let mut lanes: [&mut [f32]; 1] = [&mut a];
        let mut buffer = Buffer::new(&mut lanes);
        strip.process(&ctx(&tempo, 16), &mut buffer);
        assert_eq!(a[0], 0.5);
    }
}
