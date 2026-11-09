//! A plucked-string voice for playing tabs back: Karplus-Strong, one
//! voice pair per string.
//!
//! Each string is a delay line one period long, filled with a burst of
//! shaped noise when it is plucked and fed back through a gentle low-pass,
//! so the high harmonics die first as on a real string. The pitch is the
//! line's length, read at a fractional position, so bends and vibrato are
//! a change of read position and never a click. A string plays one note at
//! a time: a new note on it fades the old one out over a few milliseconds
//! in the other voice of its pair.
//!
//! Everything is allocated in [`Processor::prepare`]; [`Pluck::play`] and
//! `process` only index, so both are real-time safe. The noise comes from
//! a fixed seed per pluck, so a render is the same every time and golden
//! files can pin it.

use crate::processor::{Buffer, ParamId, ParamInfo, Processor, Unit};
use crate::smooth::Smoother;
use crate::{db_to_linear, linear_to_db};

static PARAMS: [ParamInfo; 1] = [ParamInfo::new("Level", Unit::Decibels, -60.0, 12.0, 0.0)];

const LEVEL: ParamId = 0;
const SMOOTHING_SECONDS: f32 = 0.005;

/// Strings the voice can play: an eight-string guitar.
pub const STRINGS: usize = 8;
/// Notes that can wait for one block. Beyond this they are dropped and
/// counted.
pub const MAX_PLUCKS: usize = 64;
/// The lowest pitch a string can sound, in Hz: below a 6-string bass's B.
const LOWEST_HZ: f64 = 25.0;
/// How long a note takes to fade when it ends or another takes its string.
const RELEASE_SECONDS: f32 = 0.03;
/// How long a cut voice takes to fade when a new note takes its string.
const STEAL_SECONDS: f32 = 0.006;
/// How long a bend takes to reach its pitch, and to come back.
const BEND_SECONDS: f32 = 0.12;
/// Vibrato: how deep (semitones), how fast (Hz), and after how long.
const VIBRATO_DEPTH: f32 = 0.3;
const VIBRATO_HZ: f32 = 5.5;
const VIBRATO_DELAY_SECONDS: f32 = 0.12;
/// Where the string is picked, as a fraction of its length: the comb
/// this makes gives a guitar its nasal edge.
const PICK_POSITION: f32 = 0.13;

/// Which instrument the voice imitates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    /// Steel strings: bright, a few seconds of ring.
    Guitar,
    /// Round wound bass strings: darker and longer.
    Bass,
}

/// A note to play: where, how loud, for how long and with what technique.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluckNote {
    /// 1 for the highest string, up to [`STRINGS`].
    pub string: u8,
    /// The pitch as a MIDI note; a harmonic is given at the pitch it sounds.
    pub pitch: f32,
    /// 0 to 1.
    pub velocity: f32,
    /// Frames it sounds before it is let go.
    pub length: u32,
    /// Semitones it bends up to; 0 for none.
    pub bend: f32,
    /// The bend comes back down before the note ends.
    pub release: bool,
    /// Vibrato after a moment.
    pub vibrato: bool,
    /// Damped at the bridge: short and dark.
    pub palm_mute: bool,
    /// A muted click with no pitch to speak of.
    pub dead: bool,
    /// A natural harmonic: a pure, bell-like tone.
    pub harmonic: bool,
}

impl PluckNote {
    /// A plain note.
    pub fn new(string: u8, pitch: f32, velocity: f32, length: u32) -> Self {
        Self {
            string,
            pitch,
            velocity,
            length,
            bend: 0.0,
            release: false,
            vibrato: false,
            palm_mute: false,
            dead: false,
            harmonic: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    note: PluckNote,
    frame: usize,
}

/// One string sounding: a delay line and the note's state.
#[derive(Debug, Clone, Default)]
struct Voice {
    active: bool,
    line: Vec<f32>,
    write: usize,
    /// The previous filtered sample, for the loop's low-pass.
    last: f32,
    note: Option<PluckNote>,
    /// Frames since the pluck.
    age: u32,
    /// The weight of the newest sample in the loop's two-point filter:
    /// 0.5 is darkest, 1 lets everything through.
    brightness: f32,
    /// Ring time to -60 dB, in seconds, at the note's pitch.
    t60: f32,
    /// The loop's length at the note's own pitch, in samples, and the
    /// loss per pass round it; worked out once per pluck.
    delay: f64,
    per_pass: f32,
    /// 1 while sounding, falling to 0 once let go.
    fade: f32,
    fade_step: f32,
}

/// The tab voice. Queue notes with [`Pluck::play`], then process.
pub struct Pluck {
    instrument: Instrument,
    level: Smoother,
    sample_rate: f64,
    /// Two per string: the note sounding and the one fading out.
    voices: Vec<Voice>,
    pending: [Pending; MAX_PLUCKS],
    queued: usize,
    dropped: u64,
    /// Plucks so far, which seed each pluck's noise.
    plucks: u32,
}

impl Pluck {
    /// A voice for `instrument`, at 48 kHz until it is prepared.
    pub fn new(instrument: Instrument) -> Self {
        let mut pluck = Self {
            instrument,
            level: Smoother::new(db_to_linear(PARAMS[LEVEL].default)),
            sample_rate: 48_000.0,
            voices: Vec::new(),
            pending: [Pending {
                note: PluckNote::new(1, 0.0, 0.0, 0),
                frame: 0,
            }; MAX_PLUCKS],
            queued: 0,
            dropped: 0,
            plucks: 0,
        };
        pluck.allocate();
        pluck
    }

    /// The instrument it imitates.
    pub fn instrument(&self) -> Instrument {
        self.instrument
    }

    /// Queues a note to start `frame` frames into the next block. Returns
    /// false, and plays nothing, for a string out of range, a velocity of
    /// zero or a full queue. Real-time safe.
    pub fn play(&mut self, note: PluckNote, frame: usize) -> bool {
        let string = usize::from(note.string);
        if !(1..=STRINGS).contains(&string)
            || note.velocity.is_nan()
            || note.velocity <= 0.0
            || !note.pitch.is_finite()
        {
            return false;
        }
        if self.queued >= MAX_PLUCKS {
            self.dropped += 1;
            return false;
        }
        self.pending[self.queued] = Pending { note, frame };
        self.queued += 1;
        true
    }

    /// Lets every string go, quickly but without a click: the transport
    /// stopped or jumped. Real-time safe.
    pub fn release_all(&mut self) {
        self.queued = 0;
        let step = 1.0 / (RELEASE_SECONDS * self.sample_rate as f32).max(1.0);
        for voice in &mut self.voices {
            if voice.active && voice.fade_step == 0.0 {
                voice.fade_step = step;
            }
        }
    }

    /// Strings sounding now.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Notes refused because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    fn allocate(&mut self) {
        let length = (self.sample_rate / LOWEST_HZ).ceil() as usize + 4;
        self.voices = (0..STRINGS * 2)
            .map(|_| Voice {
                line: vec![0.0; length],
                ..Voice::default()
            })
            .collect();
    }

    fn silence(&mut self) {
        self.queued = 0;
        for voice in &mut self.voices {
            voice.active = false;
            voice.line.fill(0.0);
            voice.last = 0.0;
        }
    }

    /// Starts a note on its string: the voice sounding there now fades
    /// out, and the other voice of the pair is plucked.
    fn start(&mut self, note: PluckNote) {
        let sample_rate = self.sample_rate as f32;
        let pair = (usize::from(note.string) - 1) * 2;
        let steal = 1.0 / (STEAL_SECONDS * sample_rate).max(1.0);
        // The voice to use: an idle one, else the quieter.
        let (a, b) = (pair, pair + 1);
        let slot = match (self.voices[a].active, self.voices[b].active) {
            (false, _) => a,
            (true, false) => b,
            (true, true) => {
                if self.voices[a].fade <= self.voices[b].fade {
                    a
                } else {
                    b
                }
            }
        };
        let other = if slot == a { b } else { a };
        if self.voices[other].active {
            let v = &mut self.voices[other];
            v.fade_step = v.fade_step.max(steal);
        }

        self.plucks = self.plucks.wrapping_add(1);
        let mut seed =
            0x9E37_79B9u32 ^ self.plucks.wrapping_mul(0x85EB_CA6B) ^ u32::from(note.string);
        let velocity = note.velocity.clamp(0.0, 1.0);
        let (brightness, t60) = self.timbre(&note, velocity);
        let hz = midi_to_hz(note.pitch);
        let period = (self.sample_rate / hz).max(2.0);

        let voice = &mut self.voices[slot];
        let size = voice.line.len();
        let n = (period.round() as usize).clamp(2, size - 2);
        voice.line.fill(0.0);
        voice.write = 0;
        voice.last = 0.0;
        // The burst: noise, darker when soft, rounder for a harmonic, and
        // combed at the pick position. It goes where the reader will find
        // it over the first period.
        let smooth = if note.harmonic {
            0.85
        } else if note.dead {
            0.3
        } else {
            0.6 - 0.45 * velocity
        };
        let mut low = 0.0f32;
        let start = size - n;
        for i in 0..n {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let white = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
            low = smooth * low + (1.0 - smooth) * white;
            voice.line[start + i] = low;
        }
        let pick = ((n as f32 * PICK_POSITION) as usize).max(1);
        for i in (pick..n).rev() {
            voice.line[start + i] -= voice.line[start + i - pick];
        }
        let mean = voice.line[start..].iter().sum::<f32>() / n as f32;
        let peak = voice.line[start..]
            .iter()
            .fold(0.0f32, |m, s| m.max((s - mean).abs()))
            .max(1e-6);
        let amplitude = 0.25 * velocity;
        for s in &mut voice.line[start..] {
            *s = (*s - mean) / peak * amplitude;
        }
        voice.active = true;
        voice.note = Some(note);
        voice.age = 0;
        voice.brightness = brightness;
        voice.t60 = t60;
        voice.delay = loop_delay(self.sample_rate, hz, brightness, size);
        voice.per_pass = 10f32.powf(-3.0 / (t60 * hz as f32));
        voice.fade = 1.0;
        voice.fade_step = 0.0;
    }

    /// How bright the loop is and how long the note rings.
    fn timbre(&self, note: &PluckNote, velocity: f32) -> (f32, f32) {
        if note.dead {
            return (0.5, 0.025);
        }
        if note.palm_mute {
            return (0.52, 0.22);
        }
        // Low notes ring longer than high ones, as on a real neck.
        let (base, reference, bright) = match self.instrument {
            Instrument::Guitar => (3.5, 40.0, 0.62 + 0.3 * velocity),
            Instrument::Bass => (5.0, 28.0, 0.53 + 0.15 * velocity),
        };
        let t60 = (base * 2f32.powf(-(note.pitch - reference) / 24.0)).clamp(0.6, 8.0);
        let bright = if note.harmonic { 0.97 } else { bright };
        (bright.min(0.98), t60)
    }
}

/// The loop's length for `hz`: one period, less the half sample or so
/// the two-point filter delays it by (`1 - brightness`).
fn loop_delay(sample_rate: f64, hz: f64, brightness: f32, size: usize) -> f64 {
    (sample_rate / hz - f64::from(1.0 - brightness)).clamp(2.0, (size - 2) as f64)
}

fn midi_to_hz(pitch: f32) -> f64 {
    440.0 * 2f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

/// The pitch offset in semitones `age` frames into a note.
fn bend_at(note: &PluckNote, age: u32, sample_rate: f32) -> f32 {
    let seconds = age as f32 / sample_rate;
    let mut offset = 0.0;
    if note.bend > 0.0 {
        let rise = (seconds / BEND_SECONDS).min(1.0);
        let mut amount = rise * rise * (3.0 - 2.0 * rise);
        if note.release {
            // Back down over the last part of the note.
            let back_at = note.length as f32 / sample_rate * 0.6;
            let fall = ((seconds - back_at) / BEND_SECONDS).clamp(0.0, 1.0);
            amount *= 1.0 - fall * fall * (3.0 - 2.0 * fall);
        }
        offset += note.bend * amount;
    }
    if note.vibrato && seconds > VIBRATO_DELAY_SECONDS {
        let t = seconds - VIBRATO_DELAY_SECONDS;
        let swell = (t / 0.2).min(1.0);
        offset += VIBRATO_DEPTH * swell * (std::f32::consts::TAU * VIBRATO_HZ * t).sin();
    }
    offset
}

impl Processor for Pluck {
    fn name(&self) -> &str {
        "Pluck"
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
        self.level.set_time(SMOOTHING_SECONDS, sample_rate as f32);
        self.level.snap(self.level.target());
        self.allocate();
        self.silence();
    }

    fn reset(&mut self) {
        self.silence();
    }

    /// Adds the sounding strings, and the queued notes from their frames
    /// on, to every channel of the buffer.
    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if frames == 0 {
            self.queued = 0;
            return;
        }
        let n = self.queued;
        // In frame order; the queue is small and nearly sorted.
        for i in 1..n {
            let mut j = i;
            while j > 0 && self.pending[j - 1].frame > self.pending[j].frame {
                self.pending.swap(j - 1, j);
                j -= 1;
            }
        }
        self.queued = 0;
        let sample_rate = self.sample_rate as f32;
        let release = 1.0 / (RELEASE_SECONDS * sample_rate).max(1.0);
        let channels = buffer.channels();
        let mut next = 0;
        for frame in 0..frames {
            while next < n && self.pending[next].frame.min(frames - 1) == frame {
                let note = self.pending[next].note;
                self.start(note);
                next += 1;
            }
            let level = self.level.advance();
            let mut out = 0.0f32;
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }
                let Some(note) = voice.note else {
                    voice.active = false;
                    continue;
                };
                let size = voice.line.len();
                // Bends and vibrato move the read position; otherwise
                // the loop stays the length the pluck set.
                let offset = bend_at(&note, voice.age, sample_rate);
                let delay = if offset == 0.0 {
                    voice.delay
                } else {
                    let hz = midi_to_hz(note.pitch + offset);
                    loop_delay(self.sample_rate, hz, voice.brightness, size)
                };
                let read = voice.write as f64 + size as f64 - delay;
                let i = read.floor();
                let frac = (read - i) as f32;
                let i = i as usize % size;
                let y = voice.line[i] * (1.0 - frac) + voice.line[(i + 1) % size] * frac;
                let filtered = voice.brightness * y + (1.0 - voice.brightness) * voice.last;
                voice.last = y;
                voice.line[voice.write] = filtered * voice.per_pass;
                voice.write = (voice.write + 1) % size;

                out += y * voice.fade;
                voice.age = voice.age.saturating_add(1);
                if voice.age >= note.length && voice.fade_step == 0.0 {
                    voice.fade_step = release;
                }
                if voice.fade_step > 0.0 {
                    voice.fade -= voice.fade_step;
                    if voice.fade <= 0.0 {
                        voice.fade = 0.0;
                        voice.active = false;
                    }
                }
            }
            let sample = out * level;
            for c in 0..channels {
                buffer.channel_mut(c)[frame] += sample;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn render(pluck: &mut Pluck, frames: usize, notes: &[(PluckNote, usize)]) -> Vec<f32> {
        pluck.prepare(SR, 64);
        let mut out = vec![0.0f32; frames];
        let mut at = 0;
        while at < frames {
            let n = 64.min(frames - at);
            for (note, frame) in notes {
                if (at..at + n).contains(frame) {
                    assert!(pluck.play(*note, frame - at));
                }
            }
            let mut chunk = [&mut out[at..at + n]];
            pluck.process(&mut Buffer::new(&mut chunk));
            at += n;
        }
        out
    }

    /// The fundamental by autocorrelation, in Hz.
    fn pitch_of(samples: &[f32]) -> f64 {
        let (lo, hi) = ((SR / 1000.0) as usize, (SR / 30.0) as usize);
        let mut best = (0.0f32, lo);
        for lag in lo..hi {
            let c: f32 = samples
                .iter()
                .zip(&samples[lag..])
                .map(|(a, b)| a * b)
                .sum();
            if c > best.0 {
                best = (c, lag);
            }
        }
        SR / best.1 as f64
    }

    #[test]
    fn a_pluck_sounds_its_pitch_and_dies_away() {
        let mut pluck = Pluck::new(Instrument::Guitar);
        // A on the 5th fret of the low E: 110 Hz.
        let out = render(
            &mut pluck,
            48_000,
            &[(PluckNote::new(6, 45.0, 0.8, 48_000), 0)],
        );
        let hz = pitch_of(&out[4_800..14_400]);
        assert!((hz - 110.0).abs() < 1.5, "{hz} Hz");
        let early: f32 = out[..4_800].iter().map(|s| s * s).sum();
        let late: f32 = out[38_400..43_200].iter().map(|s| s * s).sum();
        assert!(late < early * 0.3, "it decays: {early} then {late}");
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 1.0));
    }

    #[test]
    fn a_bend_raises_the_pitch() {
        let mut pluck = Pluck::new(Instrument::Guitar);
        let mut note = PluckNote::new(2, 64.0, 0.8, 48_000);
        note.bend = 2.0;
        let out = render(&mut pluck, 24_000, &[(note, 0)]);
        let hz = pitch_of(&out[14_400..24_000]);
        let want = midi_to_hz(66.0);
        assert!((hz - want).abs() < want * 0.02, "{hz} Hz, want {want}");
    }

    #[test]
    fn palm_mutes_and_dead_notes_are_short() {
        let energy_after = |note: PluckNote| {
            let mut pluck = Pluck::new(Instrument::Guitar);
            let out = render(&mut pluck, 24_000, &[(note, 0)]);
            out[12_000..].iter().map(|s| s * s).sum::<f32>()
        };
        let open = energy_after(PluckNote::new(6, 40.0, 0.8, 48_000));
        let mut muted = PluckNote::new(6, 40.0, 0.8, 48_000);
        muted.palm_mute = true;
        let mut dead = PluckNote::new(6, 40.0, 0.8, 48_000);
        dead.dead = true;
        assert!(energy_after(muted) < open * 0.05);
        assert!(energy_after(dead) < open * 0.001);
    }

    #[test]
    fn a_note_on_a_string_cuts_the_one_before_and_notes_end() {
        let mut pluck = Pluck::new(Instrument::Bass);
        let out = render(
            &mut pluck,
            24_000,
            &[
                (PluckNote::new(4, 28.0, 0.8, 4_800), 0),
                (PluckNote::new(4, 33.0, 0.8, 4_800), 2_400),
            ],
        );
        assert!(pluck.active_voices() == 0, "both ended");
        assert!(out[20_000..].iter().all(|s| s.abs() < 1e-6));
        // Nothing jumps when the second note cuts the first.
        let worst = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(worst < 0.5, "{worst}");
    }

    #[test]
    fn renders_are_the_same_every_time_and_refuse_bad_notes() {
        let notes = [(PluckNote::new(1, 64.0, 0.7, 9_600), 10)];
        let a = render(&mut Pluck::new(Instrument::Guitar), 9_600, &notes);
        let b = render(&mut Pluck::new(Instrument::Guitar), 9_600, &notes);
        assert_eq!(a, b);
        let mut pluck = Pluck::new(Instrument::Guitar);
        assert!(!pluck.play(PluckNote::new(0, 64.0, 0.7, 10), 0));
        assert!(!pluck.play(PluckNote::new(9, 64.0, 0.7, 10), 0));
        assert!(!pluck.play(PluckNote::new(1, 64.0, 0.0, 10), 0));
    }
}
