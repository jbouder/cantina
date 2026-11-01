use crate::db_to_linear;
use crate::envelope::Envelope;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::readout::Readout;

static PARAMS: [ParamInfo; 4] = [
    ParamInfo::new("Threshold", Unit::Decibels, -80.0, 0.0, -50.0),
    ParamInfo::new("Attack", Unit::Milliseconds, 0.1, 100.0, 1.0),
    ParamInfo::new("Hold", Unit::Milliseconds, 0.0, 1000.0, 50.0),
    ParamInfo::new("Release", Unit::Milliseconds, 1.0, 2000.0, 100.0),
];

const THRESHOLD: ParamId = 0;
const ATTACK: ParamId = 1;
const HOLD: ParamId = 2;
const RELEASE: ParamId = 3;

static READOUTS: [&str; 1] = ["Open"];

/// The gate opens at the threshold and closes this far below it, so a
/// note hovering around the threshold does not flutter the gate.
const HYSTERESIS_DB: f32 = 6.0;
/// How fast the detector follows a rising and a falling signal.
const DETECTOR_ATTACK_SECONDS: f32 = 0.0005;
const DETECTOR_RELEASE_SECONDS: f32 = 0.02;

/// A noise gate: silence below a threshold, with attack, hold and
/// release so the edges are not heard. The level is taken from the
/// loudest channel, so a stereo pair opens and closes together.
#[derive(Debug, Clone)]
pub struct NoiseGate {
    values: [f32; 4],
    detector: Envelope,
    /// The gain the audio is multiplied by, easing between 0 and 1.
    gain: f32,
    attack_coeff: f32,
    release_coeff: f32,
    open: bool,
    hold_samples: u32,
    hold_remaining: u32,
    sample_rate: f32,
    readout: Readout,
}

impl Default for NoiseGate {
    fn default() -> Self {
        Self {
            values: [
                PARAMS[THRESHOLD].default,
                PARAMS[ATTACK].default,
                PARAMS[HOLD].default,
                PARAMS[RELEASE].default,
            ],
            detector: Envelope::new(),
            gain: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            open: false,
            hold_samples: 0,
            hold_remaining: 0,
            sample_rate: 48_000.0,
            readout: Readout::new(&READOUTS),
        }
    }
}

impl NoiseGate {
    /// Whether the gate is open now.
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn update_times(&mut self) {
        let sr = self.sample_rate;
        self.attack_coeff = Envelope::coefficient(self.values[ATTACK] / 1000.0, sr);
        self.release_coeff = Envelope::coefficient(self.values[RELEASE] / 1000.0, sr);
        self.hold_samples = (self.values[HOLD] / 1000.0 * sr) as u32;
    }
}

impl Processor for NoiseGate {
    fn name(&self) -> &str {
        "Gate"
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
        self.detector.set_times(
            DETECTOR_ATTACK_SECONDS,
            DETECTOR_RELEASE_SECONDS,
            self.sample_rate,
        );
        self.update_times();
        self.reset();
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.gain = 0.0;
        self.open = false;
        self.hold_remaining = 0;
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let open_at = db_to_linear(self.values[THRESHOLD]);
        let close_at = db_to_linear(self.values[THRESHOLD] - HYSTERESIS_DB);
        let channels = buffer.channels().min(MAX_CHANNELS);
        for frame in 0..buffer.frames() {
            let mut peak = 0.0f32;
            for ch in 0..channels {
                peak = peak.max(buffer.channel(ch)[frame].abs());
            }
            let level = self.detector.tick(peak);
            if self.open {
                if level < close_at {
                    if self.hold_remaining > 0 {
                        self.hold_remaining -= 1;
                    } else {
                        self.open = false;
                    }
                } else {
                    self.hold_remaining = self.hold_samples;
                }
            } else if level >= open_at {
                self.open = true;
                self.hold_remaining = self.hold_samples;
            }
            let (target, coeff) = if self.open {
                (1.0, self.attack_coeff)
            } else {
                (0.0, self.release_coeff)
            };
            self.gain = target + (self.gain - target) * coeff;
            for ch in 0..channels {
                buffer.channel_mut(ch)[frame] *= self.gain;
            }
        }
        self.readout.set(0, if self.open { 1.0 } else { 0.0 });
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

    fn sine(hz: f32, seconds: f32, amplitude: impl Fn(f32) -> f32) -> Vec<f32> {
        let sr = 48_000.0;
        (0..(seconds * sr) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                amplitude(t) * (TAU * hz * t).sin()
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn closes_on_quiet_and_opens_on_loud() {
        let mut gate = NoiseGate::default();
        gate.set_param(THRESHOLD, -30.0);
        // Half a second at -50 dB, then half a second at -10 dB.
        let input = sine(220.0, 1.0, |t| if t < 0.5 { 0.003 } else { 0.3 });
        let out = Renderer::default().render(&mut gate, &[input]);
        let quiet = &out[0][..24_000];
        let loud = &out[0][30_000..];
        assert!(rms(quiet) < 1e-5, "quiet part leaked: {}", rms(quiet));
        assert!(
            (rms(loud) - 0.3 / 2f32.sqrt()).abs() < 0.01,
            "loud part {}",
            rms(loud)
        );
        assert!(gate.is_open());
        assert_eq!(gate.readout().unwrap().get(0), 1.0);
    }

    #[test]
    fn does_not_chatter_on_a_wobbling_sustained_note() {
        let mut gate = NoiseGate::default();
        gate.set_param(THRESHOLD, -30.0);
        gate.prepare(48_000.0, 64);
        // A note that swells past the threshold, then hovers 2 dB either
        // side of it at 6 Hz while slowly dying away: a real sustained
        // note with vibrato. Without hysteresis and hold this flutters.
        let thr = db_to_linear(-30.0);
        let input = sine(110.0, 3.0, |t| {
            if t < 0.1 {
                thr * 4.0
            } else {
                let wobble = db_to_linear(2.0 * (TAU * 6.0 * t).sin());
                thr * 1.2 * wobble * (-(t - 0.1) * 0.4).exp()
            }
        });
        let mut transitions = 0;
        let mut was_open = false;
        let mut lanes = vec![input];
        Renderer::default().run(&mut lanes, |buffer| {
            gate.process(buffer);
            if gate.is_open() != was_open {
                transitions += 1;
                was_open = gate.is_open();
            }
        });
        // Open once; close at most once as the note dies.
        assert!(transitions <= 2, "gate changed state {transitions} times");
        assert!(
            !gate.is_open(),
            "the dying note should have closed the gate"
        );
    }

    #[test]
    fn attack_and_release_shape_the_edges() {
        let mut gate = NoiseGate::default();
        gate.set_param(THRESHOLD, -30.0);
        gate.set_param(ATTACK, 20.0);
        gate.set_param(HOLD, 0.0);
        gate.set_param(RELEASE, 200.0);
        let input: Vec<f32> = (0..48_000)
            .map(|i| {
                if (4800..24_000).contains(&i) {
                    0.5
                } else {
                    0.0
                }
            })
            .collect();
        let out = Renderer::default().render(&mut gate, &[input]);
        let out = &out[0];
        // The attack eases in: after one time constant (20 ms) the gain
        // is about 63%.
        let after_attack = out[4800 + 960];
        assert!((0.25..0.4).contains(&after_attack), "{after_attack}");
        assert!(out[20_000] > 0.49);
        // The step down leaves a tail that decays over the release.
        assert!(
            out[24_000 + 100] < 1e-3,
            "input went silent; {}",
            out[24_000 + 100]
        );
        let decays = out[24_000..].windows(2).all(|w| w[1] <= w[0] + 1e-6);
        assert!(decays);
    }
}
