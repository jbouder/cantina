//! An algorithmic reverb: a feedback delay network of eight lines
//! behind a pre-delay and input diffusion.
//!
//! Each line feeds every other through a Householder matrix, which
//! keeps the energy while scattering it, so the echo density builds
//! fast. The lines are at mutually awkward lengths and each is slowly
//! modulated by a fraction of a millisecond, so no two paths ever line
//! up for long: that is what keeps a sustained chord from turning into
//! a metallic ring. A one-pole lowpass in each loop damps the highs
//! faster than the lows, as air and walls do, and the loop gain is set
//! from the decay time so the tail reaches -60 dB when asked.

use crate::delay_line::DelayLine;
use crate::lfo::{Lfo, Shape};
use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit, MAX_CHANNELS};
use crate::smooth::Smoother;

static PARAMS: [ParamInfo; 5] = [
    ParamInfo::new("Size", Unit::Percent, 0.0, 100.0, 50.0),
    ParamInfo::new("Decay", Unit::Seconds, 0.1, 20.0, 2.0),
    ParamInfo::new("Damping", Unit::Percent, 0.0, 100.0, 50.0),
    ParamInfo::new("Pre-delay", Unit::Milliseconds, 0.0, 200.0, 10.0),
    ParamInfo::new("Mix", Unit::Percent, 0.0, 100.0, 30.0),
];

const SIZE: ParamId = 0;
const DECAY: ParamId = 1;
const DAMPING: ParamId = 2;
const PREDELAY: ParamId = 3;
const MIX: ParamId = 4;

const LINES: usize = 8;
/// Line lengths in milliseconds at full size; size scales them from
/// half to one and a half times this.
const LENGTHS_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 61.3, 67.9, 73.1];
/// Input diffusers: Schroeder allpasses at these lengths.
const DIFFUSER_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];
const DIFFUSER_GAIN: f32 = 0.6;
/// How far each line's length wobbles, in milliseconds, and how fast.
/// Small and slow: enough to keep the modes from lining up over a long
/// tail, too little to hear as a pitch wobble (a sustained chord
/// spreads by about a hertz).
const WOBBLE_MS: f32 = 0.1;
const WOBBLE_HZ: [f32; LINES] = [0.11, 0.17, 0.23, 0.29, 0.31, 0.37, 0.41, 0.47];
const SMOOTHING_SECONDS: f32 = 0.05;
/// Pre-delay changes crossfade like the delay pedal would; here a
/// smoother on the read position is enough since it is rarely moved.
const MAX_PREDELAY_MS: f32 = 200.0;

/// A Schroeder allpass: dense, colourless smearing.
#[derive(Debug, Clone)]
struct Diffuser {
    line: DelayLine,
    delay: usize,
}

impl Diffuser {
    fn new(delay: usize) -> Self {
        Self {
            line: DelayLine::new(delay + 1),
            delay,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let delayed = self.line.read_at(self.delay - 1);
        let input = x - DIFFUSER_GAIN * delayed;
        self.line.push(input);
        delayed + DIFFUSER_GAIN * input
    }
}

/// The reverb.
pub struct Reverb {
    size: Smoother,
    decay_seconds: f32,
    damping: Smoother,
    predelay: Smoother,
    mix: Smoother,
    pre: DelayLine,
    diffusers: [Diffuser; 4],
    lines: [DelayLine; LINES],
    /// One-pole lowpass state per line.
    damp_state: [f32; LINES],
    wobble: [Lfo; LINES],
    sample_rate: f32,
}

impl Default for Reverb {
    fn default() -> Self {
        let mut wobble: [Lfo; LINES] = std::array::from_fn(|_| Lfo::new());
        for (i, lfo) in wobble.iter_mut().enumerate() {
            lfo.set_shape(Shape::Sine);
            lfo.set_rate_hz(WOBBLE_HZ[i]);
        }
        Self {
            size: Smoother::new(PARAMS[SIZE].default / 100.0),
            decay_seconds: PARAMS[DECAY].default,
            damping: Smoother::new(PARAMS[DAMPING].default / 100.0),
            predelay: Smoother::new(PARAMS[PREDELAY].default),
            mix: Smoother::new(PARAMS[MIX].default / 100.0),
            pre: DelayLine::new(2),
            diffusers: std::array::from_fn(|_| Diffuser::new(2)),
            lines: std::array::from_fn(|_| DelayLine::new(2)),
            damp_state: [0.0; LINES],
            wobble,
            sample_rate: 48_000.0,
        }
    }
}

impl Reverb {
    /// Length of line `i` in samples at `size` (0 to 1).
    fn length(&self, i: usize, size: f32) -> f32 {
        LENGTHS_MS[i] * (0.5 + size) * self.sample_rate / 1000.0
    }

    /// Loop gain for a line so the tail falls 60 dB in the decay time.
    fn loop_gain(&self, length_samples: f32) -> f32 {
        let seconds = length_samples / self.sample_rate;
        10f32.powf(-3.0 * seconds / self.decay_seconds.max(0.01))
    }
}

impl Processor for Reverb {
    fn name(&self) -> &str {
        "Reverb"
    }

    fn params(&self) -> &[ParamInfo] {
        &PARAMS
    }

    fn param(&self, id: ParamId) -> f32 {
        match id {
            SIZE => self.size.target() * 100.0,
            DECAY => self.decay_seconds,
            DAMPING => self.damping.target() * 100.0,
            PREDELAY => self.predelay.target(),
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
            SIZE => self.size.set_target(value / 100.0),
            DECAY => self.decay_seconds = value,
            DAMPING => self.damping.set_target(value / 100.0),
            PREDELAY => self.predelay.set_target(value),
            MIX => self.mix.set_target(value / 100.0),
            _ => {}
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block_frames: usize) {
        self.sample_rate = sample_rate as f32;
        for s in [
            &mut self.size,
            &mut self.damping,
            &mut self.predelay,
            &mut self.mix,
        ] {
            s.set_time(SMOOTHING_SECONDS, self.sample_rate);
            s.snap(s.target());
        }
        let per_ms = self.sample_rate / 1000.0;
        self.pre = DelayLine::new((MAX_PREDELAY_MS * per_ms) as usize + 2);
        self.diffusers = std::array::from_fn(|i| Diffuser::new((DIFFUSER_MS[i] * per_ms) as usize));
        self.lines = std::array::from_fn(|i| {
            DelayLine::new(((LENGTHS_MS[i] * 1.5 + WOBBLE_MS * 2.0) * per_ms) as usize + 2)
        });
        for lfo in &mut self.wobble {
            lfo.prepare(self.sample_rate);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.pre.reset();
        for d in &mut self.diffusers {
            d.line.reset();
        }
        for line in &mut self.lines {
            line.reset();
        }
        self.damp_state = [0.0; LINES];
        for (i, lfo) in self.wobble.iter_mut().enumerate() {
            lfo.reset();
            // Spread the wobbles so they do not start together.
            for _ in 0..(i * 997) {
                lfo.advance();
            }
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let channels = buffer.channels().min(MAX_CHANNELS);
        let per_ms = self.sample_rate / 1000.0;
        let wobble = WOBBLE_MS * per_ms;
        let mut gains = [0.0f32; LINES];
        let mut lengths = [0.0f32; LINES];
        for frame in 0..buffer.frames() {
            let size = self.size.advance();
            let damping = self.damping.advance();
            let predelay = self.predelay.advance() * per_ms;
            let mix = self.mix.advance();
            // Damping: a one-pole lowpass whose coefficient rises with
            // the knob, from nearly open to a few hundred hertz.
            let damp = 0.05 + 0.9 * damping * damping;

            let left = buffer.channel(0)[frame];
            let right = if channels > 1 {
                buffer.channel(1)[frame]
            } else {
                left
            };
            self.pre.push((left + right) * 0.5);
            let mut x = self.pre.read(predelay);
            for d in &mut self.diffusers {
                x = d.tick(x);
            }

            // Read every line, then mix with the Householder matrix:
            // each output is the input minus twice the mean.
            let mut outs = [0.0f32; LINES];
            let mut sum = 0.0f32;
            for i in 0..LINES {
                let length = self.length(i, size);
                lengths[i] = length;
                gains[i] = self.loop_gain(length);
                let modulated = length + wobble * self.wobble[i].value(0.0);
                self.wobble[i].advance();
                outs[i] = self.lines[i].read(modulated);
                sum += outs[i];
            }
            let mean2 = sum * (2.0 / LINES as f32);
            for i in 0..LINES {
                let mixed = outs[i] - mean2;
                // Damp, scale for the decay, and feed back with the input.
                self.damp_state[i] += (mixed - self.damp_state[i]) * (1.0 - damp);
                let back = self.damp_state[i] * gains[i];
                self.lines[i].push(x + back);
            }
            let wet_l = (outs[0] - outs[1] + outs[2] - outs[3] + outs[4] - outs[5] + outs[6]
                - outs[7])
                * 0.3;
            let wet_r =
                (outs[0] + outs[1] - outs[2] - outs[3] + outs[4] + outs[5] - outs[6] - outs[7])
                    * 0.3;
            buffer.channel_mut(0)[frame] = left * (1.0 - mix) + wet_l * mix;
            if channels > 1 {
                buffer.channel_mut(1)[frame] = right * (1.0 - mix) + wet_r * mix;
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

    fn energy_db(signal: &[f32]) -> f32 {
        10.0 * (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32)
            .max(1e-30)
            .log10()
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

    fn wet(decay: f32) -> Reverb {
        let mut r = Reverb::default();
        r.set_param(MIX, 100.0);
        r.set_param(DECAY, decay);
        r.set_param(PREDELAY, 0.0);
        r.set_param(DAMPING, 0.0);
        r
    }

    #[test]
    fn tail_decays_at_the_decay_time() {
        for decay in [0.5f32, 2.0] {
            let mut r = wet(decay);
            let mut input = vec![0.0; (decay * 1.5 * RATE) as usize];
            input[0] = 1.0;
            let out = Renderer::default().render(&mut r, &[input]);
            // Energy in 100 ms windows from 200 ms on falls at the slope
            // that reaches -60 dB at the decay time.
            let window = 4800;
            let a = energy_db(&out[0][2 * window..3 * window]);
            let b_at = (decay * RATE * 0.9) as usize;
            let b = energy_db(&out[0][b_at..b_at + window]);
            let seconds = (b_at as f32 - 2.0 * window as f32) / RATE;
            let slope = (a - b) / seconds;
            let expected = 60.0 / decay;
            assert!(
                (slope - expected).abs() < expected * 0.25,
                "decay {decay}: {slope:.1} dB/s, expected {expected:.1}"
            );
        }
    }

    #[test]
    fn the_tail_has_no_periodic_structure() {
        // A metallic ring is a repeating pattern in the tail: a comb
        // whose period you hear as a pitch. The impulse response's
        // tail, normalised, must not correlate with itself at any lag
        // from 1 ms to 100 ms.
        let mut r = wet(3.0);
        let mut input = vec![0.0; 72_000];
        input[0] = 1.0;
        let out = Renderer::default().render(&mut r, &[input]);
        let tail = &out[0][14_400..38_400];
        let power: f64 = tail.iter().map(|s| (s * s) as f64).sum();
        let mut worst = (0usize, 0.0f64);
        for lag in 48..4800 {
            let dot: f64 = tail[lag..]
                .iter()
                .zip(&tail[..tail.len() - lag])
                .map(|(a, b)| (a * b) as f64)
                .sum();
            let r = (dot / power).abs();
            if r > worst.1 {
                worst = (lag, r);
            }
        }
        eprintln!(
            "strongest self-correlation {:.3} at {:.1} ms",
            worst.1,
            worst.0 as f32 / 48.0
        );
        assert!(
            worst.1 < 0.15,
            "the tail repeats itself every {:.1} ms ({:.2})",
            worst.0 as f32 / 48.0,
            worst.1
        );
    }

    #[test]
    fn a_sustained_chord_rings_at_its_own_notes() {
        // A G major chord held for two seconds, then released. Half a
        // second later the tail is the chord's notes dying away (and
        // the modes a few hertz either side that a held note drives,
        // as in any room); nothing else may come within 15 dB of them.
        let chord = [196.0f32, 246.9, 293.7];
        let held = (2.0 * RATE) as usize;
        let input: Vec<f32> = (0..held + (1.5 * RATE) as usize)
            .map(|i| {
                if i >= held {
                    return 0.0;
                }
                chord
                    .iter()
                    .map(|f| (TAU * f * i as f32 / RATE).sin())
                    .sum::<f32>()
                    * 0.2
            })
            .collect();
        let mut r = wet(3.0);
        let out = Renderer::default().render(&mut r, &[input]);
        let tail = &out[0][held + 24_000..held + 48_000];
        // What rings is the mode nearest each note, so look a little
        // either side of it for the note's level.
        let quietest_note = chord
            .iter()
            .map(|f| {
                (-6..=6)
                    .map(|k| goertzel_db(tail, f * (1.0 + k as f32 * 0.005)))
                    .fold(f32::MIN, f32::max)
            })
            .fold(f32::MAX, f32::min);
        let partials: Vec<f32> = chord
            .iter()
            .flat_map(|f| (1..=12).map(move |n| f * n as f32))
            .collect();
        let mut worst = (0.0f32, f32::MIN);
        let mut hz = 150.0f32;
        while hz < 6000.0 {
            let near_partial = partials.iter().any(|p| (hz / p - 1.0).abs() < 0.08);
            if !near_partial {
                let level = goertzel_db(tail, hz);
                if level > worst.1 {
                    worst = (hz, level);
                }
            }
            hz *= 2f32.powf(1.0 / 24.0);
        }
        eprintln!(
            "quietest note {quietest_note:.1} dB, worst elsewhere {:.1} dB at {:.0} Hz",
            worst.1, worst.0
        );
        assert!(
            quietest_note - worst.1 > 15.0,
            "{:.0} Hz rings within {:.1} dB of the chord",
            worst.0,
            quietest_note - worst.1
        );
    }

    #[test]
    fn predelay_holds_the_wet_back_and_the_sides_differ() {
        let mut r = wet(1.0);
        r.set_param(PREDELAY, 50.0);
        let mut input = vec![0.0; 24_000];
        input[0] = 1.0;
        let out = Renderer::default().render(&mut r, &[input.clone(), input]);
        let first = out[0].iter().position(|s| s.abs() > 1e-4).unwrap();
        assert!(first >= 2400, "wet started at {first}");
        let diff: f32 = out[0].iter().zip(&out[1]).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0, "sides are the same");
        assert!(out[0].iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    #[test]
    fn dry_mix_is_untouched_and_damping_darkens() {
        let input: Vec<f32> = (0..24_000)
            .map(|i| 0.3 * (TAU * 300.0 * i as f32 / RATE).sin())
            .collect();
        let mut dry = Reverb::default();
        dry.set_param(MIX, 0.0);
        let out = Renderer::default().render(&mut dry, std::slice::from_ref(&input));
        assert_eq!(out[0], input);

        let mut impulse = vec![0.0; 48_000];
        impulse[0] = 1.0;
        let mut open = wet(2.0);
        let mut dark = wet(2.0);
        dark.set_param(DAMPING, 100.0);
        let a = Renderer::default().render(&mut open, std::slice::from_ref(&impulse));
        let b = Renderer::default().render(&mut dark, std::slice::from_ref(&impulse));
        let tail = 24_000..48_000;
        let bright_open =
            goertzel_db(&a[0][tail.clone()], 6000.0) - goertzel_db(&a[0][tail.clone()], 300.0);
        let bright_dark =
            goertzel_db(&b[0][tail.clone()], 6000.0) - goertzel_db(&b[0][tail], 300.0);
        assert!(
            bright_dark < bright_open - 10.0,
            "{bright_open} vs {bright_dark}"
        );
    }
}
