//! The metronome click, driven by the tempo map.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use cantina_dsp::{db_to_linear, linear_to_db, Buffer, ParamId, ParamInfo, Smoother, Unit};

use crate::graph::{BlockContext, Node};

/// The metronome's sounds, by the index the `Sound` parameter takes.
pub const SOUNDS: &[&str] = &["Standard", "Woodblock", "Cowbell", "Drum", "Hi-hat"];

static PARAMS: [ParamInfo; 3] = [
    ParamInfo::new("Level", Unit::Decibels, -40.0, 6.0, -6.0),
    ParamInfo::choice("Sound", SOUNDS, 0),
    ParamInfo::toggle("On", true),
];

/// The click's level in decibels.
pub const LEVEL: ParamId = 0;
/// Which of [`SOUNDS`] plays, by index.
pub const SOUND: ParamId = 1;
/// 1 to click while the transport plays, 0 for silence.
pub const ON: ParamId = 2;

const SMOOTHING_SECONDS: f32 = 0.005;

/// How one hit of a sound is made: up to two tones that decay together,
/// and a burst of high-passed noise that decays on its own.
#[derive(Debug, Clone, Copy)]
struct Recipe {
    /// Peak level of the hit.
    amplitude: f32,
    /// How long the hit rings, in seconds, before the tones are 60 dB down.
    seconds: f32,
    /// Two tones as (hertz, gain); a gain of zero leaves one out.
    tones: [(f32, f32); 2],
    /// Square waves instead of sines, low-passed at `lowpass_hz`.
    square: bool,
    lowpass_hz: f32,
    /// The first tone glides down to this many hertz over `sweep_seconds`.
    sweep_to_hz: f32,
    sweep_seconds: f32,
    /// Gain of the noise burst; zero for none.
    noise: f32,
    /// How long the noise takes to fall 60 dB.
    noise_seconds: f32,
    /// The noise is high-passed at this many hertz.
    noise_highpass_hz: f32,
}

impl Recipe {
    /// A plain decaying tone.
    const fn tone(hz: f32, amplitude: f32, seconds: f32) -> Self {
        Self {
            amplitude,
            seconds,
            tones: [(hz, 1.0), (0.0, 0.0)],
            square: false,
            lowpass_hz: 0.0,
            sweep_to_hz: hz,
            sweep_seconds: 0.0,
            noise: 0.0,
            noise_seconds: 0.0,
            noise_highpass_hz: 0.0,
        }
    }
}

/// The downbeat's recipe and every other beat's, for each of [`SOUNDS`].
const RECIPES: [(Recipe, Recipe); 5] = [
    // Standard: a short sine beep, higher on the downbeat.
    (
        Recipe::tone(1500.0, 1.0, 0.03),
        Recipe::tone(1000.0, 0.6, 0.03),
    ),
    // Woodblock: a hollow knock, two inharmonic partials and a tick of noise.
    (
        Recipe {
            tones: [(1250.0, 1.0), (3375.0, 0.3)],
            noise: 0.25,
            noise_seconds: 0.004,
            noise_highpass_hz: 2000.0,
            ..Recipe::tone(0.0, 0.65, 0.06)
        },
        Recipe {
            tones: [(950.0, 1.0), (2565.0, 0.3)],
            noise: 0.25,
            noise_seconds: 0.004,
            noise_highpass_hz: 2000.0,
            ..Recipe::tone(0.0, 0.39, 0.06)
        },
    ),
    // Cowbell: two detuned square waves, a fourth-ish apart.
    (
        Recipe {
            tones: [(660.0, 0.5), (980.0, 0.5)],
            square: true,
            lowpass_hz: 4000.0,
            ..Recipe::tone(0.0, 0.9, 0.2)
        },
        Recipe {
            tones: [(540.0, 0.5), (800.0, 0.5)],
            square: true,
            lowpass_hz: 4000.0,
            ..Recipe::tone(0.0, 0.55, 0.2)
        },
    ),
    // Drum: a kick on the downbeat, a snare on the others.
    (
        Recipe {
            sweep_to_hz: 50.0,
            sweep_seconds: 0.06,
            noise: 0.15,
            noise_seconds: 0.005,
            noise_highpass_hz: 1000.0,
            ..Recipe::tone(160.0, 0.9, 0.25)
        },
        Recipe {
            tones: [(190.0, 0.5), (0.0, 0.0)],
            noise: 0.8,
            noise_seconds: 0.12,
            noise_highpass_hz: 1000.0,
            ..Recipe::tone(0.0, 0.63, 0.12)
        },
    ),
    // Hi-hat: bright noise, longer and louder on the downbeat.
    (
        Recipe {
            tones: [(0.0, 0.0), (0.0, 0.0)],
            noise: 1.6,
            noise_seconds: 0.08,
            noise_highpass_hz: 7000.0,
            ..Recipe::tone(0.0, 0.9, 0.08)
        },
        Recipe {
            tones: [(0.0, 0.0), (0.0, 0.0)],
            noise: 1.6,
            noise_seconds: 0.035,
            noise_highpass_hz: 7000.0,
            ..Recipe::tone(0.0, 0.5, 0.035)
        },
    ),
];

/// The gain of a one-pole filter at `hz`: 1 passes everything.
fn one_pole(hz: f32, sample_rate: f32) -> f32 {
    if hz <= 0.0 {
        1.0
    } else {
        1.0 - (-TAU * hz / sample_rate).exp()
    }
}

/// Per-sample decay that falls 60 dB over `seconds`.
fn decay(seconds: f32, sample_rate: f32) -> f32 {
    let frames = (seconds * sample_rate).max(1.0);
    (0.001f32).powf(1.0 / frames)
}

/// One hit, retriggered on every beat. Everything it needs is set when
/// it triggers, so rendering only does arithmetic.
#[derive(Debug, Clone, Copy)]
struct Voice {
    remaining: usize,
    phases: [f32; 2],
    steps: [f32; 2],
    gains: [f32; 2],
    /// The first tone's step, multiplied each sample, stops at `sweep_end`.
    sweep: f32,
    sweep_end: f32,
    square: bool,
    lowpass: f32,
    lowpass_state: f32,
    envelope: f32,
    decay: f32,
    noise_envelope: f32,
    noise_decay: f32,
    highpass: f32,
    highpass_state: f32,
    rng: u32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            remaining: 0,
            phases: [0.0; 2],
            steps: [0.0; 2],
            gains: [0.0; 2],
            sweep: 1.0,
            sweep_end: 0.0,
            square: false,
            lowpass: 1.0,
            lowpass_state: 0.0,
            envelope: 0.0,
            decay: 0.0,
            noise_envelope: 0.0,
            noise_decay: 0.0,
            highpass: 1.0,
            highpass_state: 0.0,
            rng: 0x9E37_79B9,
        }
    }
}

impl Voice {
    fn trigger(&mut self, recipe: &Recipe, sample_rate: f32) {
        let rng = self.rng;
        *self = Self {
            rng,
            ..Self::default()
        };
        self.remaining = (recipe.seconds.max(recipe.noise_seconds) * sample_rate) as usize;
        for (i, (hz, gain)) in recipe.tones.iter().enumerate() {
            // Start at the peak so the click has an edge.
            self.phases[i] = FRAC_PI_2;
            self.steps[i] = TAU * hz / sample_rate;
            self.gains[i] = *gain;
        }
        let [(start_hz, _), _] = recipe.tones;
        if recipe.sweep_seconds > 0.0 && start_hz > 0.0 && recipe.sweep_to_hz > 0.0 {
            let frames = (recipe.sweep_seconds * sample_rate).max(1.0);
            self.sweep = (recipe.sweep_to_hz / start_hz).powf(1.0 / frames);
            self.sweep_end = TAU * recipe.sweep_to_hz / sample_rate;
        }
        self.square = recipe.square;
        self.lowpass = if recipe.square {
            one_pole(recipe.lowpass_hz, sample_rate)
        } else {
            1.0
        };
        self.envelope = recipe.amplitude;
        self.decay = decay(recipe.seconds, sample_rate);
        self.noise_envelope = recipe.amplitude * recipe.noise;
        self.noise_decay = decay(recipe.noise_seconds, sample_rate);
        self.highpass = one_pole(recipe.noise_highpass_hz, sample_rate);
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        // xorshift32: cheap, and the same every run.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    #[inline]
    fn render(&mut self) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        let mut tone = 0.0;
        for i in 0..2 {
            if self.gains[i] == 0.0 {
                continue;
            }
            let phase = self.phases[i];
            let wave = if self.square {
                if phase < PI || phase >= TAU {
                    1.0
                } else {
                    -1.0
                }
            } else {
                phase.sin()
            };
            tone += wave * self.gains[i];
            let mut next = phase + self.steps[i];
            while next >= TAU {
                next -= TAU;
            }
            self.phases[i] = next;
        }
        if self.sweep < 1.0 {
            self.steps[0] = (self.steps[0] * self.sweep).max(self.sweep_end);
        }
        self.lowpass_state += self.lowpass * (tone - self.lowpass_state);
        let mut sample = self.lowpass_state * self.envelope;
        if self.noise_envelope > 0.0 {
            let white = self.noise();
            self.highpass_state += self.highpass * (white - self.highpass_state);
            sample += (white - self.highpass_state) * self.noise_envelope;
            self.noise_envelope *= self.noise_decay;
        }
        self.envelope *= self.decay;
        self.remaining -= 1;
        sample
    }
}

/// Adds a click on every beat of the tempo map while the transport plays,
/// louder or different on the downbeat, in one of [`SOUNDS`], and on every
/// beat of a count-in, whether or not the metronome is on. Put it on
/// its own path to the output so it mixes with, rather than replaces,
/// the signal.
#[derive(Debug, Clone)]
pub struct Click {
    level: Smoother,
    sound: usize,
    on: bool,
    voice: Voice,
    sample_rate: f32,
}

impl Default for Click {
    fn default() -> Self {
        Self {
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
            sound: PARAMS[SOUND].default as usize,
            on: true,
            voice: Voice::default(),
            sample_rate: 48_000.0,
        }
    }
}

impl Node for Click {
    fn name(&self) -> &str {
        "Click"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            LEVEL => linear_to_db(self.level.target()),
            SOUND => self.sound as f32,
            ON => f32::from(u8::from(self.on)),
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        match id {
            LEVEL => self
                .level
                .set_target(db_to_linear(PARAMS[LEVEL].clamp(value))),
            // The new sound starts on the next beat; one ringing goes on.
            SOUND => self.sound = PARAMS[SOUND].clamp(value) as usize,
            // Off lets a ringing click finish rather than cut it.
            ON => self.on = value >= 0.5,
            _ => {}
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        self.level.set_time(SMOOTHING_SECONDS, self.sample_rate);
        self.level.snap(self.level.target());
        self.voice = Voice::default();
    }

    fn reset(&mut self) {
        self.voice = Voice::default();
    }

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let mut beats = (ctx.playing && self.on).then(|| {
            ctx.tempo
                .beats(ctx.position, ctx.position + ctx.span, ctx.sample_rate)
        });
        let mut next = beats.as_mut().and_then(|b| b.next());
        let (downbeat, beat) = &RECIPES[self.sound.min(RECIPES.len() - 1)];
        for frame in 0..frames {
            if let Some(first) = ctx.count_in.and_then(|c| c.beat_at(frame as u64)) {
                let recipe = if first { downbeat } else { beat };
                self.voice.trigger(recipe, self.sample_rate);
            }
            if let Some(b) = next {
                if ctx.frame_of(b.sample) == frame {
                    let recipe = if b.is_downbeat { downbeat } else { beat };
                    self.voice.trigger(recipe, self.sample_rate);
                    next = beats.as_mut().and_then(|b| b.next());
                }
            }
            let sample = self.voice.render() * self.level.advance();
            if sample != 0.0 {
                for channel in buffer.channels_mut() {
                    channel[frame] += sample;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::{TempoMap, TimeSignature};
    use crate::transport::CountIn;

    const SR: f64 = 48_000.0;

    fn render(
        click: &mut Click,
        tempo: &TempoMap,
        start: u64,
        frames: usize,
        playing: bool,
    ) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        let ctx = BlockContext {
            sample_rate: SR,
            frames,
            span: frames as u64,
            rate: 1.0,
            playing,
            position: start,
            loop_range: None,
            count_in: None,
            tempo,
        };
        let mut lanes = [&mut out[..]];
        let mut buffer = Buffer::new(&mut lanes);
        click.process(&ctx, &mut buffer);
        out
    }

    /// Frames where a click starts: the first loud sample after a run of
    /// silence longer than a cycle of the click's tone.
    fn onsets(signal: &[f32]) -> Vec<usize> {
        onsets_after(signal, 50)
    }

    /// Onsets after at least `gap` quiet frames.
    fn onsets_after(signal: &[f32], gap: usize) -> Vec<usize> {
        let mut onsets = Vec::new();
        let mut quiet_run = gap;
        for (i, s) in signal.iter().enumerate() {
            if s.abs() > 1e-4 {
                if quiet_run >= gap {
                    onsets.push(i);
                }
                quiet_run = 0;
            } else {
                quiet_run += 1;
            }
        }
        onsets
    }

    #[test]
    fn clicks_land_on_beats_across_blocks() {
        let tempo = TempoMap::new(120.0, TimeSignature::COMMON);
        let mut click = Click::default();
        click.prepare(SR, 64);
        click.set_param(LEVEL, 0.0);
        click.prepare(SR, 64);
        let total = 2 * 96_000;
        let mut out = Vec::with_capacity(total);
        let mut cursor = 0usize;
        for block in [64usize, 100, 7, 4096].into_iter().cycle() {
            if cursor >= total {
                break;
            }
            let n = block.min(total - cursor);
            out.extend(render(&mut click, &tempo, cursor as u64, n, true));
            cursor += n;
        }
        let found = onsets(&out);
        let expected: Vec<usize> = (0..8).map(|i| i * 24_000).collect();
        assert_eq!(found, expected);
        // The downbeat is louder than the other beats.
        let peak = |from: usize| {
            out[from..from + 200]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()))
        };
        assert!(
            peak(0) > peak(24_000) * 1.3,
            "{} vs {}",
            peak(0),
            peak(24_000)
        );
        assert!(peak(96_000) > peak(72_000) * 1.3);
        // Every click has rung out within 30 ms.
        for start in &expected {
            let after = start + (0.03 * SR) as usize + 1;
            assert!(out[after].abs() < 1e-3, "still ringing at {after}");
        }
    }

    /// Renders a count-in over several blocks, the way the runtime does.
    fn count(click: &mut Click, tempo: &TempoMap, count: CountIn, block: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; count.remaining as usize];
        let mut remaining = count.remaining;
        for chunk in out.chunks_mut(block) {
            let frames = chunk.len();
            let ctx = BlockContext {
                sample_rate: SR,
                frames,
                span: frames as u64,
                rate: 1.0,
                playing: false,
                position: 1000,
                loop_range: None,
                count_in: Some(CountIn { remaining, ..count }),
                tempo,
            };
            let mut lanes = [chunk];
            let mut buffer = Buffer::new(&mut lanes);
            click.process(&ctx, &mut buffer);
            remaining -= frames as u64;
        }
        out
    }

    #[test]
    fn a_count_in_clicks_its_beats_with_the_metronome_off() {
        let tempo = TempoMap::new(120.0, TimeSignature::COMMON);
        let mut click = Click::default();
        click.prepare(SR, 512);
        click.set_param(ON, 0.0);
        let out = count(&mut click, &tempo, CountIn::new(24_000, 4), 512);
        assert_eq!(onsets(&out), vec![0, 24_000, 48_000, 72_000]);
        let peak = |from: usize| {
            out[from..from + 2000]
                .iter()
                .fold(0.0f32, |p, s| p.max(s.abs()))
        };
        assert!(peak(0) > peak(24_000), "the first beat is marked");
    }

    #[test]
    fn silent_while_stopped_and_adds_to_the_input() {
        let tempo = TempoMap::default();
        let mut click = Click::default();
        click.prepare(SR, 64);
        assert!(render(&mut click, &tempo, 0, 64, false)
            .iter()
            .all(|v| *v == 0.0));
        let mut out = vec![0.25f32; 64];
        let ctx = BlockContext {
            sample_rate: SR,
            frames: 64,
            span: 64,
            rate: 1.0,
            playing: true,
            position: 0,
            loop_range: None,
            count_in: None,
            tempo: &tempo,
        };
        let mut lanes = [&mut out[..]];
        click.process(&ctx, &mut Buffer::new(&mut lanes));
        assert!(
            (out[0] - 0.75).abs() < 2e-3,
            "the downbeat peak at -6 dB adds to the input: {}",
            out[0]
        );
        assert!(out[63] != 0.25, "the click is still ringing");
        assert_eq!(click.param(LEVEL), -6.0);
        assert_eq!(click.name(), "Click");
    }

    /// Two bars at 120 bpm in 4/4 with `sound`, in uneven blocks.
    fn two_bars(sound: usize) -> Vec<f32> {
        let tempo = TempoMap::new(120.0, TimeSignature::COMMON);
        let mut click = Click::default();
        click.prepare(SR, 64);
        click.set_param(LEVEL, 0.0);
        click.set_param(SOUND, sound as f32);
        click.prepare(SR, 64);
        let total = 2 * 96_000;
        let mut out = Vec::with_capacity(total);
        let mut cursor = 0usize;
        for block in [64usize, 100, 7, 4096].into_iter().cycle() {
            if cursor >= total {
                break;
            }
            let n = block.min(total - cursor);
            out.extend(render(&mut click, &tempo, cursor as u64, n, true));
            cursor += n;
        }
        out
    }

    #[test]
    fn every_sound_lands_on_the_beat_and_marks_the_downbeat() {
        let expected: Vec<usize> = (0..8).map(|i| i * 24_000).collect();
        for (sound, name) in SOUNDS.iter().enumerate() {
            let out = two_bars(sound);
            // A cowbell's two squares cancel for a moment now and then,
            // so an onset needs 20 ms of quiet before it.
            assert_eq!(onsets_after(&out, 960), expected, "{name}");
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.3 && peak < 1.01, "{name} peaks at {peak}");
            assert!(out.iter().all(|v| v.is_finite()), "{name}");
            // The downbeat sounds different from the beat after it.
            let window = 2_000;
            let diff: f32 = (0..window)
                .map(|i| (out[i] - out[24_000 + i]).abs())
                .sum::<f32>()
                / window as f32;
            assert!(diff > 0.01, "{name}: downbeat and beat are alike ({diff})");
        }
    }

    #[test]
    fn sound_and_switch_are_parameters() {
        let tempo = TempoMap::default();
        let mut click = Click::default();
        click.prepare(SR, 64);
        click.set_param(SOUND, 2.4);
        assert_eq!(click.param(SOUND), 2.0);
        click.set_param(SOUND, 99.0);
        assert_eq!(click.param(SOUND), (SOUNDS.len() - 1) as f32);
        assert_eq!(click.param(ON), 1.0);
        click.set_param(ON, 0.0);
        assert_eq!(click.param(ON), 0.0);
        assert!(render(&mut click, &tempo, 0, 4_096, true)
            .iter()
            .all(|v| *v == 0.0));
        click.set_param(ON, 1.0);
        assert!(render(&mut click, &tempo, 0, 64, true)
            .iter()
            .any(|v| *v != 0.0));
    }
}
