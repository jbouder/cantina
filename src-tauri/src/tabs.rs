//! Tabs from recordings: the glue between the transcriber
//! (`cantina_engine::transcribe`), the fretboard (`cantina_dsp::fretboard`)
//! and the project's [`Tab`].
//!
//! A transcription keeps what it heard in `Tab::heard`, in seconds from
//! the tab's tick 0, and the notes are built from that: cleaned for the
//! tuning, quantised to a grid at the tab's tempo, fitted to strings and
//! frets, with the techniques the timing shows. Re-quantising runs the
//! same build again on the same heard notes, so it never transcribes
//! twice.

use std::path::Path;

use anyhow::{Context, Result};
use cantina_dsp::fretboard::{self, FitOptions, Fretboard};
use cantina_engine::{transcribe, wav, TabEvent, TabScore, TempoMap};
use cantina_project::{Bend, Clip, HeardNote, Instrument, Tab, TabNote, Tempo, Track, PPQ};
use serde::{Deserialize, Serialize};

/// The highest fret a fitted note may use.
const FRETS: u8 = 24;

/// The grid notes snap to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grid {
    /// Eighth notes.
    #[serde(rename = "1/8")]
    Eighth,
    /// Sixteenth notes.
    #[serde(rename = "1/16")]
    Sixteenth,
    /// Eighth-note triplets.
    #[serde(rename = "1/8t")]
    EighthTriplet,
    /// Sixteenth-note triplets.
    #[serde(rename = "1/16t")]
    SixteenthTriplet,
}

impl Grid {
    /// Ticks per step, at 960 per quarter note.
    pub fn ticks(self) -> u64 {
        let quarter = u64::from(PPQ);
        match self {
            Grid::Eighth => quarter / 2,
            Grid::Sixteenth => quarter / 4,
            Grid::EighthTriplet => quarter / 3,
            Grid::SixteenthTriplet => quarter / 6,
        }
    }
}

/// What the transcribe sheet asks for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabSettings {
    /// Guitar or bass.
    pub instrument: Instrument,
    /// The open strings, lowest first, as MIDI notes.
    pub tuning: Vec<u8>,
    /// The capo's fret; 0 for none.
    #[serde(default)]
    pub capo: u8,
    /// The grid to quantise to.
    pub grid: Grid,
}

/// The clip's audio, mixed to mono and resampled for the model. Reads
/// only the frames the clip plays, through the streaming reader; the
/// file is opened for reading and never written.
pub fn clip_audio(path: &Path, clip: &Clip) -> Result<Vec<f32>> {
    let mut reader =
        wav::Reader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let info = reader.info().clone();
    let channels = usize::from(info.channels).max(1);
    let mut mono = Vec::with_capacity(clip.length as usize);
    let mut chunk = Vec::new();
    let mut at = clip.offset;
    let end = clip.offset + clip.length;
    while at < end {
        chunk.clear();
        let want = ((end - at) as usize).min(65_536);
        let got = reader.read(at, want, &mut chunk)?;
        if got == 0 {
            break;
        }
        mono.extend(
            chunk
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        at += got as u64;
    }
    Ok(cantina_dsp::resample(
        &mono,
        f64::from(info.sample_rate),
        transcribe::RATE,
    ))
}

/// The frame of the clip's file where the tab's tick 0 goes: the bar
/// line at or before the clip's start, so the tab's bars are the
/// project's. When that bar line is before the file begins, the beat
/// line, and failing that the clip's first frame.
pub fn origin(clip: &Clip, tempo: &Tempo, sample_rate: f64) -> u64 {
    let beat = 60.0 / tempo.bpm * 4.0 / f64::from(tempo.denominator.max(1)) * sample_rate;
    let bar = beat * f64::from(tempo.numerator.max(1));
    // The timeline sample where the file's frame 0 would be.
    let file_start = clip.start as f64 - clip.offset as f64;
    for step in [bar, beat] {
        let line = (clip.start as f64 / step).floor() * step;
        if line >= file_start {
            return (line - file_start).round() as u64;
        }
    }
    clip.offset
}

/// A transcribed tab: the heard notes, placed `lead` seconds after tick
/// 0 (the clip's audio starts that long after the origin), and the notes
/// built from them.
pub fn transcribed(
    name: &str,
    settings: &TabSettings,
    tempo: &Tempo,
    origin: u64,
    lead: f64,
    heard: &[transcribe::Heard],
) -> Result<Tab> {
    let mut tab = Tab {
        name: name.to_string(),
        instrument: settings.instrument,
        tuning: settings.tuning.clone(),
        capo: settings.capo,
        tempo: tempo.clone(),
        origin,
        notes: Vec::new(),
        heard: heard
            .iter()
            .filter(|h| h.end > h.start && h.start + lead >= 0.0)
            .map(|h| HeardNote {
                pitch: h.pitch,
                onset: h.start + lead,
                length: h.end - h.start,
                velocity: h.amplitude.clamp(0.0, 1.0),
                bend: h
                    .bend
                    .clamp(-cantina_project::MAX_BEND, cantina_project::MAX_BEND),
            })
            .collect(),
    };
    tab.validate().map_err(anyhow::Error::msg)?;
    tab.notes = notes(&tab, settings.grid);
    Ok(tab)
}

/// Builds a tab's notes from what it heard: clean for the tuning,
/// quantise to `grid`, fit to strings and frets, and read techniques.
pub fn notes(tab: &Tab, grid: Grid) -> Vec<TabNote> {
    let board = Fretboard::new(tab.tuning.clone(), tab.capo, FRETS);
    let mut heard: Vec<fretboard::Note> = tab
        .heard
        .iter()
        .map(|h| fretboard::Note {
            pitch: h.pitch,
            start: h.onset,
            end: h.onset + h.length,
            velocity: h.velocity,
            bend: h.bend,
        })
        .collect();
    fretboard::clean(&board, &mut heard);

    let tick = tab.seconds(1);
    let step = grid.ticks();
    let snap = |seconds: f64| ((seconds / tick / step as f64).round().max(0.0) as u64) * step;
    let quantised: Vec<fretboard::Note> = heard
        .iter()
        .map(|n| {
            let start = snap(n.start);
            let end = snap(n.end).max(start + step);
            fretboard::Note {
                start: start as f64 * tick,
                end: end as f64 * tick,
                ..*n
            }
        })
        .collect();
    let places = fretboard::fit(
        &board,
        &quantised,
        FitOptions {
            // Quantised, a chord's notes start together exactly.
            chord_window: tick / 2.0,
            ..FitOptions::default()
        },
    );
    let techniques = fretboard::techniques(&board, &heard, &places);

    let mut out: Vec<TabNote> = Vec::new();
    for ((q, place), t) in quantised.iter().zip(&places).zip(&techniques) {
        let Some(place) = place else { continue };
        let start = (q.start / tick).round() as u64;
        let end = (q.end / tick).round() as u64;
        out.push(TabNote {
            velocity: q.velocity.clamp(0.0, 1.0),
            bend: t.bend.map(|semitones| Bend {
                semitones,
                release: false,
            }),
            hammer_on: t.hammer_on,
            pull_off: t.pull_off,
            palm_mute: t.palm_mute,
            ..TabNote::new(place.string, place.fret, start, end - start)
        });
    }
    // A string plays one note at a time: a note ends where the next one
    // on its string starts.
    out.sort_by_key(|n| (n.string, n.start));
    for i in 1..out.len() {
        if out[i].string == out[i - 1].string && out[i - 1].end() > out[i].start {
            out[i - 1].length = (out[i].start - out[i - 1].start).max(1);
        }
    }
    out.retain(|n| n.length > 0);
    out.sort_by_key(|n| (n.start, std::cmp::Reverse(n.string)));
    out
}

/// An edit to a tab's notes: the indices to take away and the notes to
/// add, as `Command::EditTabNotes` carries them.
pub type NotesEdit = (Vec<u32>, Vec<TabNote>);

/// Re-quantises the notes at `selection`, or every note when it is
/// empty, from what the tab heard. A selected note stands for the heard
/// notes of its pitch (or an octave off, which cleaning may have fixed)
/// that started within a beat of it; those alone are built again, so
/// the notes around the selection stay as they are.
pub fn requantise(tab: &Tab, grid: Grid, selection: &[u32]) -> NotesEdit {
    let chosen: Vec<u32> = selection
        .iter()
        .copied()
        .filter(|&i| (i as usize) < tab.notes.len())
        .collect();
    if chosen.is_empty() {
        return ((0..tab.notes.len() as u32).collect(), notes(tab, grid));
    }
    let beat = tab.seconds(u64::from(PPQ));
    let heard = tab
        .heard
        .iter()
        .filter(|h| {
            chosen.iter().any(|&i| {
                let n = &tab.notes[i as usize];
                let near = (h.onset - tab.seconds(n.start)).abs() <= beat;
                let pitch = tab.pitch(n).map(i16::from);
                let same = pitch.is_some_and(|p| matches!((i16::from(h.pitch) - p).abs(), 0 | 12));
                near && same
            })
        })
        .cloned()
        .collect();
    let part = Tab {
        heard,
        notes: Vec::new(),
        ..tab.clone()
    };
    (chosen, notes(&part, grid))
}

/// Places the notes at `selection` (every note when it is empty) on the
/// fretboard again, as one hand position, and keeps their timing and
/// techniques. Dead notes and harmonics stay where they are, and notes
/// that land where they were are left alone.
pub fn refit(tab: &Tab, selection: &[u32]) -> NotesEdit {
    let board = Fretboard::new(tab.tuning.clone(), tab.capo, FRETS);
    let fitted: Vec<usize> = (0..tab.notes.len())
        .filter(|&i| !tab.notes[i].dead && !tab.notes[i].harmonic)
        .collect();
    let notes: Vec<fretboard::Note> = fitted
        .iter()
        .map(|&i| {
            let n = &tab.notes[i];
            fretboard::Note {
                pitch: tab.pitch(n).unwrap_or(0),
                start: tab.seconds(n.start),
                end: tab.seconds(n.end()),
                velocity: n.velocity,
                bend: n.bend.map_or(0.0, |b| b.semitones),
            }
        })
        .collect();
    let places: Vec<Option<fretboard::Place>> = fitted
        .iter()
        .map(|&i| {
            Some(fretboard::Place {
                string: tab.notes[i].string,
                fret: tab.notes[i].fret,
            })
        })
        .collect();
    let chosen: Vec<usize> = (0..fitted.len())
        .filter(|&k| selection.is_empty() || selection.contains(&(fitted[k] as u32)))
        .collect();
    let options = FitOptions {
        chord_window: tab.seconds(1) / 2.0,
        ..FitOptions::default()
    };
    let new = fretboard::refit(&board, &notes, &places, &chosen, options);
    let mut edit: NotesEdit = (Vec::new(), Vec::new());
    for &k in &chosen {
        let (Some(was), Some(now)) = (places[k], new[k]) else {
            continue;
        };
        if was != now {
            let i = fitted[k];
            edit.0.push(i as u32);
            edit.1.push(TabNote {
                string: now.string,
                fret: now.fret,
                ..tab.notes[i]
            });
        }
    }
    edit
}

/// The pitch a natural harmonic over `fret` sounds above the open
/// string, in semitones: the 12th an octave, the 7th and 19th an octave
/// and a fifth, the 5th and 24th two octaves, and so on.
fn harmonic_interval(fret: u8) -> u8 {
    match fret {
        12 => 12,
        7 | 19 => 19,
        5 | 24 => 24,
        4 | 9 | 16 => 28,
        3 => 31,
        _ => fret,
    }
}

/// Every note the tabs of a track's clips play, on the timeline in ticks
/// of `tempo` (the project's tempo map, never a practice speed), for the
/// tab voice. A note the clip has trimmed away is not played, and one
/// that runs past the clip's end stops there.
pub fn timeline_notes(track: &Track, tempo: &TempoMap, sample_rate: f64) -> TabScore {
    let mut score = TabScore::new();
    for clip in &track.clips {
        let Some(tab) = &clip.tab else { continue };
        let instrument = match tab.instrument {
            Instrument::Guitar => cantina_dsp::pluck::Instrument::Guitar,
            Instrument::Bass => cantina_dsp::pluck::Instrument::Bass,
        };
        let clip_end = clip.offset + clip.length;
        for note in &tab.notes {
            let from = tab.file_frame(note.start, sample_rate);
            if !clip.plays_frame(from) {
                continue;
            }
            let to = tab.file_frame(note.end(), sample_rate).min(clip_end);
            let Some(open) = tab.open_string(note.string) else {
                continue;
            };
            let above = if note.harmonic {
                harmonic_interval(note.fret)
            } else {
                note.fret
            };
            let pitch = f32::from(open) + f32::from(tab.capo) + f32::from(above);
            let at = |frame: u64| {
                tempo.samples_to_ticks(clip.start + (frame - clip.offset), sample_rate)
            };
            let tick = at(from);
            score.push(TabEvent {
                tick,
                ticks: (at(to) - tick).max(1.0),
                instrument,
                note: cantina_dsp::PluckNote {
                    string: note.string,
                    pitch,
                    velocity: note.velocity,
                    length: 0,
                    bend: note.bend.map_or(0.0, |b| b.semitones),
                    release: note.bend.is_some_and(|b| b.release),
                    vibrato: note.vibrato,
                    palm_mute: note.palm_mute,
                    dead: note.dead,
                    harmonic: note.harmonic,
                },
            });
        }
    }
    score.sort_by(|a, b| a.tick.total_cmp(&b.tick));
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo(bpm: f64) -> Tempo {
        Tempo {
            bpm,
            numerator: 4,
            denominator: 4,
        }
    }

    fn clip(start: u64, offset: u64) -> Clip {
        Clip {
            id: cantina_project::ClipId(1),
            name: "Take".into(),
            file: "take.wav".into(),
            start,
            offset,
            length: 480_000,
            tab: None,
        }
    }

    fn settings(grid: Grid) -> TabSettings {
        TabSettings {
            instrument: Instrument::Guitar,
            tuning: vec![40, 45, 50, 55, 59, 64],
            capo: 0,
            grid,
        }
    }

    fn heard(pitch: u8, start: f64, end: f64) -> transcribe::Heard {
        transcribe::Heard {
            start,
            end,
            pitch,
            amplitude: 0.8,
            bend: 0.0,
        }
    }

    #[test]
    fn grids_count_ticks() {
        assert_eq!(Grid::Eighth.ticks(), 480);
        assert_eq!(Grid::Sixteenth.ticks(), 240);
        assert_eq!(Grid::EighthTriplet.ticks(), 320);
        assert_eq!(Grid::SixteenthTriplet.ticks(), 160);
        let g: Grid = serde_json::from_str("\"1/16t\"").unwrap();
        assert_eq!(g, Grid::SixteenthTriplet);
    }

    #[test]
    fn tick_zero_is_on_the_bar_before_the_clip() {
        // 120 bpm 4/4 at 48 kHz: a bar is 96 000 samples.
        let t = tempo(120.0);
        // A clip on bar 2 from the start of its file: tick 0 is its start.
        assert_eq!(origin(&clip(96_000, 0), &t, 48_000.0), 0);
        // Trimmed half a bar in: the bar line is 48 000 frames back.
        assert_eq!(origin(&clip(144_000, 48_000), &t, 48_000.0), 0);
        // Recorded from a beat mid-bar: the bar line is before the file,
        // so the beat line, the file's first frame, is used.
        assert_eq!(origin(&clip(120_000, 0), &t, 48_000.0), 0);
        // Trimmed so the bar line is before the file but a beat is not.
        assert_eq!(origin(&clip(150_000, 40_000), &t, 48_000.0), 34_000);
        // Recorded from between beats: no line falls in the file before
        // the clip, so tick 0 is the clip's first frame.
        assert_eq!(origin(&clip(130_000, 0), &t, 48_000.0), 0);
    }

    #[test]
    fn heard_notes_become_a_quantised_fitted_tab() {
        // 120 bpm: an eighth is a quarter second. An E5 a little late,
        // then a B a little early, then a long A.
        let notes = [
            heard(40, 0.012, 0.24),
            heard(47, 0.012, 0.24),
            heard(52, 0.020, 0.24),
            heard(59, 0.245, 0.50),
            heard(57, 0.51, 1.40),
        ];
        let tab = transcribed(
            "Take",
            &settings(Grid::Eighth),
            &tempo(120.0),
            0,
            0.0,
            &notes,
        )
        .unwrap();
        assert_eq!(tab.heard.len(), 5);
        let got: Vec<(u8, u8, u64, u64)> = tab
            .notes
            .iter()
            .map(|n| (n.string, n.fret, n.start, n.length))
            .collect();
        assert_eq!(
            got,
            [
                (6, 0, 0, 480),
                (5, 2, 0, 480),
                (4, 2, 0, 480),
                (2, 0, 480, 480),
                (3, 2, 960, 1920),
            ]
        );
        tab.validate().unwrap();
    }

    #[test]
    fn requantising_uses_what_was_heard() {
        let notes = [heard(57, 0.13, 0.36), heard(59, 0.38, 0.6)];
        let tab = transcribed(
            "Take",
            &settings(Grid::Eighth),
            &tempo(120.0),
            0,
            0.0,
            &notes,
        )
        .unwrap();
        // On eighths the first note snaps to 1 eighth (0.125 s rounds up).
        assert_eq!(tab.notes[0].start, 480);
        // On sixteenths it is the second sixteenth.
        let fine = super::notes(&tab, Grid::Sixteenth);
        assert_eq!(fine[0].start, 240);
        assert_eq!(fine[1].start, 720);
    }

    #[test]
    fn a_tracks_tabs_are_placed_on_the_timeline_for_the_voice() {
        let map = TempoMap::new(120.0, cantina_engine::TimeSignature::COMMON);
        let mut track = Track::new(cantina_project::TrackId(1), "Guitar");
        let mut c = clip(48_000, 24_000);
        let mut tab = Tab::new("Riff", Instrument::Guitar, 6).unwrap();
        tab.tempo = tempo(120.0);
        // Tick 0 is the clip's first frame; at 120 bpm 960 ticks is half
        // a second, 24 000 frames.
        tab.origin = 24_000;
        tab.capo = 2;
        tab.notes = vec![
            TabNote::new(6, 3, 0, 960),
            TabNote {
                harmonic: true,
                ..TabNote::new(1, 7, 960, 960)
            },
            // Past the clip's end: not played.
            TabNote::new(1, 0, 96_000, 960),
        ];
        c.length = 96_000;
        c.tab = Some(tab);
        track.clips.push(c);
        let score = timeline_notes(&track, &map, 48_000.0);
        assert_eq!(score.len(), 2);
        // The clip starts at 48 000: one second, two beats, 1920 ticks.
        assert!((score[0].tick - 1920.0).abs() < 1e-6);
        assert!((score[0].ticks - 960.0).abs() < 1e-6);
        // G on the low E with a capo at 2: 40 + 2 + 3.
        assert_eq!(score[0].note.pitch, 45.0);
        // A harmonic over the 7th of the high E sounds an octave and a fifth up.
        assert_eq!(score[1].note.pitch, 64.0 + 2.0 + 19.0);
        assert!(score[1].note.harmonic);
    }

    #[test]
    fn a_selection_requantises_only_its_span() {
        // Four notes a beat apart; on eighths each snaps to a beat line.
        let notes = [
            heard(57, 0.02, 0.4),
            heard(59, 0.52, 0.9),
            heard(60, 1.13, 1.4),
            heard(62, 1.52, 1.9),
        ];
        let mut tab = transcribed(
            "Take",
            &settings(Grid::Eighth),
            &tempo(120.0),
            0,
            0.0,
            &notes,
        )
        .unwrap();
        assert_eq!(tab.notes.len(), 4);
        // Move the first note by hand: a whole-tab re-quantise undoes it,
        // a re-quantise of the third note alone keeps it.
        tab.notes[0].start = 120;
        let (remove, add) = requantise(&tab, Grid::Sixteenth, &[2]);
        assert_eq!(remove, vec![2]);
        assert_eq!(tab.pitch(&add[0]), Some(60));
        assert_eq!(add.len(), 1);
        // 1.13 s is 2.26 beats: the 9th sixteenth on sixteenths.
        assert_eq!(add[0].start, 2160);
        let (remove, add) = requantise(&tab, Grid::Sixteenth, &[]);
        assert_eq!(remove.len(), 4);
        assert_eq!(add[0].start, 0);
    }

    #[test]
    fn refitting_moves_the_selection_and_keeps_its_timing() {
        let mut tab = Tab::new("Riff", Instrument::Guitar, 6).unwrap();
        // E4 at the 24th fret of the low E, and an A at the 5th fret of
        // the low E that is not selected.
        let mut high = TabNote::new(6, 24, 960, 480);
        high.vibrato = true;
        tab.notes = vec![TabNote::new(6, 5, 0, 480), high];
        let (remove, add) = refit(&tab, &[1]);
        assert_eq!(remove, vec![1]);
        let moved = add[0];
        assert_ne!((moved.string, moved.fret), (6, 24));
        assert_eq!(tab.pitch(&moved), Some(64), "the same pitch");
        assert_eq!((moved.start, moved.length, moved.vibrato), (960, 480, true));
        // Already where it fits best: nothing to change.
        tab.notes[1] = moved;
        assert_eq!(refit(&tab, &[1]), (vec![], vec![]));
    }

    #[test]
    fn the_lead_moves_heard_notes_after_tick_zero() {
        let tab = transcribed(
            "Take",
            &settings(Grid::Sixteenth),
            &tempo(120.0),
            0,
            0.5,
            &[heard(57, 0.0, 0.25)],
        )
        .unwrap();
        assert_eq!(tab.heard[0].onset, 0.5);
        assert_eq!(tab.notes[0].start, 960);
    }

    #[test]
    fn a_driven_root_an_octave_low_is_put_back() {
        // The spike's commonest error: E5 heard with E1 for its root.
        let notes = [
            heard(28, 0.0, 0.5),
            heard(47, 0.0, 0.5),
            heard(52, 0.0, 0.5),
        ];
        let tab = transcribed(
            "Take",
            &settings(Grid::Eighth),
            &tempo(120.0),
            0,
            0.0,
            &notes,
        )
        .unwrap();
        let pitches: Vec<u8> = tab.notes.iter().filter_map(|n| tab.pitch(n)).collect();
        assert_eq!(pitches, [40, 47, 52]);
    }

    /// Precision, recall and F1 of `found` against `truth`, each a pitch
    /// and an onset in seconds: a match is the same pitch within 50 ms,
    /// each truth note matched once.
    fn f1(truth: &[(u8, f64)], found: &[(u8, f64)]) -> (f64, f64, f64) {
        let mut used = vec![false; truth.len()];
        let mut hits = 0;
        for &(p, t) in found {
            if let Some(i) = (0..truth.len())
                .filter(|&i| !used[i] && truth[i].0 == p && (truth[i].1 - t).abs() <= 0.05)
                .min_by(|&a, &b| (truth[a].1 - t).abs().total_cmp(&(truth[b].1 - t).abs()))
            {
                used[i] = true;
                hits += 1;
            }
        }
        let p = hits as f64 / found.len().max(1) as f64;
        let r = hits as f64 / truth.len().max(1) as f64;
        (
            p,
            r,
            if p + r > 0.0 {
                2.0 * p * r / (p + r)
            } else {
                0.0
            },
        )
    }

    /// Transcribes each rendered reference take (`cargo run --release -p
    /// cantina-dsp --example make_takes` first) through the whole pipeline
    /// and prints F1 at each stage: what the model heard, after cleaning
    /// for the tuning, and the finished tab's notes. Not run by default:
    ///
    ///     cargo test --release -p cantina reference_takes -- --ignored --nocapture
    #[test]
    #[ignore]
    fn reference_takes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let rendered = root.join("target/takes");
        let model = transcribe::Transcriber::new().unwrap();
        let mut sums = [0.0; 3];
        let mut count = 0.0;
        println!("| Take | Heard | Cleaned | Tab |");
        println!("| --- | ---: | ---: | ---: |");
        let mut ids: Vec<String> = std::fs::read_dir(root.join("takes"))
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().to_string_lossy().to_string();
                name.strip_suffix(".take.json").map(str::to_string)
            })
            .collect();
        ids.sort();
        for id in ids {
            let take: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(format!("takes/{id}.take.json"))).unwrap(),
            )
            .unwrap();
            let truth: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(rendered.join(format!("{id}.truth.json")))
                    .expect("render the takes first"),
            )
            .unwrap();
            let truth: Vec<(u8, f64)> = truth["notes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| {
                    (
                        n["pitch"].as_f64().unwrap() as u8,
                        n["onset"].as_f64().unwrap(),
                    )
                })
                .collect();
            let bass = take["instrument"] == "bass";
            let path = rendered.join(format!("{id}.wav"));
            let info = wav::info(&path).unwrap();
            let clip = Clip {
                length: info.frames,
                ..clip(0, 0)
            };
            let audio = clip_audio(&path, &clip).unwrap();
            let maps = model.run(&audio, |_| true).unwrap().unwrap();
            let heard = transcribe::notes(&maps, transcribe::Options::default());
            let settings = TabSettings {
                instrument: if bass {
                    Instrument::Bass
                } else {
                    Instrument::Guitar
                },
                tuning: if bass {
                    vec![28, 33, 38, 43]
                } else {
                    vec![40, 45, 50, 55, 59, 64]
                },
                capo: 0,
                grid: Grid::Sixteenth,
            };
            let t = tempo(take["tempo"].as_f64().unwrap());
            // The takes' first beat is a quarter second in, as tick 0 is
            // a bar line in the app.
            const LEAD: f64 = 0.25;
            let tab = transcribed(&id, &settings, &t, 0, -LEAD, &heard).unwrap();

            let raw: Vec<(u8, f64)> = heard.iter().map(|h| (h.pitch, h.start)).collect();
            let board = Fretboard::new(settings.tuning.clone(), 0, FRETS);
            let mut cleaned: Vec<fretboard::Note> = tab
                .heard
                .iter()
                .map(|h| fretboard::Note {
                    pitch: h.pitch,
                    start: h.onset,
                    end: h.onset + h.length,
                    velocity: h.velocity,
                    bend: h.bend,
                })
                .collect();
            fretboard::clean(&board, &mut cleaned);
            let cleaned: Vec<(u8, f64)> =
                cleaned.iter().map(|n| (n.pitch, n.start + LEAD)).collect();
            let notes: Vec<(u8, f64)> = tab
                .notes
                .iter()
                .filter_map(|n| Some((tab.pitch(n)?, tab.seconds(n.start) + LEAD)))
                .collect();
            let scores = [
                f1(&truth, &raw).2,
                f1(&truth, &cleaned).2,
                f1(&truth, &notes).2,
            ];
            println!(
                "| `{id}` | {:.2} | {:.2} | {:.2} |",
                scores[0], scores[1], scores[2]
            );
            for (s, v) in sums.iter_mut().zip(scores) {
                *s += v;
            }
            count += 1.0;
        }
        println!(
            "| mean | {:.2} | {:.2} | {:.2} |",
            sums[0] / count,
            sums[1] / count,
            sums[2] / count
        );
    }

    #[test]
    fn notes_on_one_string_do_not_overlap() {
        let notes = [heard(57, 0.0, 0.9), heard(57, 0.5, 1.0)];
        let tab = transcribed(
            "Take",
            &settings(Grid::Eighth),
            &tempo(120.0),
            0,
            0.0,
            &notes,
        )
        .unwrap();
        assert!(tab.overlaps().is_empty(), "{:?}", tab.notes);
    }
}
