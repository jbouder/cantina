//! A polyphonic drum sampler: plays the pads of a [`LoadedKit`] on
//! sample-accurate triggers.
//!
//! The sequencer (in the engine crate) decides *when*: it queues a
//! [`Trigger`] per hit with the frame inside the next block it lands on,
//! and `process` fires each one at exactly that frame. Everything else is
//! here: which velocity layer answers, the velocity as gain, the pad's
//! gain and pan, choke groups, playing a sample recorded at one rate in a
//! project at another, and a fixed pool of voices so nothing is allocated
//! on the audio thread.
//!
//! Velocity maps to gain linearly: a hit at 0.5 is half the amplitude of
//! one at 1.0. A pan is constant power, so a mono sample sits at −3 dB on
//! each side when centred; a stereo sample plays at unity when centred.

use std::sync::Arc;

use crate::kit::{LoadedKit, Sample};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit};
use crate::smooth::Smoother;
use crate::{db_to_linear, linear_to_db};

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Level", Unit::Decibels, -60.0, 12.0, 0.0)];

const LEVEL: ParamId = 0;
const SMOOTHING_SECONDS: f32 = 0.005;

/// Voices that can ring at once. The oldest is stolen beyond this.
pub const MAX_VOICES: usize = 32;
/// Triggers that can wait for one block. Beyond this they are dropped
/// and counted.
pub const MAX_TRIGGERS: usize = 64;
/// How long a choked voice takes to fade out, so a cut never clicks.
pub const CHOKE_SECONDS: f32 = 0.003;

/// A hit: a pad, how hard, and the frame of the block it lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trigger {
    pub pad: usize,
    /// 0 to 1.
    pub velocity: f32,
    /// Frame within the next block. Past the block's end lands on its
    /// last frame.
    pub frame: usize,
}

#[derive(Debug, Clone, Copy)]
struct Voice {
    active: bool,
    pad: usize,
    layer: usize,
    /// Frame into the sample, fractional while resampling.
    position: f64,
    /// Frames of the sample per frame of output.
    step: f64,
    gain_left: f32,
    gain_right: f32,
    /// Gain for a one-channel buffer.
    gain_mono: f32,
    /// 1 while playing; counts down once choked.
    fade: f32,
    fade_step: f32,
    choke: Option<usize>,
    /// Trigger order, for stealing the oldest.
    serial: u64,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            pad: 0,
            layer: 0,
            position: 0.0,
            step: 1.0,
            gain_left: 0.0,
            gain_right: 0.0,
            gain_mono: 0.0,
            fade: 1.0,
            fade_step: 0.0,
            choke: None,
            serial: 0,
        }
    }
}

/// The sampler. Build one with a kit, queue triggers, process.
pub struct Sampler {
    kit: Arc<LoadedKit>,
    level: Smoother,
    sample_rate: f64,
    voices: [Voice; MAX_VOICES],
    triggers: [Trigger; MAX_TRIGGERS],
    pending: usize,
    next_serial: u64,
    dropped: u64,
    choke_frames: f32,
}

impl Sampler {
    pub fn new(kit: Arc<LoadedKit>) -> Self {
        Self {
            kit,
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
            sample_rate: 48_000.0,
            voices: [Voice::default(); MAX_VOICES],
            triggers: [Trigger {
                pad: 0,
                velocity: 0.0,
                frame: 0,
            }; MAX_TRIGGERS],
            pending: 0,
            next_serial: 1,
            dropped: 0,
            choke_frames: CHOKE_SECONDS * 48_000.0,
        }
    }

    pub fn kit(&self) -> &Arc<LoadedKit> {
        &self.kit
    }

    /// Queues a hit for the next `process`. Returns false, and plays
    /// nothing, for an unknown pad, a velocity of zero or a full queue.
    /// Real-time safe.
    pub fn trigger(&mut self, pad: usize, velocity: f32, frame: usize) -> bool {
        if pad >= self.kit.pads().len() || velocity.is_nan() || velocity <= 0.0 {
            return false;
        }
        if self.pending >= MAX_TRIGGERS {
            self.dropped += 1;
            return false;
        }
        self.triggers[self.pending] = Trigger {
            pad,
            velocity: velocity.min(1.0),
            frame,
        };
        self.pending += 1;
        true
    }

    /// Voices ringing now.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Triggers refused because the queue was full.
    pub fn dropped_triggers(&self) -> u64 {
        self.dropped
    }

    /// Silences every voice at once. Off the audio thread, or when a
    /// click does not matter.
    pub fn all_off(&mut self) {
        for voice in &mut self.voices {
            voice.active = false;
        }
        self.pending = 0;
    }

    fn start(&mut self, trigger: Trigger) {
        let Some(pad) = self.kit.pads().get(trigger.pad) else {
            return;
        };
        if let Some(group) = pad.choke {
            for voice in self.voices.iter_mut() {
                if voice.active && voice.choke == Some(group) && voice.fade_step == 0.0 {
                    voice.fade_step = 1.0 / self.choke_frames.max(1.0);
                }
            }
        }
        let layer_index = pad.layer_for(trigger.velocity);
        let layer = &pad.layers[layer_index];
        let gain = trigger.velocity * pad.gain * layer.gain;
        // Constant power: −3 dB a side at centre for mono.
        let angle = (pad.pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
        // cos(π/2) is not quite zero in f32; a hard pan should be silent
        // on the far side.
        let exact = |v: f32| if v.abs() < 1e-6 { 0.0 } else { v };
        let (mut gain_left, mut gain_right) =
            (exact(angle.cos()) * gain, exact(angle.sin()) * gain);
        if layer.sample.channels.len() == 2 {
            // Unity at centre for stereo: the pan is a balance.
            gain_left *= std::f32::consts::SQRT_2;
            gain_right *= std::f32::consts::SQRT_2;
        }
        let slot = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => {
                // Steal the oldest.
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.serial)
                    .map_or(0, |(i, _)| i)
            }
        };
        self.voices[slot] = Voice {
            active: true,
            pad: trigger.pad,
            layer: layer_index,
            position: 0.0,
            step: layer.sample.sample_rate / self.sample_rate,
            gain_left,
            gain_right,
            gain_mono: gain,
            fade: 1.0,
            fade_step: 0.0,
            choke: pad.choke,
            serial: self.next_serial,
        };
        self.next_serial += 1;
    }
}

/// The sample at a fractional frame, linearly interpolated; silence past
/// the end.
#[inline]
fn read(channel: &[f32], position: f64) -> f32 {
    let i = position as usize;
    if i + 1 < channel.len() {
        let t = (position - i as f64) as f32;
        channel[i] + (channel[i + 1] - channel[i]) * t
    } else if i < channel.len() {
        channel[i] * (1.0 - (position - i as f64) as f32)
    } else {
        0.0
    }
}

impl Processor for Sampler {
    fn name(&self) -> &str {
        "Sampler"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            LEVEL => linear_to_db(self.level.target()),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == LEVEL {
            self.level
                .set_target(db_to_linear(PARAMS[LEVEL].clamp(value)));
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate;
        self.choke_frames = CHOKE_SECONDS * sample_rate as f32;
        self.level.set_time(SMOOTHING_SECONDS, sample_rate as f32);
        self.level.snap(self.level.target());
        self.all_off();
    }

    fn reset(&mut self) {
        self.all_off();
    }

    /// Adds the ringing voices, and the queued hits from their frames on,
    /// to the buffer.
    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames == 0 {
            self.pending = 0;
            return;
        }
        let channels = buffer.channels();
        // Triggers in frame order; the queue is small and nearly sorted.
        let n = self.pending;
        let pending = &mut self.triggers[..n];
        for i in 1..n {
            let mut j = i;
            while j > 0 && pending[j - 1].frame > pending[j].frame {
                pending.swap(j - 1, j);
                j -= 1;
            }
        }
        self.pending = 0;
        let mut next_trigger = 0;
        let kit = Arc::clone(&self.kit);
        for frame in 0..frames {
            while next_trigger < n {
                let trigger = self.triggers[next_trigger];
                if trigger.frame.min(frames - 1) != frame {
                    break;
                }
                self.start(trigger);
                next_trigger += 1;
            }
            let level = self.level.advance();
            for voice in self.voices.iter_mut() {
                if !voice.active {
                    continue;
                }
                let sample: &Sample = &kit.pads()[voice.pad].layers[voice.layer].sample;
                let fade = voice.fade * level;
                if channels >= 2 {
                    let (l, r) = match sample.channels.as_slice() {
                        [mono] => {
                            let s = read(mono, voice.position);
                            (s, s)
                        }
                        [left, right, ..] => {
                            (read(left, voice.position), read(right, voice.position))
                        }
                        [] => (0.0, 0.0),
                    };
                    buffer.channel_mut(0)[frame] += l * voice.gain_left * fade;
                    buffer.channel_mut(1)[frame] += r * voice.gain_right * fade;
                } else {
                    let s = match sample.channels.as_slice() {
                        [mono] => read(mono, voice.position),
                        [left, right, ..] => {
                            0.5 * (read(left, voice.position) + read(right, voice.position))
                        }
                        [] => 0.0,
                    };
                    buffer.channel_mut(0)[frame] += s * voice.gain_mono * fade;
                }
                voice.position += voice.step;
                if voice.fade_step > 0.0 {
                    voice.fade -= voice.fade_step;
                    if voice.fade <= 1e-4 {
                        voice.active = false;
                    }
                }
                if voice.position >= sample.frames() as f64 {
                    voice.active = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::Kit;

    const SR: f64 = 48_000.0;

    fn sample(values: Vec<f32>, rate: f64) -> Arc<Sample> {
        Arc::new(Sample {
            sample_rate: rate,
            channels: vec![values],
        })
    }

    fn kit() -> Arc<LoadedKit> {
        let kit = Kit::from_json(
            r#"{"version":1,"name":"t","pads":[
              {"id":"kick","layers":[
                {"file":"soft","velocity":[0,0.5]},{"file":"hard","velocity":[0.5,1]}]},
              {"id":"hat-c","choke":"hat","pan":1,"layers":[{"file":"long"}]},
              {"id":"hat-o","choke":"hat","pan":-1,"layers":[{"file":"long"}]},
              {"id":"slow","layers":[{"file":"half"}]}
            ]}"#,
        )
        .unwrap();
        Arc::new(
            LoadedKit::build(&kit, |file| {
                Ok(match file {
                    "soft" => sample(vec![0.25; 4], SR),
                    "hard" => sample(vec![1.0, 0.5, 0.25, 0.125], SR),
                    "long" => sample(vec![0.5; 1000], SR),
                    "half" => sample(vec![1.0, 0.0, 1.0, 0.0], SR / 2.0),
                    _ => unreachable!(),
                })
            })
            .unwrap(),
        )
    }

    fn render(sampler: &mut Sampler, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        {
            let mut lanes = [&mut left[..], &mut right[..]];
            let mut buffer = Buffer::new(&mut lanes);
            sampler.process(&mut buffer);
        }
        (left, right)
    }

    #[test]
    fn hits_land_on_their_frame_with_velocity_and_layer() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 64);
        assert!(s.trigger(0, 1.0, 10));
        assert!(s.trigger(0, 0.4, 20));
        assert!(!s.trigger(9, 1.0, 0), "unknown pad");
        assert!(!s.trigger(0, 0.0, 0), "silent hit");
        let (l, r) = render(&mut s, 64);
        let centre = std::f32::consts::FRAC_1_SQRT_2;
        assert!(l[..10].iter().all(|v| *v == 0.0));
        assert!((l[10] - 1.0 * centre).abs() < 1e-6, "{}", l[10]);
        assert!((l[11] - 0.5 * centre).abs() < 1e-6);
        assert!((l[13] - 0.125 * centre).abs() < 1e-6);
        assert_eq!(l[14], 0.0, "the hard sample is four frames long");
        assert!(
            (l[20] - 0.25 * 0.4 * centre).abs() < 1e-6,
            "soft layer at 0.4"
        );
        assert_eq!(l, r, "centred");
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn choke_group_fades_the_other_pad_and_pans_hard() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 512);
        s.trigger(2, 1.0, 0); // open hat, hard left
        s.trigger(1, 1.0, 100); // closed hat, hard right
        let (l, r) = render(&mut s, 512);
        assert!((l[0] - 0.5).abs() < 1e-6, "open hat on the left: {}", l[0]);
        assert!(r[0].abs() < 1e-6);
        assert!((l[99] - 0.5).abs() < 1e-6, "still ringing before the choke");
        let fade = (CHOKE_SECONDS * SR as f32) as usize;
        assert!(
            (l[100] - 0.5).abs() < 1e-6,
            "the fade starts after the hit's frame"
        );
        assert!(l[101] < 0.5, "fading from the next frame on");
        assert!(l[101] > 0.4, "not cut dead: {}", l[101]);
        assert_eq!(l[100 + fade + 1], 0.0, "gone after the fade");
        assert!((r[100] - 0.5).abs() < 1e-6, "closed hat on the right");
        assert_eq!(s.active_voices(), 1);
        // Hitting the same pad again chokes the earlier hit too.
        s.trigger(1, 1.0, 0);
        render(&mut s, 512);
        assert_eq!(s.active_voices(), 1);
    }

    #[test]
    fn polyphony_sums_and_the_oldest_voice_is_stolen() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 64);
        for _ in 0..MAX_VOICES {
            s.trigger(3, 1.0, 0);
        }
        render(&mut s, 1);
        assert_eq!(s.active_voices(), MAX_VOICES);
        s.trigger(0, 1.0, 0);
        render(&mut s, 1);
        assert_eq!(s.active_voices(), MAX_VOICES, "one was stolen, not added");
        assert!(s.voices.iter().any(|v| v.active && v.pad == 0));
        for _ in 0..MAX_TRIGGERS {
            assert!(s.trigger(3, 1.0, 0));
        }
        assert!(!s.trigger(3, 1.0, 0));
        assert_eq!(s.dropped_triggers(), 1);
        s.all_off();
        assert_eq!(s.active_voices(), 0);
        s.trigger(3, 1.0, 0);
        s.trigger(3, 1.0, 0);
        let (l, _) = render(&mut s, 4);
        assert!(
            (l[0] - 2.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
            "two voices sum"
        );
    }

    #[test]
    fn samples_at_another_rate_are_resampled() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 64);
        s.trigger(3, 1.0, 0);
        let (l, _) = render(&mut s, 16);
        let c = std::f32::consts::FRAC_1_SQRT_2;
        // 24 kHz sample [1, 0, 1, 0] at 48 kHz: half a frame per frame.
        let expected = [1.0, 0.5, 0.0, 0.5, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0];
        for (i, e) in expected.iter().enumerate() {
            assert!((l[i] - e * c).abs() < 1e-6, "frame {i}: {} vs {e}", l[i]);
        }
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn mono_buffer_gets_the_mono_mix_and_level_applies() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 64);
        s.set_param(LEVEL, -6.0206);
        s.prepare(SR, 64);
        assert!((s.param(LEVEL) + 6.0206).abs() < 1e-3);
        s.trigger(0, 1.0, 2);
        let mut mono = [0.0f32; 8];
        {
            let mut lanes = [&mut mono[..]];
            s.process(&mut Buffer::new(&mut lanes));
        }
        assert!((mono[2] - 0.5).abs() < 1e-6, "{}", mono[2]);
        assert_eq!(mono[1], 0.0);
        assert_eq!(s.name(), "Sampler");
        assert_eq!(s.params().len(), 1);
        // A trigger past the block lands on its last frame; an empty
        // block drops the queue.
        s.trigger(0, 1.0, 999);
        let (l, _) = render(&mut s, 4);
        assert!(l[3] != 0.0);
        s.all_off();
        s.trigger(0, 1.0, 0);
        render(&mut s, 0);
        let (l, _) = render(&mut s, 4);
        assert!(l.iter().all(|v| *v == 0.0));
    }
}
