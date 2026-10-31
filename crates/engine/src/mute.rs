//! A node that lets audio through or silences it, with a short fade so
//! opening and closing never click. The shell uses it as the monitor
//! gate on an armed track.

use cantina_dsp::{Buffer, ParamId, ParamInfo, Smoother, Unit};

use crate::graph::{BlockContext, Node};

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Open", Unit::Generic, 0.0, 1.0, 1.0)];

/// Time constant of the fade between open and closed, in seconds; the
/// fade is done, to within a hundredth, after five of them.
const FADE: f32 = 0.002;

/// Passes audio when open, silence when closed. Parameter 0 is `Open`,
/// 0 or 1; values in between are a partial fade.
pub struct Mute {
    gain: Smoother,
}

impl Mute {
    pub fn new(open: bool) -> Self {
        Self {
            gain: Smoother::new(if open { 1.0 } else { 0.0 }),
        }
    }
}

impl Default for Mute {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Node for Mute {
    fn name(&self) -> &str {
        "Mute"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, _id: ParamId) -> f32 {
        self.gain.target()
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == 0 {
            self.gain.set_target(PARAMS[0].clamp(value));
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.gain.set_time(FADE, sample_rate as f32);
    }

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        if self.gain.is_settled() {
            let g = self.gain.current();
            if g == 0.0 {
                buffer.fill(0.0);
            } else if g != 1.0 {
                for lane in buffer.channels_mut() {
                    for s in lane.iter_mut() {
                        *s *= g;
                    }
                }
            }
            return;
        }
        let frames = buffer.frames();
        let channels = buffer.channels();
        for i in 0..frames {
            let g = self.gain.advance();
            for c in 0..channels {
                buffer.channel_mut(c)[i] *= g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoMap;

    #[test]
    fn closes_with_a_fade_and_then_silences() {
        let tempo = TempoMap::default();
        let ctx = BlockContext {
            sample_rate: 48_000.0,
            frames: 480,
            playing: false,
            position: 0,
            loop_range: None,
            tempo: &tempo,
        };
        let mut mute = Mute::new(true);
        mute.prepare(48_000.0, 480);
        mute.set_param(0, 0.0);
        let mut lane = [1.0f32; 480];
        let mut lanes: [&mut [f32]; 1] = [&mut lane[..]];
        mute.process(&ctx, &mut Buffer::new(&mut lanes));
        assert!(
            lane[0] < 1.0 && lane[0] > 0.9,
            "fade starts gently: {}",
            lane[0]
        );
        assert!(
            lane[479] < 0.01,
            "fade nearly done after 10 ms: {}",
            lane[479]
        );
        assert!(lane.windows(2).all(|w| w[1] <= w[0]), "monotonic fade");
        let mut lane = [1.0f32; 480];
        let mut lanes: [&mut [f32]; 1] = [&mut lane[..]];
        mute.process(&ctx, &mut Buffer::new(&mut lanes));
        // The tail of the fade: 20 ms in, the gain is below -60 dB.
        assert!(lane[0] < 0.01);
        assert!(lane[479] < 1e-3);
    }
}
