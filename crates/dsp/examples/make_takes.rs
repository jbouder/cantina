//! Renders the transcription reference takes in `takes/` to audio.
//!
//! Each `takes/<id>.take.json` is hand-written ground truth: the notes a
//! player would read, in beats at a tempo, with strums, palm mutes, bends
//! and vibrato marked. This turns each one into `target/takes/<id>.wav`
//! (48 kHz mono) and `target/takes/<id>.truth.json`, the notes as they
//! actually sound in the file: onset and offset in seconds after the
//! strum spread and a few milliseconds of deterministic timing jitter,
//! pitch as a MIDI number at the start of the note.
//!
//! The strings are Karplus-Strong: a filtered noise burst through a
//! delay line one period long, with a pick-position comb, a loop filter
//! that sets the decay, a fractional read for bends and vibrato, and a
//! damped decay when the note is let go. Driven takes go through a
//! soft clipper and a cabinet-like roll-off. Synthesized strings are
//! cleaner than a real guitar, so numbers measured on them are an upper
//! bound on what a real take will score.
//!
//! Deterministic, so running it again writes the same files.
//!
//!     cargo run -p cantina-dsp --example make_takes

use std::path::Path;

use cantina_dsp::Biquad;
use serde_json::{json, Value};

const RATE: f32 = 48_000.0;
/// Silence before the first beat and after the last note's tail.
const LEAD: f32 = 0.25;
const TAIL: f32 = 0.6;

/// One sounding note, resolved from the take file.
struct Note {
    pitch: f32,
    onset: f32,
    offset: f32,
    velocity: f32,
    mute: bool,
    /// Semitones, start and ramp in seconds after the onset, release
    /// in seconds after the onset.
    bend: Option<(f32, f32, f32, Option<f32>)>,
    vibrato: bool,
}

/// A deterministic generator for noise and timing jitter.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

fn midi(name: &str) -> f32 {
    let bytes = name.as_bytes();
    let step = match bytes[0] {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => panic!("bad note name {name}"),
    };
    let (accidental, rest) = match bytes.get(1) {
        Some(b'#') => (1, &name[2..]),
        Some(b'b') => (-1, &name[2..]),
        _ => (0, &name[1..]),
    };
    let octave: i32 = rest.parse().expect("octave");
    ((octave + 1) * 12 + step + accidental) as f32
}

fn resolve(take: &Value, seed: u64) -> Vec<Note> {
    let tempo = take["tempo"].as_f64().unwrap() as f32;
    let beat = 60.0 / tempo;
    let strum = take["strum_ms"].as_f64().unwrap_or(10.0) as f32 / 1000.0;
    let mut jitter = Lcg(seed);
    let mut notes = Vec::new();
    for n in take["notes"].as_array().unwrap() {
        let start = LEAD + n["beat"].as_f64().unwrap() as f32 * beat + jitter.next() * 0.004;
        let end = start + n["length"].as_f64().unwrap() as f32 * beat;
        let pitches: Vec<f32> = match &n["pitch"] {
            Value::String(s) => vec![midi(s)],
            Value::Array(a) => a.iter().map(|p| midi(p.as_str().unwrap())).collect(),
            _ => panic!("pitch"),
        };
        let bend = n.get("bend").map(|b| {
            (
                b["semitones"].as_f64().unwrap() as f32,
                b["at"].as_f64().unwrap() as f32 * beat,
                b["ramp"].as_f64().unwrap() as f32 * beat,
                b.get("release").map(|r| r.as_f64().unwrap() as f32 * beat),
            )
        });
        for (i, &pitch) in pitches.iter().enumerate() {
            let onset = start + i as f32 * strum;
            notes.push(Note {
                pitch,
                onset,
                offset: end,
                velocity: n["velocity"].as_f64().unwrap_or(0.85) as f32
                    * (1.0 + jitter.next() * 0.08),
                mute: n["mute"].as_bool().unwrap_or(false),
                bend,
                vibrato: n["vibrato"].as_bool().unwrap_or(false),
            });
        }
    }
    notes
}

/// Seconds for a 60 dB decay, from the loop gain per period.
fn loop_gain(t60: f32, hz: f32) -> f32 {
    0.001f32.powf(1.0 / (t60 * hz))
}

fn pluck(out: &mut [f32], note: &Note, bass: bool, noise: &mut Lcg) {
    let base = 440.0 * 2f32.powf((note.pitch - 69.0) / 12.0);
    let size = (RATE / 20.0) as usize + 4;
    let mut line = vec![0.0f32; size];
    let mut write = 0usize;
    let first = (note.onset * RATE) as usize;
    let stop = ((note.offset + TAIL) * RATE) as usize;
    let period = RATE / base;

    // The excitation: a noise burst one period long, darker when soft,
    // through a comb for the pick's distance from the bridge.
    let burst = period as usize;
    let bright = if bass { 0.25 } else { 0.35 } + 0.5 * note.velocity;
    let bright = if note.mute { bright * 0.4 } else { bright };
    let mut lp = 0.0;
    let mut excitation: Vec<f32> = (0..burst)
        .map(|_| {
            lp += bright * (noise.next() - lp);
            lp
        })
        .collect();
    let pick = ((if bass { 0.22 } else { 0.14 }) * period) as usize;
    for i in (pick..burst).rev() {
        excitation[i] -= excitation[i - pick];
    }

    let ring = if bass { 5.0 } else { 3.5 } * (110.0 / base).sqrt().clamp(0.4, 1.6);
    let mut previous = 0.0;
    for n in first..stop.min(out.len()) {
        let t = (n - first) as f32 / RATE;
        let mut semis = 0.0;
        if let Some((amount, at, ramp, release)) = note.bend {
            let up = ((t - at) / ramp).clamp(0.0, 1.0);
            let down = release.map_or(0.0, |r| ((t - r) / ramp).clamp(0.0, 1.0));
            semis += amount * (up - down);
        }
        if note.vibrato {
            semis += 0.3 * (t * 5.5 * std::f32::consts::TAU).sin() * (t / 0.3).min(1.0);
        }
        let hz = base * 2f32.powf(semis / 12.0);
        let delay = RATE / hz - 0.5;
        let t60 = if n as f32 >= note.offset * RATE {
            0.05
        } else if note.mute {
            0.12
        } else {
            ring
        };
        let gain = loop_gain(t60, hz);

        let read = write as f32 + size as f32 - delay;
        let i = read.floor() as usize;
        let frac = read - read.floor();
        let a = line[i % size];
        let b = line[(i + 1) % size];
        let z = a + (b - a) * frac;
        let filtered = 0.5 * (z + previous);
        previous = z;
        let k = n - first;
        let input = if k < burst {
            excitation[k] * note.velocity
        } else {
            0.0
        };
        let y = input + gain * filtered;
        line[write] = y;
        write = (write + 1) % size;
        out[n] += y;
    }
}

fn render(take: &Value, notes: &[Note], seed: u64) -> Vec<f32> {
    let bass = take["instrument"] == "bass";
    let drive = take["drive"].as_f64().unwrap_or(0.0) as f32;
    let end = notes.iter().map(|n| n.offset).fold(0.0, f32::max) + TAIL + LEAD;
    let mut out = vec![0.0f32; (end * RATE) as usize];
    let mut noise = Lcg(seed ^ 0x9e37_79b9);
    for note in notes {
        pluck(&mut out, note, bass, &mut noise);
    }

    let mut low = Biquad::identity();
    low.set_highpass(RATE, if bass { 30.0 } else { 70.0 }, 0.7);
    let mut body = Biquad::identity();
    body.set_peaking(RATE, if bass { 80.0 } else { 110.0 }, 3.0, 1.0);
    let mut air = Biquad::identity();
    air.set_peaking(RATE, if bass { 700.0 } else { 220.0 }, 2.0, 1.5);
    let mut cab = [Biquad::identity(), Biquad::identity()];
    for c in &mut cab {
        c.set_lowpass(RATE, 4500.0, 0.7);
    }
    let pre = 1.0 + 40.0 * drive;
    for x in out.iter_mut() {
        let mut y = air.tick(body.tick(low.tick(*x)));
        if drive > 0.0 {
            y = (y * pre).tanh();
            y = cab[0].tick(y);
            y = cab[1].tick(y);
        }
        *x = y;
    }
    let peak = out.iter().fold(0.0f32, |p, x| p.max(x.abs())).max(1e-9);
    let scale = 0.89 / peak;
    for x in out.iter_mut() {
        // A noise floor 70 dB down, as a real interface has.
        *x = *x * scale + noise.next() * 0.0003;
    }
    out
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("takes");
    let target = root.join("target/takes");
    std::fs::create_dir_all(&target).expect("target/takes");
    let mut paths: Vec<_> = std::fs::read_dir(&source)
        .expect("takes/")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".take.json"))
        .collect();
    paths.sort();
    for path in paths {
        let id = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".take.json")
            .to_string();
        let take: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let seed = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
        });
        let notes = resolve(&take, seed);
        let audio = render(&take, &notes, seed);

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(target.join(format!("{id}.wav")), spec).unwrap();
        for x in &audio {
            wav.write_sample((x.clamp(-1.0, 1.0) * 8_388_607.0) as i32)
                .unwrap();
        }
        wav.finalize().unwrap();

        let truth: Vec<Value> = notes
            .iter()
            .map(|n| {
                json!({
                    "pitch": n.pitch,
                    "onset": n.onset,
                    "offset": n.offset,
                    "velocity": n.velocity,
                    "mute": n.mute,
                    "bend": n.bend.map(|b| b.0),
                    "vibrato": n.vibrato,
                })
            })
            .collect();
        let doc = json!({ "id": id, "instrument": take["instrument"], "notes": truth });
        std::fs::write(
            target.join(format!("{id}.truth.json")),
            serde_json::to_string_pretty(&doc).unwrap(),
        )
        .unwrap();
        println!(
            "{id}: {} notes, {:.1} s",
            notes.len(),
            audio.len() as f32 / RATE
        );
    }
}
