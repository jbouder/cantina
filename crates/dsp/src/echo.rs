//! The delay pedal: free time or a note value that follows the tempo,
//! feedback with a tone filter, and mono, stereo or ping-pong echoes.
//!
//! A change of time, whether from the knob or a tempo change on the
//! timeline, never slides the read head (which would bend the pitch of
//! the repeats): the pedal crossfades from the old time to the new over
//! a few tens of milliseconds, and takes the next change once the fade
//! is done, so a long knob drag follows in steps and sounds like a
//! digital delay, not a tape machine.

use crate::biquad::Biquad;
use crate::delay_line::DelayLine;
use crate::lfo::{note_beats, NOTE_NAMES};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Transport, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;

static MODES: [&str; 3] = ["Mono", "Stereo", "Ping-pong"];

static PARAMS: [ParamInfo; 7] = [
    ParamInfo::new("Time", Unit::Milliseconds, 1.0, 2000.0, 375.0),
    ParamInfo::toggle("Sync", false),
    ParamInfo::choice("Note", &NOTE_NAMES, 3),
    ParamInfo::new("Feedback", Unit::Percent, 0.0, 100.0, 35.0),
    ParamInfo::new("Tone", Unit::Hertz, 500.0, 12_000.0, 4000.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 35.0),
    ParamInfo::choice("Mode", &MODES, 0),
];

const TIME: ParamId = 0;
const SYNC: ParamId = 1;
const NOTE: ParamId = 2;
const FEEDBACK: ParamId = 3;
const TONE: ParamId = 4;
const MIX: ParamId = 5;
const MODE: ParamId = 6;

/// Longest delay, in seconds.
pub const MAX_SECONDS: f32 = 2.0;
/// Length of the crossfade between two delay times.
const FADE_SECONDS: f32 = 0.05;
/// Full feedback still loses this much per repeat, so it never runs away.
const MAX_FEEDBACK: f32 = 0.98;
const SMOOTHING_SECONDS: f32 = 0.02;
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Mono,
    Stereo,
    PingPong,
}

/// A read head that crossfades between two delay times instead of
/// sliding.
#[derive(Debug, Clone)]
struct Head {
    /// The time being faded out of, and the one being faded into.
    from: f32,
    to: f32,
    /// Fade progress from 0 (all `from`) to 1 (all `to`).
    fade: f32,
    step: f32,
    /// The time to fade to next, once this fade is done.
    pending: f32,
}

impl Head {
    fn new(samples: f32) -> Self {
        Self {
            from: samples,
            to: samples,
            fade: 1.0,
            step: 0.0,
            pending: samples,
        }
    }

    fn request(&mut self, samples: f32) {
        self.pending = samples;
    }

    fn snap(&mut self, samples: f32) {
        *self = Self {
            step: self.step,
            ..Self::new(samples)
        };
    }

    /// Reads the line at the current blend and advances the fade.
    #[inline]
    fn read(&mut self, line: &DelayLine) -> f32 {
        if self.fade >= 1.0 {
            if (self.pending - self.to).abs() >= 0.5 {
                self.from = self.to;
                self.to = self.pending;
                self.fade = 0.0;
            } else {
                return line.read(self.to);
            }
        }
        let a = line.read(self.from);
        let b = line.read(self.to);
        let out = a + (b - a) * self.fade;
        self.fade = (self.fade + self.step).min(1.0);
        out
    }
}

/// The delay pedal.
pub struct Echo {
    time_ms: f32,
    sync: bool,
    note: usize,
    transport: Transport,
    feedback: Smoother,
    tone_hz: Smoother,
    mix: Smoother,
    mode: Mode,
    lines: [DelayLine; MAX_CHANNELS],
    heads: [Head; MAX_CHANNELS],
    tone: [Biquad; MAX_CHANNELS],
    sample_rate: f32,
}

impl Default for Echo {
    fn default() -> Self {
        Self {
            time_ms: PARAMS[TIME].default,
            sync: false,
            note: PARAMS[NOTE].default as usize,
            transport: Transport::default(),
            feedback: Smoother::new(PARAMS[FEEDBACK].default / 100.0 * MAX_FEEDBACK),
            tone_hz: Smoother::new(PARAMS[TONE].default),
            mix: Smoother::new(PARAMS[MIX].default / 100.0),
            mode: Mode::Mono,
            lines: std::array::from_fn(|_| DelayLine::new(2)),
            heads: std::array::from_fn(|_| Head::new(1.0)),
            tone: [Biquad::identity(); MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }
}

impl Echo {
    /// The delay time in samples the pedal is heading for.
    fn wanted_samples(&self) -> f32 {
        let seconds = if self.sync {
            (60.0 / self.transport.bpm * note_beats(self.note)) as f32
        } else {
            self.time_ms / 1000.0
        };
        // One less than the time: the head reads before the push.
        (seconds.clamp(0.001, MAX_SECONDS) * self.sample_rate - 1.0).max(1.0)
    }

    /// The time in effect now, in seconds, for tests and display.
    pub fn time_seconds(&self) -> f32 {
        (self.heads[0].to + 1.0) / self.sample_rate
    }

    fn update_tone(&mut self, frames: usize) {
        if self.tone_hz.is_settled() {
            return;
        }
        let hz = self.tone_hz.advance_by(frames);
        for f in &mut self.tone {
            f.set_lowpass(self.sample_rate, hz, BUTTERWORTH_Q);
        }
    }
}

impl Processor for Echo {
    fn name(&self) -> &str {
        "Delay"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            TIME => self.time_ms,
            SYNC => self.sync as u8 as f32,
            NOTE => self.note as f32,
            FEEDBACK => self.feedback.target() / MAX_FEEDBACK * 100.0,
            TONE => self.tone_hz.target(),
            MIX => self.mix.target() * 100.0,
            MODE => match self.mode {
                Mode::Mono => 0.0,
                Mode::Stereo => 1.0,
                Mode::PingPong => 2.0,
            },
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = PARAMS.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            TIME => self.time_ms = value,
            SYNC => self.sync = p.index(value) == 1,
            NOTE => self.note = p.index(value),
            FEEDBACK => self.feedback.set_target(value / 100.0 * MAX_FEEDBACK),
            TONE => self.tone_hz.set_target(value),
            MIX => self.mix.set_target(value / 100.0),
            MODE => {
                self.mode = match p.index(value) {
                    1 => Mode::Stereo,
                    2 => Mode::PingPong,
                    _ => Mode::Mono,
                }
            }
            _ => {}
        }
        let wanted = self.wanted_samples();
        for head in &mut self.heads {
            head.request(wanted);
        }
    }

    fn set_transport(&mut self, transport: &Transport) {
        self.transport = *transport;
        if self.sync {
            let wanted = self.wanted_samples();
            for head in &mut self.heads {
                head.request(wanted);
            }
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [&mut self.feedback, &mut self.tone_hz, &mut self.mix] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        let longest = (MAX_SECONDS * self.sample_rate) as usize + 2;
        self.lines = std::array::from_fn(|_| DelayLine::new(longest));
        let wanted = self.wanted_samples();
        let step = 1.0 / (FADE_SECONDS * self.sample_rate);
        for head in &mut self.heads {
            head.step = step;
            head.snap(wanted);
        }
        let hz = self.tone_hz.target();
        for f in &mut self.tone {
            f.set_lowpass(self.sample_rate, hz, BUTTERWORTH_Q);
        }
        self.reset();
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.reset();
        }
        for f in &mut self.tone {
            f.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        self.update_tone(frames);
        let channels = buffer.channels().min(MAX_CHANNELS);
        for frame in 0..frames {
            let feedback = self.feedback.advance();
            let mix = self.mix.advance();
            let left = buffer.channel(0)[frame];
            let right = if channels > 1 {
                buffer.channel(1)[frame]
            } else {
                left
            };
            let wet_l = self.heads[0].read(&self.lines[0]);
            let wet_r = self.heads[1].read(&self.lines[1]);
            let (out_l, out_r) = match self.mode {
                Mode::Mono => {
                    let input = (left + right) * 0.5;
                    let back = self.tone[0].tick(wet_l) * feedback;
                    self.lines[0].push(input + back);
                    (wet_l, wet_l)
                }
                Mode::Stereo => {
                    let back_l = self.tone[0].tick(wet_l) * feedback;
                    let back_r = self.tone[1].tick(wet_r) * feedback;
                    self.lines[0].push(left + back_l);
                    self.lines[1].push(right + back_r);
                    (wet_l, wet_r)
                }
                Mode::PingPong => {
                    // The input goes left; each repeat crosses over.
                    let input = (left + right) * 0.5;
                    let back_r = self.tone[1].tick(wet_r) * feedback;
                    self.lines[0].push(input + back_r);
                    self.lines[1].push(self.tone[0].tick(wet_l));
                    (wet_l, wet_r)
                }
            };
            buffer.channel_mut(0)[frame] = left * (1.0 - mix) + out_l * mix;
            if channels > 1 {
                buffer.channel_mut(1)[frame] = right * (1.0 - mix) + out_r * mix;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;
    use std::f32::consts::TAU;

    const RATE: f32 = 48_000.0;

    fn impulse(n: usize) -> Vec<f32> {
        let mut v = vec![0.0; n];
        v[0] = 1.0;
        v
    }

    /// The signal's weight just after `at`: a repeat that went through
    /// the tone filter is spread over a few samples, so sum them.
    fn weight(signal: &[f32], at: usize) -> f32 {
        signal[at..at + 64].iter().sum()
    }

    fn silent(signal: &[f32], at: usize) -> bool {
        signal[at.saturating_sub(32)..at + 64]
            .iter()
            .all(|s| s.abs() < 1e-4)
    }

    fn goertzel_db(signal: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE;
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

    #[test]
    fn repeats_at_the_time_with_the_feedback() {
        let mut echo = Echo::default();
        echo.set_param(TIME, 100.0);
        echo.set_param(FEEDBACK, 50.0);
        echo.set_param(MIX, 50.0);
        echo.set_param(TONE, 12_000.0);
        let out = Renderer::default().render(&mut echo, &[impulse(24_000)]);
        let out = &out[0];
        // The dry impulse, then repeats every 4800 samples, each about
        // half (0.49, with the feedback cap) of the last.
        assert!((out[0] - 0.5).abs() < 1e-5);
        assert!(silent(out, 2400) && silent(out, 7200));
        assert!(
            out[4799].abs() < 1e-6 && (out[4800] - 0.5).abs() < 0.02,
            "{}",
            out[4800]
        );
        let first = weight(out, 4800);
        let second = weight(out, 9600);
        let third = weight(out, 14_400);
        assert!(out[9599].abs() < 1e-6, "second repeat is early");
        assert!((second / first - 0.49).abs() < 0.03, "{first} {second}");
        assert!((third / second - 0.49).abs() < 0.03, "{second} {third}");
    }

    #[test]
    fn sync_follows_the_tempo_without_bending_the_pitch() {
        let mut echo = Echo::default();
        echo.set_param(SYNC, 1.0);
        echo.set_param(NOTE, 3.0);
        echo.set_param(FEEDBACK, 0.0);
        echo.set_param(MIX, 100.0);
        echo.set_param(TONE, 12_000.0);
        echo.prepare(48_000.0, 64);
        echo.set_transport(&Transport {
            bpm: 120.0,
            beats: 0.0,
            playing: true,
        });
        assert!((echo.time_seconds() - 0.5).abs() < 1e-4);
        // A steady tone; the tempo drops to 60 bpm one second in.
        let input: Vec<f32> = (0..144_000)
            .map(|i| 0.5 * (TAU * 440.0 * i as f32 / RATE).sin())
            .collect();
        let mut lanes = vec![input];
        let mut position = 0usize;
        Renderer::default().run(&mut lanes, |buffer| {
            let bpm = if position < 48_000 { 120.0 } else { 60.0 };
            echo.set_transport(&Transport {
                bpm,
                beats: position as f64 / 24_000.0,
                playing: true,
            });
            echo.process(buffer);
            position += buffer.frames();
        });
        assert!((echo.time_seconds() - 1.0).abs() < 1e-4);
        // Around the change the wet signal is still a clean 440 Hz:
        // a sliding read head would have smeared it down in pitch.
        let around = &lanes[0][46_000..54_000];
        let tone = goertzel_db(around, 440.0);
        for off in [400.0, 420.0, 460.0, 480.0] {
            let side = goertzel_db(around, off);
            assert!(
                tone - side > 30.0,
                "{off} Hz is only {} dB down",
                tone - side
            );
        }
    }

    #[test]
    fn ping_pong_alternates_and_stereo_keeps_sides() {
        let mut pp = Echo::default();
        pp.set_param(TIME, 100.0);
        pp.set_param(FEEDBACK, 60.0);
        pp.set_param(MIX, 100.0);
        pp.set_param(TONE, 12_000.0);
        pp.set_param(MODE, 2.0);
        let out = Renderer::default().render(&mut pp, &[impulse(24_000), vec![0.0; 24_000]]);
        assert!(
            weight(&out[0], 4800) > 0.4 && silent(&out[1], 4800),
            "first repeat left"
        );
        assert!(
            weight(&out[1], 9600) > 0.4 && silent(&out[0], 9600),
            "second repeat right"
        );
        assert!(
            weight(&out[0], 14_400) > 0.2 && silent(&out[1], 14_400),
            "third repeat left"
        );

        let mut stereo = Echo::default();
        stereo.set_param(TIME, 100.0);
        stereo.set_param(MIX, 100.0);
        stereo.set_param(MODE, 1.0);
        let out = Renderer::default().render(&mut stereo, &[impulse(12_000), vec![0.0; 12_000]]);
        assert!(out[0][4800].abs() > 0.5);
        assert!(out[1].iter().all(|s| s.abs() < 1e-6), "right stays silent");
    }

    #[test]
    fn tone_darkens_the_repeats_and_dry_mix_is_untouched() {
        let input: Vec<f32> = (0..24_000)
            .map(|i| 0.3 * (TAU * 8000.0 * i as f32 / RATE).sin())
            .collect();
        let mut dark = Echo::default();
        dark.set_param(TIME, 50.0);
        dark.set_param(TONE, 500.0);
        dark.set_param(MIX, 100.0);
        dark.set_param(FEEDBACK, 0.0);
        let out = Renderer::default().render(&mut dark, std::slice::from_ref(&input));
        // The first repeat is not filtered (tone sits in the feedback
        // path), so check a repeat that went round once.
        let mut fed = Echo::default();
        fed.set_param(TIME, 50.0);
        fed.set_param(TONE, 500.0);
        fed.set_param(MIX, 100.0);
        fed.set_param(FEEDBACK, 100.0);
        let fed_out = Renderer::default().render(&mut fed, std::slice::from_ref(&input));
        let once = goertzel_db(&out[0][12_000..], 8000.0);
        let twice = goertzel_db(&fed_out[0][12_000..], 8000.0);
        assert!(
            twice - once < 6.0,
            "feedback should not add bright energy: {once} {twice}"
        );

        let mut dry = Echo::default();
        dry.set_param(MIX, 0.0);
        let out = Renderer::default().render(&mut dry, std::slice::from_ref(&input));
        assert_eq!(out[0], input);
    }
}
