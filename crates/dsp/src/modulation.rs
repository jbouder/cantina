//! The modulation pedals: chorus, flanger, phaser and tremolo. Each
//! runs an [`Lfo`] at a rate in hertz or locked to the transport at a
//! note value, and the stereo channels read the oscillator a quarter
//! cycle apart (except the tremolo, which pumps both together).

use crate::delay_line::DelayLine;
use crate::lfo::{note_beats, Lfo, Shape, NOTE_NAMES};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Transport, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;

const RATE: ParamId = 0;
const DEPTH: ParamId = 1;
/// Feedback for the delay pedals, Shape for the tremolo.
const THIRD: ParamId = 2;
/// Mix for the delay pedals, Sync for the tremolo.
const FOURTH: ParamId = 3;
const FIFTH: ParamId = 4;
const SIXTH: ParamId = 5;

const SMOOTHING_SECONDS: f32 = 0.02;
/// The second channel reads the oscillator this far ahead.
const STEREO_OFFSET: f32 = 0.25;
/// Default note value when synced: a quarter note.
const DEFAULT_NOTE: usize = 3;

/// The rate control shared by the pedals: hertz, or a note value when
/// synced to the transport.
#[derive(Debug, Clone)]
struct Rate {
    hz: f32,
    sync: bool,
    note: usize,
    transport: Transport,
}

impl Rate {
    fn new(hz: f32) -> Self {
        Self {
            hz,
            sync: false,
            note: DEFAULT_NOTE,
            transport: Transport::default(),
        }
    }

    /// Points the oscillator at the rate for this block.
    fn apply(&self, lfo: &mut Lfo) {
        if self.sync {
            lfo.sync(&self.transport, note_beats(self.note));
        } else {
            lfo.set_rate_hz(self.hz);
        }
    }
}

fn percent(value: f32) -> f32 {
    value / 100.0
}

const fn rate_params(default_hz: f32, max_hz: f32) -> ParamInfo {
    ParamInfo::new("Rate", Unit::Hertz, 0.05, max_hz, default_hz)
}

const fn sync_param() -> ParamInfo {
    ParamInfo::toggle("Sync", false)
}

const fn note_param() -> ParamInfo {
    ParamInfo::choice("Note", &NOTE_NAMES, DEFAULT_NOTE)
}

// --- delay-based: chorus and flanger ---

/// What separates a chorus from a flanger: where the delay sits and
/// how far it swings.
#[derive(Debug, Clone, Copy)]
struct Sweep {
    name: &'static str,
    base_ms: f32,
    swing_ms: f32,
    max_feedback: f32,
}

static CHORUS: Sweep = Sweep {
    name: "Chorus",
    base_ms: 12.0,
    swing_ms: 8.0,
    max_feedback: 0.7,
};

static FLANGER: Sweep = Sweep {
    name: "Flanger",
    base_ms: 1.0,
    swing_ms: 4.0,
    max_feedback: 0.95,
};

static CHORUS_PARAMS: [ParamInfo; 6] = [
    rate_params(0.8, 10.0),
    ParamInfo::new("Depth", Unit::Percent, 0.0, 100.0, 50.0),
    ParamInfo::new("Feedback", Unit::Percent, 0.0, 100.0, 0.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 50.0),
    sync_param(),
    note_param(),
];

static FLANGER_PARAMS: [ParamInfo; 6] = [
    rate_params(0.3, 10.0),
    ParamInfo::new("Depth", Unit::Percent, 0.0, 100.0, 70.0),
    ParamInfo::new("Feedback", Unit::Percent, 0.0, 100.0, 50.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 50.0),
    sync_param(),
    note_param(),
];

/// A modulated delay mixed with the dry signal.
pub struct SweptDelay {
    sweep: &'static Sweep,
    params: &'static [ParamInfo; 6],
    rate: Rate,
    depth: Smoother,
    feedback: Smoother,
    mix: Smoother,
    lfo: Lfo,
    lines: [DelayLine; MAX_CHANNELS],
    sample_rate: f32,
}

impl SweptDelay {
    fn new(sweep: &'static Sweep, params: &'static [ParamInfo; 6]) -> Self {
        Self {
            sweep,
            params,
            rate: Rate::new(params[RATE].default),
            depth: Smoother::new(percent(params[DEPTH].default)),
            feedback: Smoother::new(percent(params[THIRD].default) * sweep.max_feedback),
            mix: Smoother::new(percent(params[FOURTH].default)),
            lfo: Lfo::new(),
            lines: std::array::from_fn(|_| DelayLine::new(2)),
            sample_rate: 48_000.0,
        }
    }

    pub fn chorus() -> Self {
        Self::new(&CHORUS, &CHORUS_PARAMS)
    }

    pub fn flanger() -> Self {
        Self::new(&FLANGER, &FLANGER_PARAMS)
    }
}

impl Processor for SweptDelay {
    fn name(&self) -> &str {
        self.sweep.name
    }

    fn params(&self) -> &[ParamInfo] {
        self.params
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            RATE => self.rate.hz,
            DEPTH => self.depth.target() * 100.0,
            THIRD => self.feedback.target() / self.sweep.max_feedback * 100.0,
            FOURTH => self.mix.target() * 100.0,
            FIFTH => self.rate.sync as u8 as f32,
            SIXTH => self.rate.note as f32,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = self.params.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            RATE => self.rate.hz = value,
            DEPTH => self.depth.set_target(percent(value)),
            THIRD => self
                .feedback
                .set_target(percent(value) * self.sweep.max_feedback),
            FOURTH => self.mix.set_target(percent(value)),
            FIFTH => self.rate.sync = p.index(value) == 1,
            SIXTH => self.rate.note = p.index(value),
            _ => {}
        }
    }

    fn set_transport(&mut self, transport: &Transport) {
        self.rate.transport = *transport;
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.depth, &mut self.feedback, &mut self.mix] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        let longest = (self.sweep.base_ms + self.sweep.swing_ms) / 1000.0 * self.sample_rate;
        self.lines = std::array::from_fn(|_| DelayLine::new(longest as usize + 2));
        self.lfo.prepare(self.sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.reset();
        }
        self.lfo.reset();
    }

    fn process(&mut self, buffer: &mut Buffer) {
        self.rate.apply(&mut self.lfo);
        let channels = buffer.channels().min(MAX_CHANNELS);
        let per_ms = self.sample_rate / 1000.0;
        let base = self.sweep.base_ms * per_ms;
        let swing = self.sweep.swing_ms * per_ms;
        for frame in 0..buffer.frames() {
            let depth = self.depth.advance();
            let feedback = self.feedback.advance();
            let mix = self.mix.advance();
            for ch in 0..channels {
                let offset = if ch == 0 { 0.0 } else { STEREO_OFFSET };
                // The oscillator runs -1..1; the delay swings from the
                // base up by the depth.
                let wave = (self.lfo.value(offset) + 1.0) * 0.5;
                let delay = base + swing * depth * wave;
                let x = buffer.channel(ch)[frame];
                // Read before the push, so the newest sample in the
                // line is the previous frame's: one less to go back.
                let wet = self.lines[ch].read(delay - 1.0);
                self.lines[ch].push(x + wet * feedback);
                buffer.channel_mut(ch)[frame] = x * (1.0 - mix) + wet * mix;
            }
            self.lfo.advance();
        }
    }
}

// --- phaser ---

static PHASER_PARAMS: [ParamInfo; 6] = [
    rate_params(0.5, 10.0),
    ParamInfo::new("Depth", Unit::Percent, 0.0, 100.0, 70.0),
    ParamInfo::new("Feedback", Unit::Percent, 0.0, 100.0, 30.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 50.0),
    sync_param(),
    note_param(),
];

/// Allpass stages per channel: notches in the spectrum.
const PHASER_STAGES: usize = 4;
/// The sweep runs from here up by [`PHASER_SPAN`] octaves at full depth.
const PHASER_LOW_HZ: f32 = 300.0;
const PHASER_SPAN: f32 = 4.5;
const PHASER_MAX_FEEDBACK: f32 = 0.9;

/// A first-order allpass: flat in level, shifts phase around its
/// corner frequency.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Allpass {
    coefficient: f32,
    x1: f32,
    y1: f32,
}

impl Allpass {
    pub(crate) fn set_frequency(&mut self, hz: f32, sample_rate: f32) {
        let t = (std::f32::consts::PI * hz / sample_rate).tan();
        self.coefficient = (1.0 - t) / (1.0 + t);
    }

    #[inline]
    pub(crate) fn tick(&mut self, x: f32) -> f32 {
        let y = -self.coefficient * x + self.x1 + self.coefficient * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// A cascade of swept allpass stages mixed with the dry signal.
pub struct Phaser {
    rate: Rate,
    depth: Smoother,
    feedback: Smoother,
    mix: Smoother,
    lfo: Lfo,
    stages: [[Allpass; PHASER_STAGES]; MAX_CHANNELS],
    last: [f32; MAX_CHANNELS],
    sample_rate: f32,
}

impl Default for Phaser {
    fn default() -> Self {
        let p = &PHASER_PARAMS;
        Self {
            rate: Rate::new(p[RATE].default),
            depth: Smoother::new(percent(p[DEPTH].default)),
            feedback: Smoother::new(percent(p[THIRD].default) * PHASER_MAX_FEEDBACK),
            mix: Smoother::new(percent(p[FOURTH].default)),
            lfo: Lfo::new(),
            stages: [[Allpass::default(); PHASER_STAGES]; MAX_CHANNELS],
            last: [0.0; MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }
}

impl Processor for Phaser {
    fn name(&self) -> &str {
        "Phaser"
    }

    fn params(&self) -> &[ParamInfo] {
        &PHASER_PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            RATE => self.rate.hz,
            DEPTH => self.depth.target() * 100.0,
            THIRD => self.feedback.target() / PHASER_MAX_FEEDBACK * 100.0,
            FOURTH => self.mix.target() * 100.0,
            FIFTH => self.rate.sync as u8 as f32,
            SIXTH => self.rate.note as f32,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = PHASER_PARAMS.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            RATE => self.rate.hz = value,
            DEPTH => self.depth.set_target(percent(value)),
            THIRD => self
                .feedback
                .set_target(percent(value) * PHASER_MAX_FEEDBACK),
            FOURTH => self.mix.set_target(percent(value)),
            FIFTH => self.rate.sync = p.index(value) == 1,
            SIXTH => self.rate.note = p.index(value),
            _ => {}
        }
    }

    fn set_transport(&mut self, transport: &Transport) {
        self.rate.transport = *transport;
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.depth, &mut self.feedback, &mut self.mix] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        self.lfo.prepare(self.sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.stages = [[Allpass::default(); PHASER_STAGES]; MAX_CHANNELS];
        self.last = [0.0; MAX_CHANNELS];
        self.lfo.reset();
    }

    fn process(&mut self, buffer: &mut Buffer) {
        self.rate.apply(&mut self.lfo);
        let channels = buffer.channels().min(MAX_CHANNELS);
        for frame in 0..buffer.frames() {
            let depth = self.depth.advance();
            let feedback = self.feedback.advance();
            let mix = self.mix.advance();
            for ch in 0..channels {
                let offset = if ch == 0 { 0.0 } else { STEREO_OFFSET };
                let wave = (self.lfo.value(offset) + 1.0) * 0.5;
                let hz = PHASER_LOW_HZ * 2f32.powf(PHASER_SPAN * depth * wave);
                let x = buffer.channel(ch)[frame];
                let mut y = x + self.last[ch] * feedback;
                for stage in &mut self.stages[ch] {
                    stage.set_frequency(hz, self.sample_rate);
                    y = stage.tick(y);
                }
                self.last[ch] = y;
                buffer.channel_mut(ch)[frame] = x * (1.0 - mix) + y * mix;
            }
            self.lfo.advance();
        }
    }
}

// --- tremolo ---

static TREMOLO_SHAPES: [&str; 3] = ["Sine", "Triangle", "Square"];

static TREMOLO_PARAMS: [ParamInfo; 5] = [
    rate_params(5.0, 20.0),
    ParamInfo::new("Depth", Unit::Percent, 0.0, 100.0, 60.0),
    ParamInfo::choice("Shape", &TREMOLO_SHAPES, 0),
    sync_param(),
    note_param(),
];

const TREMOLO_SYNC: ParamId = 3;
const TREMOLO_NOTE: ParamId = 4;

/// Level pumped by the oscillator, the same on both channels.
pub struct Tremolo {
    rate: Rate,
    depth: Smoother,
    shape: usize,
    lfo: Lfo,
    sample_rate: f32,
}

impl Default for Tremolo {
    fn default() -> Self {
        let p = &TREMOLO_PARAMS;
        Self {
            rate: Rate::new(p[RATE].default),
            depth: Smoother::new(percent(p[DEPTH].default)),
            shape: 0,
            lfo: Lfo::new(),
            sample_rate: 48_000.0,
        }
    }
}

impl Processor for Tremolo {
    fn name(&self) -> &str {
        "Tremolo"
    }

    fn params(&self) -> &[ParamInfo] {
        &TREMOLO_PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            RATE => self.rate.hz,
            DEPTH => self.depth.target() * 100.0,
            THIRD => self.shape as f32,
            TREMOLO_SYNC => self.rate.sync as u8 as f32,
            TREMOLO_NOTE => self.rate.note as f32,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = TREMOLO_PARAMS.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            RATE => self.rate.hz = value,
            DEPTH => self.depth.set_target(percent(value)),
            THIRD => {
                self.shape = p.index(value);
                // The tremolo's shapes are the first three of the LFO's.
                self.lfo.set_shape(Shape::from_index(self.shape));
            }
            TREMOLO_SYNC => self.rate.sync = p.index(value) == 1,
            TREMOLO_NOTE => self.rate.note = p.index(value),
            _ => {}
        }
    }

    fn set_transport(&mut self, transport: &Transport) {
        self.rate.transport = *transport;
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        self.depth.set_time(SMOOTHING_SECONDS, self.sample_rate);
        self.depth.snap(self.depth.target());
        self.lfo.prepare(self.sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.lfo.reset();
    }

    fn process(&mut self, buffer: &mut Buffer) {
        self.rate.apply(&mut self.lfo);
        let channels = buffer.channels().min(MAX_CHANNELS);
        for frame in 0..buffer.frames() {
            let depth = self.depth.advance();
            // Full level at the top of the wave, down by the depth at
            // the bottom. Read each shape where it peaks, so a synced
            // tremolo is loudest on the beat and dips between.
            let offset = match self.lfo.shape() {
                Shape::Sine => 0.25,
                Shape::Triangle => 0.5,
                _ => 0.0,
            };
            let wave = (self.lfo.value(offset) + 1.0) * 0.5;
            let gain = 1.0 - depth * (1.0 - wave);
            for ch in 0..channels {
                buffer.channel_mut(ch)[frame] *= gain;
            }
            self.lfo.advance();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    const RATE_HZ: f32 = 48_000.0;

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.5 * (TAU * hz * i as f32 / RATE_HZ).sin())
            .collect()
    }

    /// Power at `hz` in `signal`, in dB, Hann-windowed.
    fn goertzel_db(signal: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE_HZ;
        let coeff = 2.0 * w.cos() as f64;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for (i, x) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f32 / signal.len() as f32).cos();
            let s0 = (x * window) as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        10.0 * (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(1e-30).log10() as f32
    }

    fn white_noise(n: usize) -> Vec<f32> {
        let mut seed = 7u32;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect()
    }

    #[test]
    fn every_pedal_builds_with_its_parameters() {
        let pedals: Vec<Box<dyn Processor>> = vec![
            Box::new(SweptDelay::chorus()),
            Box::new(SweptDelay::flanger()),
            Box::new(Phaser::default()),
            Box::new(Tremolo::default()),
        ];
        for pedal in pedals {
            let names: Vec<&str> = pedal.params().iter().map(|p| p.name.as_ref()).collect();
            assert!(names.contains(&"Rate"), "{}", pedal.name());
            assert!(names.contains(&"Depth"), "{}", pedal.name());
            assert!(names.contains(&"Sync"), "{}", pedal.name());
            assert!(names.contains(&"Note"), "{}", pedal.name());
            if pedal.name() != "Tremolo" {
                assert!(names.contains(&"Feedback"), "{}", pedal.name());
                assert!(names.contains(&"Mix"), "{}", pedal.name());
            }
        }
    }

    #[test]
    fn chorus_with_no_depth_is_a_fixed_delay_and_no_mix_is_dry() {
        let input = sine(440.0, 9600);
        let mut dry = SweptDelay::chorus();
        dry.set_param(FOURTH, 0.0);
        let out = Renderer::default().render(&mut dry, std::slice::from_ref(&input));
        assert_eq!(out[0], input);

        let mut fixed = SweptDelay::chorus();
        fixed.set_param(DEPTH, 0.0);
        fixed.set_param(FOURTH, 100.0);
        let out = Renderer::default().render(&mut fixed, std::slice::from_ref(&input));
        let delay = (12.0 * RATE_HZ / 1000.0) as usize;
        let worst = (delay + 100..9600)
            .map(|i| (out[0][i] - input[i - delay]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "{worst}");
    }

    #[test]
    fn chorus_spreads_a_tone_and_the_channels_differ() {
        let input = sine(440.0, 48_000);
        let mut chorus = SweptDelay::chorus();
        chorus.set_param(RATE, 2.0);
        let out = Renderer::default().render(&mut chorus, &[input.clone(), input.clone()]);
        // The wobble puts energy a few hertz either side of the tone.
        let centre = goertzel_db(&out[0], 440.0);
        let side = goertzel_db(&out[0], 446.0);
        let side_in = goertzel_db(&input, 446.0);
        assert!(side - centre > side_in - goertzel_db(&input, 440.0) + 10.0);
        let diff: f32 = out[0].iter().zip(&out[1]).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 100.0, "channels are the same: {diff}");
    }

    #[test]
    fn flanger_cuts_a_comb_into_noise() {
        // With no sweep the flanger is a 1 ms comb: a notch at 500 Hz,
        // 1500 Hz... and a peak at 1000 Hz.
        let mut flanger = SweptDelay::flanger();
        flanger.set_param(DEPTH, 0.0);
        flanger.set_param(THIRD, 0.0);
        let out = Renderer::default().render(&mut flanger, &[white_noise(96_000)]);
        let notch = goertzel_db(&out[0], 500.0);
        let peak = goertzel_db(&out[0], 1000.0);
        assert!(peak - notch > 15.0, "peak {peak} notch {notch}");
    }

    #[test]
    fn phaser_notches_and_feedback_sharpens() {
        let mut phaser = Phaser::default();
        phaser.set_param(DEPTH, 0.0);
        phaser.set_param(THIRD, 0.0);
        let noise = white_noise(96_000);
        let out = Renderer::default().render(&mut phaser, std::slice::from_ref(&noise));
        // Four first-order stages at 300 Hz shift 180 degrees at 0.414
        // and 2.414 times the corner, so the notches sit at 124 Hz and
        // 724 Hz, with the corner itself untouched.
        let notch = goertzel_db(&out[0], 724.0);
        let corner = goertzel_db(&out[0], 300.0);
        let away = goertzel_db(&out[0], 5000.0);
        assert!(corner - notch > 15.0, "notch {notch}, corner {corner}");
        assert!(away - notch > 15.0, "notch {notch}, away {away}");
        let mut sharp = Phaser::default();
        sharp.set_param(DEPTH, 0.0);
        sharp.set_param(THIRD, 90.0);
        let out = Renderer::default().render(&mut sharp, std::slice::from_ref(&noise));
        assert!(out[0].iter().all(|s| s.is_finite() && s.abs() < 10.0));
    }

    #[test]
    fn tremolo_pumps_at_its_rate_and_follows_the_tempo() {
        let flat = vec![1.0f32; 48_000];
        let mut trem = Tremolo::default();
        trem.set_param(RATE, 4.0);
        trem.set_param(DEPTH, 100.0);
        let out = Renderer::default().render(&mut trem, std::slice::from_ref(&flat));
        // Four dips a second, down to zero.
        let dips = out[0]
            .windows(3)
            .filter(|w| w[1] < w[0] && w[1] <= w[2] && w[1] < 0.01)
            .count();
        assert_eq!(dips, 4, "{dips} dips");
        let top = out[0].iter().fold(0.0f32, |m, s| m.max(*s));
        assert!((top - 1.0).abs() < 1e-3);

        // Synced to an eighth note at 90 bpm: 3 Hz, so six dips in two
        // seconds, and the first dip lands half a cycle after the beat.
        let mut synced = Tremolo::default();
        synced.set_param(TREMOLO_SYNC, 1.0);
        synced.set_param(TREMOLO_NOTE, 6.0);
        synced.set_param(DEPTH, 100.0);
        synced.set_param(THIRD, 1.0);
        synced.prepare(48_000.0, 64);
        synced.set_transport(&Transport {
            bpm: 90.0,
            beats: 0.0,
            playing: true,
        });
        let mut lanes = vec![vec![1.0f32; 96_000]];
        let mut position = 0u64;
        Renderer::default().run(&mut lanes, |buffer| {
            synced.set_transport(&Transport {
                bpm: 90.0,
                beats: position as f64 / 48_000.0 * 1.5,
                playing: true,
            });
            synced.process(buffer);
            position += buffer.frames() as u64;
        });
        let dips: Vec<usize> = lanes[0]
            .windows(3)
            .enumerate()
            .filter(|(_, w)| w[1] < w[0] && w[1] <= w[2] && w[1] < 0.01)
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(dips.len(), 6, "{dips:?}");
        // Loudest on the beat; a triangle dips at half a cycle, 8000
        // samples after it.
        assert!((dips[0] as i64 - 8000).abs() < 3, "{dips:?}");
        assert!((dips[1] as i64 - 24_000).abs() < 3, "{dips:?}");
        assert!(
            lanes[0][0] > 0.999 && lanes[0][16_000] > 0.999,
            "not loudest on the beat"
        );
    }
}
