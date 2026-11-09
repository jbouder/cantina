//! The guitar DI fixture `render pedal --di` plays: a few seconds of a
//! plucked guitar as it leaves the instrument, before any amp.
//!
//! It is synthesised, not recorded, so it is ours to give away: CC0, like
//! everything else made here. Karplus-Strong strings, each pluck shaped
//! by where it is picked and damped a little by the palm, play a short
//! riff (open low E, a power chord, a strummed open chord and a few single
//! notes up the neck) at the level of a passive humbucker into a DI box:
//! peaks near -6 dBFS, quieter sustain.
//!
//! The bass DI, [`synthesize_bass`], is the same strings an octave down
//! playing a bass line: open E and A, a walk up to D and a few octaves,
//! at the level of a passive bass into a DI.
//!
//! `crates/cli/fixtures/di-guitar.wav` and `di-bass.wav` are [`synthesize`]
//! and [`synthesize_bass`] written as 16-bit PCM. A test regenerates them
//! and fails if a file differs; rewrite them with
//! `UPDATE_DI=1 cargo test -p cantina-cli di`.

/// The fixture's rate.
pub const DI_RATE: u32 = 48_000;

/// The fixture's length in seconds.
pub const DI_SECONDS: f32 = 4.0;

/// A note: when it is picked, its frequency, how hard, and how long it
/// rings before the hand stops it.
struct Pluck {
    at: f32,
    hz: f32,
    force: f32,
    ring: f32,
}

/// Standard tuning's open strings, low to high.
const E2: f32 = 82.41;
const A2: f32 = 110.0;
const D3: f32 = 146.83;
const G3: f32 = 196.0;
const B3: f32 = 246.94;
const E4: f32 = 329.63;

/// The note `semitones` above `open`.
fn fret(open: f32, semitones: i32) -> f32 {
    open * 2f32.powf(semitones as f32 / 12.0)
}

fn riff() -> Vec<Pluck> {
    let mut notes = Vec::new();
    let mut add = |at: f32, hz: f32, force: f32, ring: f32| {
        notes.push(Pluck {
            at,
            hz,
            force,
            ring,
        })
    };
    // Open low E, twice.
    add(0.0, E2, 1.0, 0.45);
    add(0.5, E2, 0.8, 0.45);
    // A power chord on A: A2, E3, A3, all at once.
    for (i, hz) in [A2, fret(A2, 7), fret(D3, 7)].into_iter().enumerate() {
        add(1.0 + i as f32 * 0.008, hz, 0.9, 0.9);
    }
    // A strummed open E major, low to high, 12 ms between strings.
    for (i, hz) in [E2, fret(A2, 2), fret(D3, 2), fret(G3, 1), B3, E4]
        .into_iter()
        .enumerate()
    {
        add(2.0 + i as f32 * 0.012, hz, 0.75, 1.0);
    }
    // Single notes up the G and B strings.
    for (i, (open, f)) in [(G3, 2), (G3, 4), (B3, 3), (B3, 5)].into_iter().enumerate() {
        add(3.1 + i as f32 * 0.2, fret(open, f), 0.7, 0.35);
    }
    notes
}

/// A bass's open strings, low to high.
const E1: f32 = 41.20;
const A1: f32 = 55.0;
const D2: f32 = 73.42;

fn bass_line() -> Vec<Pluck> {
    let mut notes = Vec::new();
    let mut add = |at: f32, hz: f32, force: f32, ring: f32| {
        notes.push(Pluck {
            at,
            hz,
            force,
            ring,
        })
    };
    // Eighths on open E, the last one an octave up.
    for i in 0..4 {
        let hz = if i == 3 { fret(E1, 12) } else { E1 };
        add(
            i as f32 * 0.25,
            hz,
            if i % 2 == 0 { 1.0 } else { 0.8 },
            0.22,
        );
    }
    // Open A, held.
    add(1.0, A1, 0.95, 0.9);
    // A walk up the A string to D: B, C#, then open D.
    for (i, hz) in [fret(A1, 2), fret(A1, 4), D2].into_iter().enumerate() {
        add(2.0 + i as f32 * 0.25, hz, 0.85, 0.22);
    }
    // Octaves on D, then a long low G at the 3rd fret of the E string.
    add(2.75, fret(D2, 12), 0.75, 0.2);
    add(3.0, fret(E1, 3), 1.0, 0.85);
    notes
}

/// The guitar fixture, mono at [`DI_RATE`].
pub fn synthesize() -> Vec<f32> {
    pluck_all(riff(), GUITAR)
}

/// The bass fixture, mono at [`DI_RATE`].
pub fn synthesize_bass() -> Vec<f32> {
    pluck_all(bass_line(), BASS)
}

/// How a hand and a pickup shape the strings.
struct Touch {
    /// A finger's softness: the pluck's noise is low-passed four times
    /// at this many times the note's frequency. Zero for a pick.
    softness: f32,
    /// How much of the last output sample the pickup keeps: more is darker.
    pickup: f32,
}

const GUITAR: Touch = Touch {
    softness: 0.0,
    pickup: 0.3,
};

const BASS: Touch = Touch {
    softness: 4.0,
    pickup: 0.6,
};

/// Plays `notes` on Karplus-Strong strings into a DI.
fn pluck_all(notes: Vec<Pluck>, touch: Touch) -> Vec<f32> {
    let rate = DI_RATE as f32;
    let frames = (DI_SECONDS * rate) as usize;
    let mut out = vec![0.0f32; frames];
    let mut seed = 0x2545_F491u32;
    for note in notes {
        let period = (rate / note.hz).round() as usize;
        // The pick: a burst of noise, low-passed more for a softer pick,
        // with a notch where the string is picked, a fifth of the way.
        let mut line: Vec<f32> = (0..period)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect();
        if touch.softness > 0.0 {
            let k = 1.0 - (-std::f32::consts::TAU * touch.softness * note.hz / rate).exp();
            for _ in 0..4 {
                // The string is a loop: start from its end.
                let mut y = line[period - 1];
                for s in line.iter_mut() {
                    y += k * (*s - y);
                    *s = y;
                }
            }
        }
        let pick = period / 5;
        let copy = line.clone();
        for (i, s) in line.iter_mut().enumerate() {
            *s -= copy[(i + pick) % period];
        }
        let start = (note.at * rate) as usize;
        let stop = start + (note.ring * rate) as usize;
        // Loss per pass: low strings sustain longer, as they do.
        let decay = 0.996 + 0.003 * (1.0 - (note.hz / 400.0).min(1.0));
        let mut i = 0usize;
        let mut prev = 0.0f32;
        let mut t = start;
        while t < frames {
            let k = i % period;
            let next = line[(k + 1) % period];
            // The string's own low-pass, a two-point average, and its loss.
            let y = decay * 0.5 * (line[k] + next);
            line[k] = y;
            // The hand stopping the note: a quick fade after `ring`.
            let damp = if t < stop {
                1.0
            } else {
                (-((t - stop) as f32) / (0.03 * rate)).exp()
            };
            if damp < 1e-4 {
                break;
            }
            // A gentle pickup-like smoothing of the output.
            let s = (1.0 - touch.pickup) * y + touch.pickup * prev;
            prev = s;
            out[t] += s * note.force * damp * 0.25;
            i += 1;
            t += 1;
        }
    }
    // Into the DI: peaks near -6 dBFS.
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let k = 0.5 / peak.max(1e-9);
    out.iter_mut().for_each(|s| *s *= k);
    out
}

/// Quantised to 16 bits, as the fixture file stores it.
pub fn quantize(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn di_fixtures_are_the_synthesised_riffs() {
        fixture_matches("di-guitar.wav", &synthesize());
        fixture_matches("di-bass.wav", &synthesize_bass());
    }

    fn fixture_matches(name: &str, samples: &[f32]) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        let expected = quantize(samples);
        if std::env::var_os("UPDATE_DI").is_some() {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: DI_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::create(&path, spec).unwrap();
            for s in &expected {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        let mut reader = hound::WavReader::open(&path).unwrap_or_else(|_| {
            panic!("fixtures/{name}; write it with UPDATE_DI=1 cargo test -p cantina-cli di")
        });
        let on_disk: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert!(
            on_disk == expected,
            "fixtures/{name} differs from the synthesis; UPDATE_DI=1 cargo test -p cantina-cli di"
        );
    }

    #[test]
    fn di_sounds_like_a_guitar_level() {
        di_level(&synthesize());
    }

    #[test]
    fn di_bass_sounds_like_a_bass_level() {
        let x = synthesize_bass();
        di_level(&x);
        // Most of its energy is below 200 Hz: a one-pole low-pass keeps it.
        let k = 1.0 - (-std::f32::consts::TAU * 200.0 / DI_RATE as f32).exp();
        let mut y = 0.0f32;
        let (mut low, mut all) = (0.0f32, 0.0f32);
        for s in &x {
            y += k * (s - y);
            low += y * y;
            all += s * s;
        }
        assert!(low / all > 0.5, "{}", low / all);
    }

    fn di_level(x: &[f32]) {
        assert_eq!(x.len(), (DI_SECONDS * DI_RATE as f32) as usize);
        let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 1e-4);
        let rms = (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
        assert!(rms > 0.03 && rms < 0.25, "rms {rms}");
        // Not silent anywhere for long: every quarter second has signal.
        for chunk in x.chunks(DI_RATE as usize / 4) {
            assert!(chunk.iter().any(|s| s.abs() > 0.01));
        }
    }
}
