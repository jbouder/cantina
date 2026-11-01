//! A speaker cabinet as an impulse response.

use crate::convolution::Convolver;
use crate::db_to_linear;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Level", Unit::Decibels, -24.0, 12.0, 0.0)];

const LEVEL: ParamId = 0;
const SMOOTHING_SECONDS: f32 = 0.01;

/// Convolves both channels with one impulse response. Without one it
/// passes audio through. The impulse is given at its own sample rate
/// and resampled to the stream's at `prepare`, and scaled to unit
/// energy so a cabinet does not change the overall level.
pub struct Cabinet {
    impulse: Vec<f32>,
    impulse_rate: f32,
    convolvers: [Convolver; MAX_CHANNELS],
    level: Smoother,
}

impl Default for Cabinet {
    fn default() -> Self {
        Self {
            impulse: Vec::new(),
            impulse_rate: 48_000.0,
            convolvers: std::array::from_fn(|_| Convolver::new()),
            level: Smoother::new(1.0),
        }
    }
}

impl Cabinet {
    /// Sets the impulse response, recorded at `sample_rate`. Off the
    /// audio thread; takes effect at the next `prepare`.
    pub fn set_impulse(&mut self, samples: &[f32], sample_rate: f32) {
        self.impulse = samples.to_vec();
        self.impulse_rate = sample_rate.max(1.0);
    }

    /// The impulse as the convolvers run it: at the prepared rate,
    /// scaled to unit energy.
    pub fn impulse(&self) -> &[f32] {
        self.convolvers[0].impulse()
    }

    fn resampled(&self, to_rate: f32) -> Vec<f32> {
        if self.impulse.is_empty() {
            return Vec::new();
        }
        let ratio = self.impulse_rate / to_rate;
        let frames = ((self.impulse.len() as f32 - 1.0) / ratio).floor() as usize + 1;
        let mut out: Vec<f32> = (0..frames)
            .map(|i| {
                let at = i as f32 * ratio;
                let whole = at as usize;
                let frac = at - whole as f32;
                let a = self.impulse[whole];
                let b = self.impulse.get(whole + 1).copied().unwrap_or(a);
                a + (b - a) * frac
            })
            .collect();
        let energy: f32 = out.iter().map(|s| s * s).sum();
        if energy > 0.0 {
            let scale = 1.0 / energy.sqrt();
            for s in &mut out {
                *s *= scale;
            }
        }
        out
    }
}

impl Processor for Cabinet {
    fn name(&self) -> &str {
        "Cabinet"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            LEVEL => crate::linear_to_db(self.level.target()),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == LEVEL {
            self.level
                .set_target(db_to_linear(PARAMS[LEVEL].clamp(value)));
        }
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.level.set_time(SMOOTHING_SECONDS, sample_rate as f32);
        self.level.snap(self.level.target());
        let impulse = self.resampled(sample_rate as f32);
        for c in &mut self.convolvers {
            c.set_impulse(&impulse);
            c.prepare(max_block_frames);
        }
    }

    fn reset(&mut self) {
        for c in &mut self.convolvers {
            c.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let channels = buffer.channels().min(MAX_CHANNELS);
        let frames = buffer.frames();
        for ch in 0..channels {
            self.convolvers[ch].process(buffer.channel_mut(ch));
        }
        if !self.level.is_settled() || self.level.current() != 1.0 {
            for frame in 0..frames {
                let level = self.level.advance();
                for ch in 0..channels {
                    buffer.channel_mut(ch)[frame] *= level;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;

    #[test]
    fn without_an_impulse_audio_passes_through() {
        let input = vec![(0..300)
            .map(|i| (i as f32 * 0.1).sin())
            .collect::<Vec<f32>>()];
        let out = Renderer::default().render(&mut Cabinet::default(), &input);
        assert_eq!(out, input);
    }

    #[test]
    fn convolves_both_channels_with_unit_energy_and_no_delay() {
        let mut cab = Cabinet::default();
        cab.set_impulse(&[2.0, 0.0, -1.0, 0.5], 48_000.0);
        let mut impulse = vec![0.0f32; 256];
        impulse[0] = 1.0;
        let out = Renderer::default().render(&mut cab, &[impulse.clone(), impulse]);
        let scale = 1.0 / (4.0f32 + 1.0 + 0.25).sqrt();
        for lane in &out {
            assert!((lane[0] - 2.0 * scale).abs() < 1e-6, "{}", lane[0]);
            assert!(lane[1].abs() < 1e-6);
            assert!((lane[2] + scale).abs() < 1e-6);
            assert!((lane[3] - 0.5 * scale).abs() < 1e-6);
            assert!(lane[4..].iter().all(|s| s.abs() < 1e-6));
        }
        assert_eq!(cab.latency(), 0);
    }

    #[test]
    fn an_impulse_at_another_rate_is_resampled() {
        let mut cab = Cabinet::default();
        // Ten samples at 96 kHz: five at 48 kHz.
        cab.set_impulse(&[1.0; 10], 96_000.0);
        cab.prepare(48_000.0, 64);
        assert_eq!(cab.impulse().len(), 5);
        assert!(cab
            .impulse()
            .iter()
            .all(|s| (s - 1.0 / 5f32.sqrt()).abs() < 1e-6));
    }
}
