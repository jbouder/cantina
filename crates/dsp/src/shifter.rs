//! The pitch pedal: an octaver and a pitch shifter in one. Four voices
//! follow the input: two octaves down, one octave down, one octave up,
//! and a free shift of up to an octave either way. Each has its own
//! level, and Mix blends them over the dry signal.
//!
//! Every voice is a delay-line shifter. A read head moves through a
//! short window of the recent input at a speed that plays it back at
//! the new pitch: faster than real time to go up, slower to go down.
//! When it reaches the end of the window it has to jump back, so each
//! voice runs two heads, half a window out of step, and crossfades
//! between them with a raised cosine, each silent at the moment it
//! jumps. The voices are worked out on the mono sum, as an octaver
//! tracks one line.
//!
//! Where a head lands when it jumps matters. Two heads reading a note a
//! fraction of a period apart sum out of phase, and a steady note comes
//! out as a warble of sidebands rather than the interval. So a head
//! does not land at a fixed place: it searches a short span for the
//! spot whose recent waveform best matches what the other head is
//! reading (the most correlated, as WSOLA does), and the two then run
//! in phase through the crossfade.
//!
//! The heads sit, on average, half a window and half the search span
//! behind the input. The dry signal is delayed by the same amount, so a
//! voice lines up with the note it came from rather than trailing it
//! like a slapback, and the pedal reports that as its latency: about
//! 16 ms.

use crate::biquad::Biquad;
use crate::delay_line::DelayLine;
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;
use std::f32::consts::TAU;

static PARAMS: [ParamInfo; 6] = [
    ParamInfo::new("Two down", Unit::Percent, 0.0, 100.0, 0.0),
    ParamInfo::new("Octave down", Unit::Percent, 0.0, 100.0, 100.0),
    ParamInfo::new("Octave up", Unit::Percent, 0.0, 100.0, 0.0),
    ParamInfo::new("Semitones", Unit::Generic, -12.0, 12.0, 7.0),
    ParamInfo::new("Shift", Unit::Percent, 0.0, 100.0, 0.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 50.0),
];

const TWO_DOWN: ParamId = 0;
const OCTAVE_DOWN: ParamId = 1;
const OCTAVE_UP: ParamId = 2;
const SEMITONES: ParamId = 3;
const SHIFT: ParamId = 4;
const MIX: ParamId = 5;

/// The window a head moves through between jumps, in seconds.
pub const WINDOW_SECONDS: f32 = 0.02;
/// How far past its landing place a head may look for one in phase
/// with the other head, in seconds: a period of a guitar's low E.
pub const SEARCH_SECONDS: f32 = 0.0125;
/// Samples of waveform compared at each candidate landing place.
const COMPARE: usize = 128;
/// The voices below the input are rounded off above this, which hides
/// the grain of the crossfades where a sub octave does not need treble.
const SUB_TONE_HZ: f32 = 2500.0;
const SMOOTHING_SECONDS: f32 = 0.02;
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// The nearest a head reads to the newest sample.
const NEAREST: f32 = 1.0;

/// The sizes a voice works with, in samples at the current rate.
#[derive(Debug, Clone, Copy)]
struct Span {
    window: f32,
    search: usize,
    /// The furthest back a head may read.
    furthest: f32,
}

/// One read head: how far back it reads, and how far through its
/// crossfade it is, from 0 (silent, just landed) to 1 (silent, about
/// to jump).
#[derive(Debug, Clone, Copy, Default)]
struct Head {
    delay: f32,
    fade: f32,
}

/// One shifted voice: two heads half a crossfade apart.
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    heads: [Head; 2],
}

/// How well the waveform `a` samples back matches the one `b` back,
/// over [`COMPARE`] samples: 1 for the same, 0 for unrelated.
fn likeness(line: &DelayLine, a: usize, b: usize) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for k in 0..COMPARE {
        let x = line.read_at(a + k);
        let y = line.read_at(b + k);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa + bb < 1e-12 {
        0.0
    } else {
        2.0 * ab / (aa + bb)
    }
}

impl Voice {
    /// Both heads at their landing places, half a crossfade apart.
    fn start(span: Span) -> Self {
        let mut v = Self::default();
        v.heads[1].fade = 0.5;
        v.heads[1].delay = NEAREST + 0.5 * span.window;
        v.heads[0].delay = NEAREST;
        v
    }

    /// Where a head going at `ratio` lands: the place in the search span
    /// past its starting point that best matches what `other` reads.
    fn land(line: &DelayLine, ratio: f32, other: f32, span: Span) -> f32 {
        // Going down, a head falls behind, so it starts near the newest
        // sample; going up, it closes in, so it starts a window back.
        let base = if ratio < 1.0 {
            NEAREST
        } else {
            NEAREST + span.window
        };
        let other = other.round() as usize;
        let mut best = (f32::MIN, 0usize);
        for offset in 0..=span.search {
            let at = base as usize + offset;
            let score = likeness(line, at, other);
            if score > best.0 {
                best = (score, offset);
            }
        }
        base + best.1 as f32
    }

    /// Reads the voice at `ratio` (2 is an octave up) from a line the
    /// current sample was just pushed to, and moves the heads on. When
    /// `quiet`, the voice is not heard, so a head lands without looking.
    #[inline]
    fn tick(&mut self, line: &DelayLine, ratio: f32, span: Span, quiet: bool) -> f32 {
        // Each head is loudest mid-crossfade and silent at either end;
        // the two gains always sum to one.
        let gain = |fade: f32| 0.5 - 0.5 * (TAU * fade).cos();
        let mut out = 0.0;
        for h in &self.heads {
            out += gain(h.fade) * line.read(h.delay);
        }
        // A head that plays faster than the input closes on it, so its
        // delay shrinks; a slower one falls behind.
        let step = 1.0 - ratio;
        let fade_step = step.abs() / span.window;
        for i in 0..2 {
            let head = &mut self.heads[i];
            head.delay = (head.delay + step).clamp(NEAREST, span.furthest);
            head.fade += fade_step;
            if head.fade >= 1.0 {
                let other = self.heads[1 - i].delay;
                let delay = if quiet {
                    if ratio < 1.0 {
                        NEAREST
                    } else {
                        NEAREST + span.window
                    }
                } else {
                    Self::land(line, ratio, other, span)
                };
                let head = &mut self.heads[i];
                head.fade -= 1.0;
                head.delay = delay;
            }
        }
        out
    }
}

/// The pitch pedal.
pub struct PitchShifter {
    two_down: Smoother,
    octave_down: Smoother,
    octave_up: Smoother,
    semitones: f32,
    /// The free voice's speed, smoothed so a change of interval glides.
    ratio: Smoother,
    shift: Smoother,
    mix: Smoother,
    /// The mono input the voices read.
    input: DelayLine,
    /// The dry signal, delayed to line up with the voices.
    dry: [DelayLine; MAX_CHANNELS],
    voices: [Voice; 4],
    sub_tone: Biquad,
    span: Span,
    /// Where the heads read on average, in whole samples: the dry
    /// signal's delay.
    latency: usize,
}

fn ratio_of(semitones: f32) -> f32 {
    (semitones / 12.0).exp2()
}

impl Default for PitchShifter {
    fn default() -> Self {
        let semitones = PARAMS[SEMITONES].default;
        let mut shifter = Self {
            two_down: Smoother::new(PARAMS[TWO_DOWN].default / 100.0),
            octave_down: Smoother::new(PARAMS[OCTAVE_DOWN].default / 100.0),
            octave_up: Smoother::new(PARAMS[OCTAVE_UP].default / 100.0),
            semitones,
            ratio: Smoother::new(ratio_of(semitones)),
            shift: Smoother::new(PARAMS[SHIFT].default / 100.0),
            mix: Smoother::new(PARAMS[MIX].default / 100.0),
            input: DelayLine::new(2),
            dry: std::array::from_fn(|_| DelayLine::new(2)),
            voices: [Voice::default(); 4],
            sub_tone: Biquad::identity(),
            span: Span {
                window: 2.0,
                search: 0,
                furthest: 2.0,
            },
            latency: 2,
        };
        shifter.size_for(48_000.0);
        shifter
    }
}

impl PitchShifter {
    /// Sizes the window, the search and the lines for a sample rate.
    /// Allocates.
    fn size_for(&mut self, sample_rate: f32) {
        let window = (WINDOW_SECONDS * sample_rate).round().max(2.0);
        let search = (SEARCH_SECONDS * sample_rate).round() as usize;
        // A head lands up to a search past its base and moves a window
        // from there; a change of direction mid-crossfade can take it a
        // window further, which the clamp holds.
        let furthest = NEAREST + 2.0 * window + search as f32;
        self.span = Span {
            window,
            search,
            furthest,
        };
        self.latency = (NEAREST + 0.5 * window + 0.5 * search as f32).round() as usize;
        self.input = DelayLine::new(furthest as usize + COMPARE + 4);
        self.dry = std::array::from_fn(|_| DelayLine::new(self.latency + 2));
        self.sub_tone
            .set_lowpass(sample_rate, SUB_TONE_HZ, BUTTERWORTH_Q);
        self.voices = [Voice::start(self.span); 4];
    }

    fn smoothers(&mut self) -> [&mut Smoother; 6] {
        [
            &mut self.two_down,
            &mut self.octave_down,
            &mut self.octave_up,
            &mut self.ratio,
            &mut self.shift,
            &mut self.mix,
        ]
    }
}

impl Processor for PitchShifter {
    fn name(&self) -> &str {
        "Pitch"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            TWO_DOWN => self.two_down.target() * 100.0,
            OCTAVE_DOWN => self.octave_down.target() * 100.0,
            OCTAVE_UP => self.octave_up.target() * 100.0,
            SEMITONES => self.semitones,
            SHIFT => self.shift.target() * 100.0,
            MIX => self.mix.target() * 100.0,
            _ => 0.0,
        }
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let Some(p) = PARAMS.get(id) else {
            return;
        };
        let value = p.clamp(value);
        match id {
            TWO_DOWN => self.two_down.set_target(value / 100.0),
            OCTAVE_DOWN => self.octave_down.set_target(value / 100.0),
            OCTAVE_UP => self.octave_up.set_target(value / 100.0),
            SEMITONES => {
                self.semitones = value;
                self.ratio.set_target(ratio_of(value));
            }
            SHIFT => self.shift.set_target(value / 100.0),
            MIX => self.mix.set_target(value / 100.0),
            _ => {}
        }
    }

    fn latency(&self) -> usize {
        self.latency
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        let rate = sample_rate as f32;
        for s in self.smoothers() {
            s.set_time(SMOOTHING_SECONDS, rate);
            s.snap(s.target());
        }
        self.size_for(rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.input.reset();
        for line in &mut self.dry {
            line.reset();
        }
        self.voices = [Voice::start(self.span); 4];
        self.sub_tone.reset();
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let channels = buffer.channels().min(MAX_CHANNELS);
        let span = self.span;
        for frame in 0..frames {
            let two_down = self.two_down.advance();
            let octave_down = self.octave_down.advance();
            let octave_up = self.octave_up.advance();
            let ratio = self.ratio.advance();
            let shift = self.shift.advance();
            let mix = self.mix.advance();

            let left = buffer.channel(0)[frame];
            let right = if channels > 1 {
                buffer.channel(1)[frame]
            } else {
                left
            };
            self.input.push((left + right) * 0.5);
            self.dry[0].push(left);
            self.dry[1].push(right);

            // A voice turned all the way down, or a dry mix, is not heard:
            // its heads still move but land without searching.
            let quiet = |level: f32| level * mix < 1e-4;
            let [v2, v1, up, free] = &mut self.voices;
            let below = two_down * v2.tick(&self.input, 0.25, span, quiet(two_down))
                + octave_down * v1.tick(&self.input, 0.5, span, quiet(octave_down));
            let wet = self.sub_tone.tick(below)
                + octave_up * up.tick(&self.input, 2.0, span, quiet(octave_up))
                + shift * free.tick(&self.input, ratio, span, quiet(shift));

            let dry_l = self.dry[0].read_at(self.latency);
            buffer.channel_mut(0)[frame] = dry_l * (1.0 - mix) + wet * mix;
            if channels > 1 {
                let dry_r = self.dry[1].read_at(self.latency);
                buffer.channel_mut(1)[frame] = dry_r * (1.0 - mix) + wet * mix;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Renderer;

    const RATE: f32 = 48_000.0;

    fn sine(hz: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|i| 0.5 * (TAU * hz * i as f32 / RATE).sin())
            .collect()
    }

    /// Level at `hz` in dB, Hann-windowed.
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

    /// The pedal with one voice up full and the mix all wet.
    fn only(id: ParamId) -> PitchShifter {
        let mut p = PitchShifter::default();
        for v in [TWO_DOWN, OCTAVE_DOWN, OCTAVE_UP, SHIFT] {
            p.set_param(v, if v == id { 100.0 } else { 0.0 });
        }
        p.set_param(MIX, 100.0);
        p
    }

    #[test]
    fn each_voice_lands_on_its_interval() {
        // 220 Hz in; each voice's note stands well above the input's.
        for (id, semitones, hz) in [
            (OCTAVE_DOWN, 0.0, 110.0),
            (TWO_DOWN, 0.0, 55.0),
            (OCTAVE_UP, 0.0, 440.0),
            (SHIFT, 7.0, 220.0 * 2f32.powf(7.0 / 12.0)),
            (SHIFT, -5.0, 220.0 * 2f32.powf(-5.0 / 12.0)),
        ] {
            let mut p = only(id);
            p.set_param(SEMITONES, semitones);
            let out = Renderer::default().render(&mut p, &[sine(220.0, 1.0)]);
            let tail = &out[0][12_000..];
            let note = goertzel_db(tail, hz);
            let input = goertzel_db(tail, 220.0);
            assert!(
                note - input > 12.0,
                "voice {id} at {semitones}: {hz} Hz is {note:.1} dB, the input {input:.1} dB"
            );
        }
    }

    #[test]
    fn dry_is_delayed_by_the_reported_latency() {
        let mut p = PitchShifter::default();
        p.set_param(MIX, 0.0);
        p.prepare(RATE as f64, 256);
        let latency = p.latency();
        assert_eq!(
            latency, 781,
            "half of 20 ms and of 12.5 ms at 48 kHz, and one"
        );
        let mut input = vec![0.0; 4096];
        input[100] = 1.0;
        let out = Renderer::default().render(&mut p, &[input.clone(), input]);
        for ch in &out {
            let peak = ch.iter().position(|s| s.abs() > 0.5).unwrap();
            assert_eq!(peak, 100 + latency);
        }
    }

    #[test]
    fn a_voice_lines_up_with_the_dry_signal() {
        // A burst through the octave-down voice alone: its energy is
        // centered on the burst delayed by the latency, not later.
        let mut p = only(OCTAVE_DOWN);
        p.prepare(RATE as f64, 256);
        let latency = p.latency();
        let mut input = vec![0.0; 24_000];
        for (i, s) in input[4800..9600].iter_mut().enumerate() {
            *s = 0.5 * (TAU * 220.0 * i as f32 / RATE).sin();
        }
        let out = Renderer::default().render(&mut p, &[input]);
        let energy: Vec<f32> = out[0].iter().map(|s| s * s).collect();
        let total: f32 = energy.iter().sum();
        let center = energy
            .iter()
            .enumerate()
            .map(|(i, e)| i as f32 * e)
            .sum::<f32>()
            / total;
        let expected = 7200.0 + latency as f32;
        assert!(
            (center - expected).abs() < 0.004 * RATE,
            "centered at {center}, expected {expected}"
        );
    }

    #[test]
    fn a_change_of_interval_glides_rather_than_clicks() {
        let mut p = only(SHIFT);
        p.set_param(SEMITONES, -12.0);
        p.prepare(RATE as f64, 64);
        let input = sine(220.0, 1.0);
        let mut lanes = vec![input];
        let mut position = 0usize;
        Renderer::default().run(&mut lanes, |buffer| {
            if position == 24_000 {
                p.set_param(SEMITONES, 12.0);
                p.set_param(MIX, 30.0);
            }
            p.process(buffer);
            position += buffer.frames();
        });
        let out = &lanes[0];
        let worst = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // An octave up of a 0.5 sine at 440 Hz steps at most 0.03 a
        // sample; nothing jumps by much more.
        assert!(worst < 0.1, "a step of {worst}");
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 1.5));
    }

    #[test]
    fn silence_stays_silent_and_parameters_round_trip() {
        let mut p = PitchShifter::default();
        let out = Renderer::default().render(&mut p, &[vec![0.0; 4800]]);
        assert!(out[0].iter().all(|s| *s == 0.0));
        p.set_param(SEMITONES, 40.0);
        assert_eq!(p.param(SEMITONES), 12.0, "clamped");
        p.set_param(OCTAVE_UP, 25.0);
        assert_eq!(p.param(OCTAVE_UP), 25.0);
        let state = p.state();
        let mut q = PitchShifter::default();
        q.set_state(&state).unwrap();
        assert_eq!(q.param(OCTAVE_UP), 25.0);
        assert_eq!(q.param(SEMITONES), 12.0);
    }
}
