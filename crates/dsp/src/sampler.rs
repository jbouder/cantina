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

/// Parameters each pad has after the sampler's own `Level`: its level
/// in dB, pan (−1 to 1, added to the kit's), tune in semitones, decay (a
/// share of the sample's natural length, 1 for all of it) and mute. Pad
/// `p`'s come at ids `1 + p * PAD_PARAMS ..`.
pub const PAD_PARAMS: usize = 5;
/// A pad's level, in dB.
pub const PAD_LEVEL: usize = 0;
/// A pad's pan, added to the kit's and kept within −1 to 1.
pub const PAD_PAN: usize = 1;
/// A pad's tune, in semitones.
pub const PAD_TUNE: usize = 2;
/// A pad's decay: 1 lets each hit ring out, less fades it sooner.
pub const PAD_DECAY: usize = 3;
/// A pad's mute.
pub const PAD_MUTE: usize = 4;

/// The parameter id of a pad's `which` (one of the `PAD_*` constants).
pub fn pad_param(pad: usize, which: usize) -> ParamId {
    1 + pad * PAD_PARAMS + which
}

/// One pad's mix: a smoothed level (silent when muted), and what new hits
/// on it take from the moment they start.
#[derive(Debug, Clone)]
struct PadMix {
    level_db: f32,
    mute: bool,
    level: Smoother,
    pan: f32,
    tune: f32,
    decay: f32,
}

impl PadMix {
    fn new() -> Self {
        Self {
            level_db: 0.0,
            mute: false,
            level: Smoother::new(1.0),
            pan: 0.0,
            tune: 0.0,
            decay: 1.0,
        }
    }

    fn retarget(&mut self) {
        let target = if self.mute {
            0.0
        } else {
            db_to_linear(self.level_db)
        };
        self.level.set_target(target);
    }
}

/// A hit: a pad, how hard, and the frame of the block it lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trigger {
    /// Index of the pad in the kit.
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
    /// The pad's decay envelope: 1, shrinking by `env_step` a frame when
    /// the pad's decay is under 1.
    env: f32,
    env_step: f32,
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
            env: 1.0,
            env_step: 1.0,
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
    /// `Level`, then each pad's mix parameters.
    params: Vec<ParamInfo>,
    mix: Vec<PadMix>,
    /// Each pad's level this frame, worked out once a frame.
    pad_gain: Vec<f32>,
}

impl Sampler {
    /// A sampler playing `kit`, at 48 kHz until it is prepared.
    pub fn new(kit: Arc<LoadedKit>) -> Self {
        Self {
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
            params: Self::param_list(&kit_pads_names(&kit)),
            mix: vec![PadMix::new(); kit.pads().len()],
            pad_gain: vec![1.0; kit.pads().len()],
            kit,
        }
    }

    fn param_list(pads: &[String]) -> Vec<ParamInfo> {
        let mut params = vec![PARAMS[LEVEL].clone()];
        for name in pads {
            let named = |what: &str| std::borrow::Cow::Owned(format!("{name} {what}"));
            params.push(ParamInfo {
                name: named("level"),
                ..ParamInfo::new("", Unit::Decibels, -60.0, 12.0, 0.0)
            });
            params.push(ParamInfo {
                name: named("pan"),
                ..ParamInfo::new("", Unit::Generic, -1.0, 1.0, 0.0)
            });
            params.push(ParamInfo {
                name: named("tune"),
                ..ParamInfo::new("", Unit::Generic, -12.0, 12.0, 0.0)
            });
            params.push(ParamInfo {
                name: named("decay"),
                ..ParamInfo::new("", Unit::Generic, 0.05, 1.0, 1.0)
            });
            params.push(ParamInfo {
                name: named("mute"),
                ..ParamInfo::toggle("", false)
            });
        }
        params
    }

    /// The kit it plays.
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
        let mix = &self.mix[trigger.pad];
        let pan = if mix.pan == 0.0 {
            pad.pan
        } else {
            (pad.pan + mix.pan).clamp(-1.0, 1.0)
        };
        let angle = (pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
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
            step: if mix.tune == 0.0 {
                layer.sample.sample_rate / self.sample_rate
            } else {
                layer.sample.sample_rate / self.sample_rate * 2f64.powf(f64::from(mix.tune) / 12.0)
            },
            gain_left,
            gain_right,
            gain_mono: gain,
            fade: 1.0,
            fade_step: 0.0,
            choke: pad.choke,
            serial: self.next_serial,
            env: 1.0,
            // Under 1, the hit is down 60 dB by that share of its length.
            env_step: if mix.decay >= 1.0 {
                1.0
            } else {
                let frames = (layer.sample.frames() as f64 * f64::from(mix.decay)).max(1.0);
                10f32.powf(-3.0 / frames as f32)
            },
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

/// The kit's pad names, for its parameter names.
fn kit_pads_names(kit: &LoadedKit) -> Vec<String> {
    kit.pads().iter().map(|p| p.name.clone()).collect()
}

impl Processor for Sampler {
    fn name(&self) -> &str {
        "Sampler"
    }

    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn param(&self, id: ParamId) -> f32 {
        if id == LEVEL {
            return linear_to_db(self.level.target());
        }
        let (pad, which) = ((id - 1) / PAD_PARAMS, (id - 1) % PAD_PARAMS);
        let Some(mix) = self.mix.get(pad) else {
            return 0.0;
        };
        match which {
            PAD_LEVEL => mix.level_db,
            PAD_PAN => mix.pan,
            PAD_TUNE => mix.tune,
            PAD_DECAY => mix.decay,
            _ => f32::from(u8::from(mix.mute)),
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == LEVEL {
            self.level
                .set_target(db_to_linear(PARAMS[LEVEL].clamp(value)));
            return;
        }
        let clamped = self.params.get(id).map_or(value, |p| p.clamp(value));
        let (pad, which) = ((id - 1) / PAD_PARAMS, (id - 1) % PAD_PARAMS);
        let Some(mix) = self.mix.get_mut(pad) else {
            return;
        };
        match which {
            PAD_LEVEL => mix.level_db = clamped,
            PAD_PAN => mix.pan = clamped,
            PAD_TUNE => mix.tune = clamped,
            PAD_DECAY => mix.decay = clamped,
            _ => mix.mute = clamped >= 0.5,
        }
        mix.retarget();
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate;
        self.choke_frames = CHOKE_SECONDS * sample_rate as f32;
        self.level.set_time(SMOOTHING_SECONDS, sample_rate as f32);
        self.level.snap(self.level.target());
        for mix in &mut self.mix {
            mix.level.set_time(SMOOTHING_SECONDS, sample_rate as f32);
            mix.level.snap(mix.level.target());
        }
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
            for (gain, mix) in self.pad_gain.iter_mut().zip(self.mix.iter_mut()) {
                *gain = mix.level.advance();
            }
            for voice in self.voices.iter_mut() {
                if !voice.active {
                    continue;
                }
                let sample: &Sample = &kit.pads()[voice.pad].layers[voice.layer].sample;
                let pad_gain = self.pad_gain.get(voice.pad).copied().unwrap_or(1.0);
                let fade = voice.fade * level * pad_gain * voice.env;
                voice.env *= voice.env_step;
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

    /// The kick's hard layer through the mono mix, with the pad mixer set.
    fn kick(set: &[(usize, f32)], frames: usize) -> Vec<f32> {
        let mut s = Sampler::new(kit());
        for &(which, value) in set {
            s.set_param(pad_param(0, which), value);
        }
        s.prepare(SR, 64);
        s.trigger(0, 1.0, 0);
        let mut mono = vec![0.0f32; frames];
        let mut lanes = [&mut mono[..]];
        s.process(&mut Buffer::new(&mut lanes));
        mono
    }

    #[test]
    fn the_pad_mixer_sets_level_mute_tune_and_decay_per_pad() {
        let plain = kick(&[], 4);
        assert!(plain[0] > 0.0);
        // −6 dB is half, set before prepare so it starts there.
        let quieter = kick(&[(PAD_LEVEL, -6.0206)], 4);
        assert!(
            (quieter[0] / plain[0] - 0.5).abs() < 1e-3,
            "{} {}",
            quieter[0],
            plain[0]
        );
        // Muted: silent.
        assert!(kick(&[(PAD_MUTE, 1.0)], 4).iter().all(|v| *v == 0.0));
        // Defaults are exactly the plain sampler.
        assert_eq!(
            kick(&[(PAD_LEVEL, 0.0), (PAD_TUNE, 0.0), (PAD_DECAY, 1.0)], 4),
            plain
        );
        let mut s = Sampler::new(kit());
        s.set_param(pad_param(1, PAD_TUNE), 7.0);
        assert_eq!(s.param(pad_param(1, PAD_TUNE)), 7.0);
        s.set_param(pad_param(1, PAD_TUNE), 40.0);
        assert_eq!(s.param(pad_param(1, PAD_TUNE)), 12.0, "clamped");
        assert!(s.params()[pad_param(1, PAD_LEVEL)].name.contains("level"));
    }

    #[test]
    fn a_mute_mid_hit_fades_rather_than_clicks() {
        let mut s = Sampler::new(kit());
        s.prepare(SR, 2048);
        s.trigger(2, 1.0, 0);
        let (before, _) = render(&mut s, 64);
        assert!(before[63].abs() > 0.0);
        s.set_param(pad_param(2, PAD_MUTE), 1.0);
        let (after, _) = render(&mut s, 2048);
        let worst = std::iter::once(before[63])
            .chain(after.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.05, "{worst}");
        assert!(after[1500..].iter().all(|v| v.abs() < 1e-4), "faded out");
    }

    #[test]
    fn tune_and_decay_change_new_hits() {
        // The slow pad's sample: an octave up plays it in half the frames.
        let mut s = Sampler::new(kit());
        s.set_param(pad_param(3, PAD_TUNE), 12.0);
        s.prepare(SR, 64);
        s.trigger(3, 1.0, 0);
        let (up, _) = render(&mut s, 64);
        let mut t = Sampler::new(kit());
        t.prepare(SR, 64);
        t.trigger(3, 1.0, 0);
        let (plain, _) = render(&mut t, 64);
        let len = |v: &[f32]| v.iter().rposition(|x| x.abs() > 1e-6).map_or(0, |i| i + 1);
        assert!(len(&up) < len(&plain), "{} {}", len(&up), len(&plain));
        // A short decay dies sooner than the sample does.
        let mut d = Sampler::new(kit());
        d.set_param(pad_param(2, PAD_DECAY), 0.1);
        d.prepare(SR, 64);
        d.trigger(2, 1.0, 0);
        let (short, _) = render(&mut d, 64);
        let mut e = Sampler::new(kit());
        e.prepare(SR, 64);
        e.trigger(2, 1.0, 0);
        let (long, _) = render(&mut e, 64);
        assert!(
            short[40].abs() < long[40].abs() * 0.5,
            "{} {}",
            short[40],
            long[40]
        );
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
        assert_eq!(s.params().len(), 1 + s.kit().pads().len() * PAD_PARAMS);
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
