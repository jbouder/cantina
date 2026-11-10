//! Transcription: a recording into notes, with Basic Pitch.
//!
//! [Basic Pitch](https://github.com/spotify/basic-pitch) is Spotify's
//! note transcription model, in `models/basic-pitch.onnx` (Apache-2.0;
//! see `models/CREDITS.md`). It is compiled into the binary and run with
//! tract, a pure Rust inference library, on whatever thread calls
//! [`Transcriber::run`]: never the audio thread. Why this model and how
//! accurate it is: the transcription architecture page.
//!
//! The model takes mono audio at 22 050 Hz in windows of 43 844 samples
//! and gives, per frame of 256 samples (11.6 ms), three maps: how likely
//! each of 88 piano keys is sounding, how likely each is starting, and a
//! pitch contour at a third of a semitone. Windows overlap by 30 frames
//! and half the overlap is dropped from each side, as Basic Pitch's own
//! `inference.py` does.
//!
//! [`notes`] turns the maps into notes. It is a port of Basic Pitch's
//! `note_creation.py` (Copyright 2022 Spotify AB, Apache-2.0): onsets at
//! peaks of the onset map, inferred onsets where the frame map jumps,
//! each note followed forward until its frames fade, then the "melodia
//! trick" picking up sustained energy no onset explained. Each note's
//! bend is read from the contour around its pitch.

use std::io::Cursor;

use anyhow::{anyhow, Context, Result};
use tract_onnx::prelude::*;

/// The rate the model hears.
pub const RATE: f64 = 22_050.0;
/// Samples per window.
pub const WINDOW: usize = 43_844;
/// Samples per frame of output.
const HOP: usize = 256;
/// Frames of output per window.
const FRAMES_PER_WINDOW: usize = 172;
/// Frames shared by neighboring windows.
const OVERLAP_FRAMES: usize = 30;
/// Samples shared by neighboring windows.
const OVERLAP: usize = OVERLAP_FRAMES * HOP;
/// Samples from one window's start to the next.
const STEP: usize = WINDOW - OVERLAP;
/// Piano keys, from A0.
pub const KEYS: usize = 88;
/// MIDI note of the lowest key.
const LOWEST_KEY: u8 = 21;
/// Contour bins: three per key.
const CONTOUR_BINS: usize = 3 * KEYS;
/// Frames per second, as Basic Pitch rounds it.
const FPS: usize = 22_050 / HOP;

static MODEL: &[u8] = include_bytes!("../models/basic-pitch.onnx");

/// The model's three maps for a whole recording, frame by frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Activations {
    /// Frames.
    pub frames: usize,
    /// How likely each key is sounding: `frames × KEYS`.
    pub note: Vec<f32>,
    /// How likely each key is starting: `frames × KEYS`.
    pub onset: Vec<f32>,
    /// Pitch salience at a third of a semitone: `frames × 3·KEYS`.
    pub contour: Vec<f32>,
}

/// A note the model heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heard {
    /// Seconds from the start of the audio.
    pub start: f64,
    /// Seconds from the start of the audio.
    pub end: f64,
    /// The MIDI note.
    pub pitch: u8,
    /// Mean activation while it sounded, 0 to 1.
    pub amplitude: f32,
    /// How far the contour rose above the pitch, in semitones.
    pub bend: f32,
}

/// How [`notes`] reads the maps. The defaults are Basic Pitch's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// An onset peak at least this likely starts a note.
    pub onset_threshold: f32,
    /// A note lasts while its frames stay at least this likely.
    pub frame_threshold: f32,
    /// Notes this many frames long or shorter are dropped.
    pub min_frames: usize,
    /// Frames below the threshold before a note is over.
    pub energy_tolerance: usize,
    /// Lowest and highest MIDI notes to keep.
    pub range: Option<(u8, u8)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            onset_threshold: 0.5,
            frame_threshold: 0.3,
            // 127.7 ms, Basic Pitch's default, in frames.
            min_frames: 11,
            energy_tolerance: 11,
            range: None,
        }
    }
}

type Plan = std::sync::Arc<TypedRunnableModel>;

/// The model, loaded and optimized: about 60 ms, so load it once per
/// transcription, not once per window.
pub struct Transcriber {
    plan: Plan,
    /// Which of the plan's outputs are the note, onset and contour maps.
    outputs: [usize; 3],
}

impl Transcriber {
    /// Loads the model compiled into the binary.
    pub fn new() -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut Cursor::new(MODEL))
            .map_err(|e| anyhow!("{e}"))
            .context("reading the transcription model")?
            .with_input_fact(0, f32::fact([1, WINDOW, 1]).into())
            .map_err(|e| anyhow!("{e}"))?;
        // Basic Pitch names its outputs; the order is not promised.
        let names: Vec<String> = model
            .output_outlets()
            .map_err(|e| anyhow!("{e}"))?
            .iter()
            .map(|o| {
                model
                    .outlet_label(*o)
                    .unwrap_or(&model.node(o.node).name)
                    .to_string()
            })
            .collect();
        let find = |name: &str| {
            names
                .iter()
                .position(|n| n == name)
                .with_context(|| format!("the model has no output {name}; it has {names:?}"))
        };
        let outputs = [
            find("StatefulPartitionedCall:1")?,
            find("StatefulPartitionedCall:2")?,
            find("StatefulPartitionedCall:0")?,
        ];
        let plan = model
            .into_optimized()
            .and_then(|m| m.into_runnable())
            .map_err(|e| anyhow!("{e}"))
            .context("preparing the transcription model")?;
        Ok(Self { plan, outputs })
    }

    /// Runs the model over mono audio at [`RATE`]. `progress` hears the
    /// fraction done after each window and returns false to stop, which
    /// gives `Ok(None)`.
    pub fn run(
        &self,
        audio: &[f32],
        mut progress: impl FnMut(f32) -> bool,
    ) -> Result<Option<Activations>> {
        let mut padded = vec![0.0f32; OVERLAP / 2];
        padded.extend_from_slice(audio);
        let windows = padded.len().div_ceil(STEP).max(1);
        let keep = OVERLAP_FRAMES / 2..FRAMES_PER_WINDOW - OVERLAP_FRAMES / 2;
        let mut out = Activations {
            frames: 0,
            note: Vec::new(),
            onset: Vec::new(),
            contour: Vec::new(),
        };
        for w in 0..windows {
            let start = w * STEP;
            let mut window = vec![0.0f32; WINDOW];
            let end = (start + WINDOW).min(padded.len());
            if start < end {
                window[..end - start].copy_from_slice(&padded[start..end]);
            }
            let input = tract_ndarray::Array3::from_shape_vec((1, WINDOW, 1), window)?;
            let result = self
                .plan
                .run(tvec!(Tensor::from(input).into()))
                .map_err(|e| anyhow!("{e}"))
                .context("running the transcription model")?;
            for (map, (index, width)) in [&mut out.note, &mut out.onset, &mut out.contour]
                .into_iter()
                .zip(self.outputs.into_iter().zip([KEYS, KEYS, CONTOUR_BINS]))
            {
                let tensor = &result[index];
                let values = tensor
                    .try_as_plain_ram()
                    .and_then(|v| v.as_slice::<f32>().map(<[f32]>::to_vec))
                    .map_err(|e| anyhow!("{e}"))?;
                map.extend_from_slice(&values[keep.start * width..keep.end * width]);
            }
            out.frames += keep.len();
            if !progress((w + 1) as f32 / windows as f32) {
                return Ok(None);
            }
        }
        // Trim to the audio's own length.
        let frames = (audio.len() * FPS / RATE as usize).min(out.frames);
        out.note.truncate(frames * KEYS);
        out.onset.truncate(frames * KEYS);
        out.contour.truncate(frames * CONTOUR_BINS);
        out.frames = frames;
        Ok(Some(out))
    }
}

/// The time of each frame, in seconds, with Basic Pitch's correction for
/// the windows being a little shorter than two seconds.
fn frame_time(frame: usize) -> f64 {
    let window_frames = (FPS * 2) as f64;
    let offset = (HOP as f64 / RATE) * (window_frames - WINDOW as f64 / HOP as f64) + 0.0018;
    frame as f64 * HOP as f64 / RATE - offset * (frame as f64 / window_frames).floor()
}

/// The notes in the maps: Basic Pitch's `output_to_notes_polyphonic`
/// with inferred onsets and the melodia trick, then each note's bend
/// from the contour. Sorted by start, then pitch.
pub fn notes(a: &Activations, options: Options) -> Vec<Heard> {
    let n = a.frames;
    if n < 3 {
        return Vec::new();
    }
    let mut frames = a.note.clone();
    let mut onsets = a.onset.clone();
    if let Some((low, high)) = options.range {
        for t in 0..n {
            for k in 0..KEYS {
                let midi = k as u8 + LOWEST_KEY;
                if midi < low || midi > high {
                    frames[t * KEYS + k] = 0.0;
                    onsets[t * KEYS + k] = 0.0;
                }
            }
        }
    }

    // Inferred onsets: where the frames rise over two frames, scaled to
    // the onsets' peak, whichever is larger.
    let mut diff = vec![0.0f32; n * KEYS];
    for t in 2..n {
        for k in 0..KEYS {
            let now = frames[t * KEYS + k];
            let d1 = now - frames[(t - 1) * KEYS + k];
            let d2 = now - frames[(t - 2) * KEYS + k];
            diff[t * KEYS + k] = d1.min(d2).max(0.0);
        }
    }
    let onset_peak = onsets.iter().copied().fold(0.0f32, f32::max);
    let diff_peak = diff.iter().copied().fold(0.0f32, f32::max);
    if diff_peak > 0.0 {
        for (o, d) in onsets.iter_mut().zip(&diff) {
            *o = o.max(onset_peak * d / diff_peak);
        }
    }

    // Onsets at local maxima in time over the threshold, latest first.
    let mut starts = Vec::new();
    for t in (1..n - 1).rev() {
        for k in (0..KEYS).rev() {
            let o = onsets[t * KEYS + k];
            if o >= options.onset_threshold
                && o > onsets[(t - 1) * KEYS + k]
                && o > onsets[(t + 1) * KEYS + k]
            {
                starts.push((t, k));
            }
        }
    }

    let mut remaining = frames.clone();
    let mut events: Vec<(usize, usize, usize, f32)> = Vec::new();
    let mean = |from: usize, to: usize, k: usize| -> f32 {
        let sum: f32 = (from..to).map(|t| frames[t * KEYS + k]).sum();
        sum / (to - from).max(1) as f32
    };
    let clear = |remaining: &mut [f32], t: usize, k: usize| {
        remaining[t * KEYS + k] = 0.0;
        if k + 1 < KEYS {
            remaining[t * KEYS + k + 1] = 0.0;
        }
        if k > 0 {
            remaining[t * KEYS + k - 1] = 0.0;
        }
    };
    for (start, k) in starts {
        if start >= n - 1 {
            continue;
        }
        let mut i = start + 1;
        let mut quiet = 0;
        while i < n - 1 && quiet < options.energy_tolerance {
            if remaining[i * KEYS + k] < options.frame_threshold {
                quiet += 1;
            } else {
                quiet = 0;
            }
            i += 1;
        }
        i -= quiet;
        if i - start <= options.min_frames {
            continue;
        }
        for t in start..i {
            clear(&mut remaining, t, k);
        }
        events.push((start, i, k, mean(start, i, k)));
    }

    // The melodia trick: sustained energy with no onset of its own,
    // followed both ways from its loudest frame.
    loop {
        let (at, peak) =
            remaining
                .iter()
                .copied()
                .enumerate()
                .fold(
                    (0, f32::MIN),
                    |best, (i, v)| if v > best.1 { (i, v) } else { best },
                );
        if peak <= options.frame_threshold {
            break;
        }
        let (mid, k) = (at / KEYS, at % KEYS);
        remaining[at] = 0.0;
        let mut i = mid + 1;
        let mut quiet = 0;
        while i < n - 1 && quiet < options.energy_tolerance {
            if remaining[i * KEYS + k] < options.frame_threshold {
                quiet += 1;
            } else {
                quiet = 0;
            }
            clear(&mut remaining, i, k);
            i += 1;
        }
        let end = i - 1 - quiet;
        let mut i = mid as isize - 1;
        let mut quiet = 0;
        while i > 0 && quiet < options.energy_tolerance {
            if remaining[i as usize * KEYS + k] < options.frame_threshold {
                quiet += 1;
            } else {
                quiet = 0;
            }
            clear(&mut remaining, i as usize, k);
            i -= 1;
        }
        let begin = (i + 1 + quiet as isize).max(0) as usize;
        if end <= begin || end - begin <= options.min_frames {
            continue;
        }
        events.push((begin, end, k, mean(begin, end, k)));
    }

    let mut heard: Vec<Heard> = events
        .into_iter()
        .map(|(start, end, k, amplitude)| Heard {
            start: frame_time(start),
            end: frame_time(end),
            pitch: k as u8 + LOWEST_KEY,
            amplitude,
            bend: bend(a, start, end, k),
        })
        .collect();
    heard.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    heard
}

/// How far a note's pitch rose, in semitones: the contour's peak near
/// the note in each frame, weighted toward the note by a Gaussian as
/// Basic Pitch's `get_pitch_bends` does, smoothed over three frames so
/// one stray frame is not a bend, and the highest of those.
fn bend(a: &Activations, start: usize, end: usize, key: usize) -> f32 {
    const TOLERANCE: isize = 25;
    let center = (3 * key) as isize;
    let mut track = Vec::with_capacity(end.saturating_sub(start));
    for t in start..end.min(a.frames) {
        let mut best = (0isize, f32::MIN);
        for d in -TOLERANCE..=TOLERANCE {
            let bin = center + d;
            if !(0..CONTOUR_BINS as isize).contains(&bin) {
                continue;
            }
            let weight = (-(d * d) as f32 / (2.0 * 25.0)).exp();
            let v = a.contour[t * CONTOUR_BINS + bin as usize] * weight;
            if v > best.1 {
                best = (d, v);
            }
        }
        track.push(best.0 as f32 / 3.0);
    }
    if track.len() < 3 {
        return 0.0;
    }
    let mut rise = 0.0f32;
    for w in track.windows(3) {
        let mut sorted = [w[0], w[1], w[2]];
        sorted.sort_by(f32::total_cmp);
        rise = rise.max(sorted[1]);
    }
    rise
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, seconds: f64) -> Vec<f32> {
        let n = (seconds * RATE) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / RATE;
                let envelope = (-t * 3.0).exp() * (t * 200.0).min(1.0);
                // A few harmonics, as a string has.
                let s: f64 = (1..=4)
                    .map(|h| (std::f64::consts::TAU * hz * h as f64 * t).sin() / h as f64)
                    .sum();
                (0.4 * envelope * s) as f32
            })
            .collect()
    }

    #[test]
    fn the_model_loads_and_hears_a_note() {
        let t = Transcriber::new().unwrap();
        // A plucked A3 (220 Hz) half a second in.
        let mut audio = vec![0.0f32; (0.5 * RATE) as usize];
        audio.extend(tone(220.0, 1.5));
        let a = t.run(&audio, |_| true).unwrap().unwrap();
        assert_eq!(a.frames, audio.len() * FPS / RATE as usize);
        assert_eq!(a.note.len(), a.frames * KEYS);
        assert_eq!(a.contour.len(), a.frames * CONTOUR_BINS);
        let heard = notes(&a, Options::default());
        // Basic Pitch may split a long decay in two; the first A3 starts
        // with the pluck. (The port matches Basic Pitch's Python note for
        // note on the reference takes: scripts/transcribe-spike/parity.py.)
        let a3: Vec<&Heard> = heard.iter().filter(|h| h.pitch == 57).collect();
        assert!(!a3.is_empty(), "{heard:?}");
        assert!((a3[0].start - 0.5).abs() < 0.05, "{:?}", a3[0]);
        assert!(a3[0].bend.abs() < 0.5, "a still note: {:?}", a3[0]);
    }

    #[test]
    fn progress_can_stop_it() {
        let t = Transcriber::new().unwrap();
        let audio = vec![0.0f32; WINDOW * 3];
        let mut seen = Vec::new();
        let stopped = t
            .run(&audio, |f| {
                seen.push(f);
                false
            })
            .unwrap();
        assert!(stopped.is_none());
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn silence_has_no_notes() {
        let t = Transcriber::new().unwrap();
        let a = t.run(&vec![0.0; WINDOW], |_| true).unwrap().unwrap();
        assert!(notes(&a, Options::default()).is_empty());
    }

    #[test]
    fn frames_have_basic_pitchs_times() {
        assert_eq!(frame_time(0), 0.0);
        assert!((frame_time(86) - 86.0 * 256.0 / 22_050.0).abs() < 1e-9);
        // From the second window on, the correction applies.
        let corrected = 172.0 * 256.0 / 22_050.0 - 0.010_326;
        assert!((frame_time(172) - corrected).abs() < 1e-5);
    }
}
