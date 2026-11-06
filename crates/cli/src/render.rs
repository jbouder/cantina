//! `render`: a pedal or a pattern played offline through the processors
//! the app builds, and the numbers that say whether it worked.
//!
//! A pedal is compiled with the same `compile` the app uses and run in
//! 64-frame blocks at 48 kHz (or the input file's rate), with the
//! transport rolling at the given tempo so synced modulation moves. A
//! pattern plays on the engine's `Sequencer`, which drives the same
//! `Sampler` a drum track does, under a tempo map, for a number of passes
//! plus a tail for the last hits to ring out.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use cantina_dsp::{compile, Buffer, PedalDef, Processor, Transport, MAX_CHANNELS};
use cantina_engine::{
    BlockContext, Node, PatternData, Placement, Sequencer, TempoMap, TimeSignature,
};
use cantina_project::Pattern;
use serde::Serialize;

/// The block size renders run in, the app's usual buffer.
pub const BLOCK: usize = 64;
/// The rate renders run at unless the input file says otherwise.
pub const RATE: f64 = 48_000.0;

/// What a pedal is fed.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The guitar DI fixture.
    Di,
    /// A sine at this frequency, at -12 dBFS.
    Tone(f32),
    /// Channels and their rate, from a file.
    Audio(f64, Vec<Vec<f32>>),
}

impl Source {
    /// The source's channels and rate, `seconds` long for a tone.
    pub fn signal(&self, seconds: f32) -> (f64, Vec<Vec<f32>>) {
        match self {
            Source::Di => (
                crate::di::DI_RATE as f64,
                vec![crate::di::quantize(&crate::di::synthesize())
                    .into_iter()
                    .map(|s| s as f32 / i16::MAX as f32)
                    .collect()],
            ),
            Source::Tone(hz) => {
                let n = (seconds as f64 * RATE) as usize;
                let k = std::f32::consts::TAU * hz / RATE as f32;
                (
                    RATE,
                    vec![(0..n).map(|i| 0.251 * (k * i as f32).sin()).collect()],
                )
            }
            Source::Audio(rate, channels) => (*rate, channels.clone()),
        }
    }
}

/// The numbers a render is judged by.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Frames rendered.
    pub frames: usize,
    /// Channels rendered.
    pub channels: usize,
    /// The rate rendered at.
    pub sample_rate: f64,
    /// The loudest sample, in dBFS.
    pub peak_db: f32,
    /// The level over the whole render, in dBFS.
    pub rms_db: f32,
    /// The mean sample value: anything but a tiny number is a DC offset.
    pub dc_offset: f32,
    /// Samples beyond full scale, which clip when written to a device.
    pub clipped: usize,
    /// Samples that are not numbers. Any is a broken pedal.
    pub non_finite: usize,
    /// True when nothing rose above -80 dBFS.
    pub silent: bool,
    /// What the processor reports, in samples. Zero for a pattern.
    pub latency: usize,
}

impl Stats {
    /// Measures `channels`.
    pub fn of(channels: &[Vec<f32>], sample_rate: f64, latency: usize) -> Stats {
        let mut peak = 0.0f32;
        let (mut sum, mut squares, mut n) = (0.0f64, 0.0f64, 0usize);
        let (mut clipped, mut non_finite) = (0, 0);
        for s in channels.iter().flatten() {
            if !s.is_finite() {
                non_finite += 1;
                continue;
            }
            peak = peak.max(s.abs());
            if s.abs() > 1.0 {
                clipped += 1;
            }
            sum += *s as f64;
            squares += (*s as f64) * (*s as f64);
            n += 1;
        }
        let n = n.max(1) as f64;
        let db = |x: f32| 20.0 * x.max(1e-10).log10();
        let peak_db = db(peak);
        Stats {
            frames: channels.first().map_or(0, Vec::len),
            channels: channels.len(),
            sample_rate,
            peak_db,
            rms_db: db((squares / n).sqrt() as f32),
            dc_offset: (sum / n) as f32,
            clipped,
            non_finite,
            silent: peak_db < -80.0,
            latency,
        }
    }

    /// One line per number, for people.
    pub fn text(&self) -> String {
        format!(
            "{} frames, {} channel{} at {} Hz ({:.2} s)\n\
             peak     {:.1} dBFS\n\
             rms      {:.1} dBFS\n\
             dc       {:.6}\n\
             latency  {} samples\n\
             clipped  {}\n\
             {}",
            self.frames,
            self.channels,
            if self.channels == 1 { "" } else { "s" },
            self.sample_rate,
            self.frames as f64 / self.sample_rate,
            self.peak_db,
            self.rms_db,
            self.dc_offset,
            self.latency,
            if self.clipped == 0 {
                "no".to_string()
            } else {
                format!("{} samples over full scale", self.clipped)
            },
            match (self.non_finite, self.silent) {
                (0, false) => "ok".to_string(),
                (0, true) => "silent: nothing above -80 dBFS".to_string(),
                (n, _) => format!("{n} samples are not numbers"),
            }
        )
    }

    /// Whether the render is broken: not numbers, or silence.
    pub fn failed(&self) -> bool {
        self.non_finite > 0 || self.silent
    }
}

/// Sets a parameter by its id or label, case-insensitively, or by index.
pub fn set_param(pedal: &mut dyn Processor, def: &PedalDef, name: &str, value: f32) -> Result<()> {
    let wanted = name.to_lowercase();
    let index = def
        .params
        .iter()
        .position(|p| p.id.to_lowercase() == wanted || p.label.to_lowercase() == wanted)
        .or_else(|| name.parse::<usize>().ok().filter(|i| *i < def.params.len()));
    let Some(index) = index else {
        let names: Vec<&str> = def.params.iter().map(|p| p.id.as_str()).collect();
        bail!(
            "{:?} has no parameter {name:?}; it has {}",
            def.name,
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        );
    };
    pedal.set_param(index, value);
    Ok(())
}

/// Runs a pedal over `input` plus `tail` frames of silence, with the
/// transport rolling at `bpm`. Returns the output and the latency the
/// pedal reports.
pub fn render_pedal(
    def: &PedalDef,
    settings: &[(String, f32)],
    sample_rate: f64,
    input: &[Vec<f32>],
    tail: usize,
    bpm: f64,
) -> Result<(Vec<Vec<f32>>, usize)> {
    let mut pedal = compile(def).map_err(|e| anyhow::anyhow!("{e}"))?;
    for (name, value) in settings {
        set_param(&mut pedal, def, name, *value)?;
    }
    pedal.prepare(sample_rate, BLOCK);
    let mut channels: Vec<Vec<f32>> = input
        .iter()
        .take(MAX_CHANNELS)
        .map(|c| {
            let mut c = c.clone();
            c.resize(c.len() + tail, 0.0);
            c
        })
        .collect();
    let frames = channels.first().map_or(0, Vec::len);
    let mut start = 0;
    while start < frames {
        let end = (start + BLOCK).min(frames);
        pedal.set_transport(&Transport {
            bpm,
            beats: start as f64 / sample_rate * bpm / 60.0,
            playing: true,
        });
        let mut lanes: Vec<&mut [f32]> = channels.iter_mut().map(|c| &mut c[start..end]).collect();
        pedal.process(&mut Buffer::new(&mut lanes));
        start = end;
    }
    Ok((channels, pedal.latency()))
}

/// Plays `pattern` through the kit in `kit_dir` for `passes` passes at
/// `bpm` in 4/4, plus `tail_seconds` for the last hits to ring. Stereo.
/// Also returns the rows whose pads the kit does not have, which are
/// left out as they are in the app.
pub fn render_pattern(
    pattern: &Pattern,
    kit_dir: &Path,
    bpm: f64,
    passes: u32,
    tail_seconds: f64,
) -> Result<(Vec<Vec<f32>>, Vec<String>)> {
    let (_, loaded) = cantina_engine::kit::load(kit_dir)?;
    let loaded = Arc::new(loaded);
    let mut missing = Vec::new();
    let rows = pattern
        .rows
        .iter()
        .filter_map(|r| match loaded.pad_index(&r.pad) {
            Some(i) => Some((i, r.steps.clone())),
            None => {
                missing.push(r.pad.clone());
                None
            }
        })
        .collect();
    let data = Arc::new(PatternData {
        steps_per_beat: pattern.steps_per_beat as u32,
        length: pattern.length as u32,
        swing: pattern.swing,
        rows,
    });
    let ticks = data.ticks() * passes.max(1) as u64;
    let (mut sequencer, mut handle) = Sequencer::new(Arc::clone(&loaded));
    if !handle.set_score(vec![Placement {
        pattern: data,
        start: 0,
        offset: 0,
        length: ticks,
    }]) {
        bail!("the sequencer would not take the pattern");
    }
    let tempo = TempoMap::new(bpm, TimeSignature::new(4, 4));
    let frames =
        tempo.ticks_to_samples(ticks as f64, RATE) as usize + (tail_seconds * RATE) as usize;
    sequencer.prepare(RATE, BLOCK);
    let mut out = vec![vec![0.0f32; frames]; 2];
    let mut start = 0;
    while start < frames {
        let end = (start + BLOCK).min(frames);
        let ctx = BlockContext {
            sample_rate: RATE,
            frames: end - start,
            playing: true,
            position: start as u64,
            loop_range: None,
            count_in: None,
            tempo: &tempo,
        };
        let (left, right) = out.split_at_mut(1);
        let mut lanes: [&mut [f32]; 2] = [&mut left[0][start..end], &mut right[0][start..end]];
        sequencer.process(&ctx, &mut Buffer::new(&mut lanes));
        start = end;
    }
    Ok((out, missing))
}

/// Reads a WAV as channels and its rate.
pub fn read_wav(path: &Path) -> Result<(f64, Vec<Vec<f32>>)> {
    let (info, channels) = cantina_engine::wav::read_all(path)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok((info.sample_rate as f64, channels))
}

/// Writes channels as a 32-bit float WAV.
pub fn write_wav(path: &Path, sample_rate: f64, channels: &[Vec<f32>]) -> Result<()> {
    cantina_engine::wav::write(path, sample_rate, channels)
}
