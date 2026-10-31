//! The metronome click, driven by the tempo map.

use std::f32::consts::{FRAC_PI_2, TAU};

use cantina_dsp::{db_to_linear, linear_to_db, Buffer, ParamId, ParamInfo, Smoother, Unit};

use crate::graph::{BlockContext, Node};

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Level", Unit::Decibels, -40.0, 6.0, -6.0)];

const LEVEL: ParamId = 0;
const SMOOTHING_SECONDS: f32 = 0.005;
/// How long a click rings, in seconds, before it has decayed by 60 dB.
const CLICK_SECONDS: f32 = 0.03;
const DOWNBEAT_HZ: f32 = 1500.0;
const BEAT_HZ: f32 = 1000.0;
const DOWNBEAT_AMPLITUDE: f32 = 1.0;
const BEAT_AMPLITUDE: f32 = 0.6;

/// A short decaying sine, retriggered on every beat.
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    remaining: usize,
    phase: f32,
    phase_step: f32,
    envelope: f32,
    decay: f32,
}

impl Voice {
    fn trigger(&mut self, hz: f32, amplitude: f32, sample_rate: f32) {
        let length = (CLICK_SECONDS * sample_rate) as usize;
        self.remaining = length;
        // Start at the peak so the click has an edge.
        self.phase = FRAC_PI_2;
        self.phase_step = TAU * hz / sample_rate;
        self.envelope = amplitude;
        // Decay by 60 dB over the click's length.
        self.decay = (0.001f32).powf(1.0 / length.max(1) as f32);
    }

    #[inline]
    fn render(&mut self) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        let sample = self.phase.sin() * self.envelope;
        self.phase += self.phase_step;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        self.envelope *= self.decay;
        self.remaining -= 1;
        sample
    }
}

/// Adds a click on every beat of the tempo map while the transport plays,
/// louder and higher on the downbeat. Put it on its own path to the output
/// so it mixes with, rather than replaces, the signal.
#[derive(Debug, Clone)]
pub struct Click {
    level: Smoother,
    voice: Voice,
    sample_rate: f32,
}

impl Default for Click {
    fn default() -> Self {
        Self {
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
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
        let mut beats = ctx.playing.then(|| {
            ctx.tempo
                .beats(ctx.position, ctx.position + frames as u64, ctx.sample_rate)
        });
        let mut next = beats.as_mut().and_then(|b| b.next());
        for frame in 0..frames {
            if let Some(beat) = next {
                if beat.sample - ctx.position == frame as u64 {
                    let (hz, amplitude) = if beat.is_downbeat {
                        (DOWNBEAT_HZ, DOWNBEAT_AMPLITUDE)
                    } else {
                        (BEAT_HZ, BEAT_AMPLITUDE)
                    };
                    self.voice.trigger(hz, amplitude, self.sample_rate);
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
            playing,
            position: start,
            loop_range: None,
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
        let mut onsets = Vec::new();
        let mut quiet_run = 50;
        for (i, s) in signal.iter().enumerate() {
            if s.abs() > 1e-4 {
                if quiet_run >= 50 {
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
            playing: true,
            position: 0,
            loop_range: None,
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
}
