//! A low-frequency oscillator for the modulation pedals, free-running
//! at a rate in hertz or locked to the transport at a note value.

use crate::processor::Transport;

/// Names for a `Unit::Choice` of shapes; index `i` is [`Shape::from_index`].
pub static SHAPE_NAMES: [&str; 5] = ["Sine", "Triangle", "Square", "Saw", "Random"];

/// Names for a `Unit::Choice` of note values; index `i` is
/// [`note_beats`]`(i)` beats per cycle.
pub static NOTE_NAMES: [&str; 12] = [
    "1/1", "1/2", "1/2T", "1/4", "1/4D", "1/4T", "1/8", "1/8D", "1/8T", "1/16", "1/16T", "1/32",
];

/// Quarter-note beats per cycle of a note value from [`NOTE_NAMES`].
pub fn note_beats(index: usize) -> f64 {
    const BEATS: [f64; 12] = [
        4.0,
        2.0,
        4.0 / 3.0,
        1.0,
        1.5,
        2.0 / 3.0,
        0.5,
        0.75,
        1.0 / 3.0,
        0.25,
        1.0 / 6.0,
        0.125,
    ];
    BEATS[index.min(BEATS.len() - 1)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Sine,
    Triangle,
    Square,
    Saw,
    /// A new random level each cycle: sample and hold.
    Random,
}

impl Shape {
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Shape::Triangle,
            2 => Shape::Square,
            3 => Shape::Saw,
            4 => Shape::Random,
            _ => Shape::Sine,
        }
    }

    /// The wave at `phase` (0 to 1), from -1 to 1. `held` is the level
    /// a random shape holds this cycle.
    #[inline]
    pub fn value(self, phase: f32, held: f32) -> f32 {
        match self {
            Shape::Sine => (phase * std::f32::consts::TAU).sin(),
            Shape::Triangle => {
                if phase < 0.5 {
                    4.0 * phase - 1.0
                } else {
                    3.0 - 4.0 * phase
                }
            }
            Shape::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Shape::Saw => 2.0 * phase - 1.0,
            Shape::Random => held,
        }
    }
}

/// The oscillator. Advance it once per frame; read it at any phase
/// offset for a second channel.
#[derive(Debug, Clone)]
pub struct Lfo {
    shape: Shape,
    phase: f32,
    increment: f32,
    sample_rate: f32,
    held: f32,
    seed: u32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub fn new() -> Self {
        let mut lfo = Self {
            shape: Shape::Sine,
            phase: 0.0,
            increment: 0.0,
            sample_rate: 48_000.0,
            held: 0.0,
            seed: 0x2545_F491,
        };
        lfo.reset();
        lfo
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        let hz = self.increment * self.sample_rate;
        self.sample_rate = sample_rate;
        self.set_rate_hz(hz);
        self.reset();
    }

    /// Back to the start of a cycle, with the random sequence restarted
    /// so a render is repeatable.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.seed = 0x2545_F491;
        self.held = self.next_random();
    }

    pub fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    pub fn set_rate_hz(&mut self, hz: f32) {
        self.increment = if self.sample_rate > 0.0 {
            hz.max(0.0) / self.sample_rate
        } else {
            0.0
        };
    }

    pub fn rate_hz(&self) -> f32 {
        self.increment * self.sample_rate
    }

    /// Runs at a note value: `beats_per_cycle` quarter notes per cycle
    /// at the transport's tempo. While the transport plays the phase is
    /// locked to the timeline, so a cycle starts on the beat; stopped,
    /// it free-runs at the tempo.
    pub fn sync(&mut self, transport: &Transport, beats_per_cycle: f64) {
        let hz = transport.bpm / 60.0 / beats_per_cycle.max(1e-6);
        self.set_rate_hz(hz as f32);
        if transport.playing {
            self.phase = (transport.beats / beats_per_cycle).rem_euclid(1.0) as f32;
        }
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// The wave now, `offset` cycles ahead (for a second channel).
    #[inline]
    pub fn value(&self, offset: f32) -> f32 {
        let phase = (self.phase + offset).rem_euclid(1.0);
        self.shape.value(phase, self.held)
    }

    /// Steps one frame.
    #[inline]
    pub fn advance(&mut self) {
        self.phase += self.increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held = self.next_random();
        }
    }

    fn next_random(&mut self) -> f32 {
        // xorshift32: plenty for a wobble.
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_span_minus_one_to_one_over_a_cycle() {
        for i in 0..5 {
            let shape = Shape::from_index(i);
            let samples: Vec<f32> = (0..1000)
                .map(|k| shape.value(k as f32 / 1000.0, 0.3))
                .collect();
            let (lo, hi) = samples
                .iter()
                .fold((1.0f32, -1.0f32), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            assert!(lo >= -1.0 && hi <= 1.0, "{shape:?}: {lo}..{hi}");
            if shape != Shape::Random {
                assert!(
                    lo < -0.99 && hi > 0.99,
                    "{shape:?} does not reach the rails"
                );
            }
        }
    }

    #[test]
    fn free_running_rate_is_right() {
        let mut lfo = Lfo::new();
        lfo.prepare(48_000.0);
        lfo.set_rate_hz(4.0);
        for _ in 0..12_000 {
            lfo.advance();
        }
        assert!(
            lfo.phase().abs() < 1e-3 || (lfo.phase() - 1.0).abs() < 1e-3,
            "{}",
            lfo.phase()
        );
    }

    #[test]
    fn sync_locks_the_phase_to_the_beat() {
        let mut lfo = Lfo::new();
        lfo.prepare(48_000.0);
        // A quarter-note cycle at 120 bpm: 2 Hz. Two and a half beats in.
        lfo.sync(
            &Transport {
                bpm: 120.0,
                beats: 2.5,
                playing: true,
            },
            note_beats(3),
        );
        assert!((lfo.rate_hz() - 2.0).abs() < 1e-5);
        assert!((lfo.phase() - 0.5).abs() < 1e-6);
        // A half-note cycle at the same point is a quarter through.
        lfo.sync(
            &Transport {
                bpm: 120.0,
                beats: 2.5,
                playing: true,
            },
            note_beats(1),
        );
        assert!((lfo.phase() - 0.25).abs() < 1e-6);
        // Stopped, the phase is left alone.
        lfo.sync(
            &Transport {
                bpm: 90.0,
                beats: 0.0,
                playing: false,
            },
            note_beats(3),
        );
        assert!((lfo.phase() - 0.25).abs() < 1e-6);
        assert!((lfo.rate_hz() - 1.5).abs() < 1e-5);
    }

    #[test]
    fn random_holds_a_level_per_cycle_and_repeats_after_reset() {
        let mut lfo = Lfo::new();
        lfo.prepare(1000.0);
        lfo.set_shape(Shape::Random);
        lfo.set_rate_hz(10.0);
        let run = |lfo: &mut Lfo| -> Vec<f32> {
            (0..300)
                .map(|_| {
                    let v = lfo.value(0.0);
                    lfo.advance();
                    v
                })
                .collect()
        };
        let a = run(&mut lfo);
        assert_eq!(a[0], a[50]);
        assert_ne!(a[0], a[150]);
        lfo.reset();
        assert_eq!(run(&mut lfo), a);
    }

    #[test]
    fn note_values_are_in_beats() {
        assert_eq!(note_beats(3), 1.0);
        assert_eq!(note_beats(0), 4.0);
        assert!((note_beats(5) - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(note_beats(99), 0.125);
        assert_eq!(NOTE_NAMES.len(), 12);
    }
}
