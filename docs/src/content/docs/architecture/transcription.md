---
title: "Transcription"
description: "How Cantina turns a recorded guitar or bass part into notes for a tab: Basic Pitch on the device through tract, the cleanup after it, how accurate it is on the reference takes and where it goes wrong."
sidebar:
  order: 9.8
---

Transcription turns a recorded clip into notes: pitch, onset, offset and
velocity. The tab work in M14 builds on it. Choosing a string and fret
for each note, editing, playback and export all start from these notes.
This page records the spike that chose the approach (issue #170), the
numbers behind the choice, and what transcription is known to get wrong.

## The approach

Cantina runs [Basic Pitch](https://github.com/spotify/basic-pitch),
Spotify's note transcription model, on the device. It runs in process
with [tract](https://github.com/sonos/tract), a pure Rust inference
library, off the audio thread.

- **The model** is 228 KB of ONNX, Apache-2.0 licensed, compiled into
  the app from `crates/engine/models/` (credited in its `CREDITS.md`). It takes raw mono audio at 22 050 Hz in windows of
  43 844 samples (1.99 s) and computes its own constant-Q transform
  inside the graph. For each frame of 11.6 ms it outputs three maps: note
  activity and onset likelihood over 88 piano keys, and a pitch contour
  at a third of a semitone, which is where bends and vibrato show.
- **tract** loads the model in about 60 ms and runs it at about 57×
  real time on an M1 Pro, so a three-minute take transcribes in about
  three seconds. Its outputs match ONNX Runtime's to within 1.5 × 10⁻⁶,
  so the notes are the same ones the reference implementation finds. It
  is MIT/Apache licensed and pure Rust, so there is no native library to
  bundle, sign or notarize.
- **Cantina's own cleanup** follows the model, because each error
  pattern below has a cause a tab can correct:
  1. Basic Pitch's note creation (thresholding onsets and frames into
     notes), ported from Python to Rust in `cantina_engine::transcribe`.
     On all six reference takes it gives the same notes as Basic Pitch's
     own Python, note for note, with identical onsets
     (`scripts/transcribe-spike/parity.py`).
  2. A range limit from the tuning: no note below the lowest open string
     or above the highest fret. Notes below the range move up an octave,
     and a note moved onto one already sounding is dropped.
  3. Harmonic suppression: a note an octave, a twelfth or two octaves
     above a louder note it sounds with is dropped, unless another note
     at a different interval (a fifth, a third) makes the group a chord.
     So a power chord keeps its octave.
  4. Bend merging: a note a semitone or two above the one before it,
     arriving as that note's contour rises to it, becomes a bend on the
     first note rather than a second note.

  Steps 2 to 4 are `cantina_dsp::fretboard::clean`, next to the fitting
  that chooses strings and frets (see [fitting](#fitting-notes-to-the-fretboard)
  below).

Transcription never touches the source audio file or the audio thread.
`EngineState::clip_transcribe` runs on a blocking worker: it reads only
the frames the clip plays through the streaming WAV reader, mixes to mono,
resamples to 22 050 Hz and runs the model window by window, reporting
progress on the `render-progress` event and stopping between windows
when asked. A stopped transcription changes nothing. A finished one is one
`SetClipTab` command, so it is undone like any edit.

Later edits to the notes, from the tab editor, re-fitting or
re-quantizing, are `EditTabNotes` commands: the indices of the notes
taken away and the notes added. The notes are kept in order of start,
the lowest-pitched string first, so the inverse names the added notes
where they landed. Only notes travel, never what was heard, so an edit
stays small in the journal. `tabs::refit` runs `fretboard::refit` over
the selection; `tabs::requantize` rebuilds only the heard notes of the
selected pitches that started within a beat of a selected note.

## From notes to a tab

`src-tauri/src/tabs.rs` turns what the model heard into the clip's tab:

1. **Where tick 0 is.** The tab is anchored to the clip's file at the
   bar line at or before the clip's start, so its bars are the
   project's. If that bar line is before the file begins, the beat line
   is used, and failing that the clip's first frame.
2. **Keep what was heard.** Every note goes into the tab's `heard` list,
   in seconds from tick 0.
3. **Clean, quantize, fit.** The heard notes are cleaned for the tuning,
   snapped to the chosen grid (eighths, sixteenths or their triplets) at
   the project's tempo, given strings and frets, and given the
   techniques the timing shows. Where two notes on one string overlap
   after snapping, the first ends where the second starts.

Re-quantizing runs step 3 again on the same `heard` notes, so changing
the grid is instant and never transcribes twice.

On the reference takes, through the whole pipeline at sixteenths (an
ignored test, `cargo test --release -p cantina reference_takes --
--ignored --nocapture`), F1 on pitch and onset within 50 ms:

| Take | Heard | Cleaned | Tab |
| --- | ---: | ---: | ---: |
| `bass-line` | 0.68 | 0.84 | 0.84 |
| `bends` | 0.74 | 0.77 | 0.77 |
| `open-chords` | 0.78 | 0.78 | 0.73 |
| `palm-muted` | 0.58 | 0.65 | 0.65 |
| `power-chords` | 0.51 | 0.60 | 0.60 |
| `single-notes` | 0.82 | 0.88 | 0.88 |
| mean | 0.68 | 0.75 | 0.75 |

Cleaning is worth seven points. Quantizing costs nothing except on
strummed chords, where the later strings of a strum snap onto the beat
more than 50 ms before they sound. That is the tab a player wants, but it
counts as a miss here.

## Fitting notes to the fretboard

The same E can be played in five places, so after transcription every
note still needs a string and a fret. `cantina_dsp::fretboard::fit`
chooses them as a shortest path over the notes, grouped into chords by
start time (within 35 ms, the spread of a strum). Each group's candidates
are every way to put its notes on different strings. A candidate's cost
adds up what a hand feels:

- **Stretch**: fretted notes more than three frets apart cost more, and
  more than five apart are not considered.
- **Strings together**: a silent string between two that sound costs a
  lot, so a G5 is 355, not 3x00.
- **Open strings** are cheaper in the first five frets, where players use
  them, and dearer above.
- **Height**: each fret up costs a little, so the lower of two equal
  shapes wins.
- **Movement**: shifting the hand from one group to the next costs per
  fret, and more past four.
- **Ringing strings**: playing on a string whose note is still sounding
  costs extra.
- **Repeats**: the same pitch again is played in the same place.
- **Region**: given a region such as frets 7 to 10, notes outside it cost
  far more than any other choice, so they move into it wherever they
  can. `refit` applies this to a selection and leaves the rest alone.

A note the fretboard cannot reach, or one too many for the strings, gets
no place. `techniques` then reads what timing and loudness show:

- a **hammer-on** or **pull-off** is a note on the same string straight
  after another, a fret to five frets away and noticeably softer;
- a **palm mute** is a short note on one of the three lowest strings,
  under 150 ms and under 60% of the gap to the next note there;
- a **bend** is a rise of half a semitone or more, to the nearest half.

Slides, vibrato and harmonics are not detected; the player adds them in
the editor. Every hand-written reference take fits with every note
placed, chords within a stretch and on adjacent strings, and nothing
above the 7th fret. That is a unit test, `every_reference_take_fits_playably`.

## The reference takes

Six takes in `takes/` cover the cases the issue named. Each
`*.take.json` is hand-written ground truth, the notes a player would
read, in beats at a tempo, with strums, palm mutes, bends and vibrato
marked:

| Take | What | Tone |
| --- | --- | --- |
| `single-notes` | an E minor pentatonic line, quarters and eighths | clean |
| `power-chords` | root, fifth and octave in an eighth-note rhythm | driven |
| `open-chords` | G, C, D and Em strummed and left ringing | clean |
| `palm-muted` | a muted low-E chug with open power-chord accents | driven |
| `bends` | whole- and half-step bends, a bend and release, vibrato | light drive |
| `bass-line` | roots, octaves and passing notes on a four-string | clean |

`cargo run --release -p cantina-dsp --example make_takes` renders each
one to `target/takes/<id>.wav` with Karplus-Strong strings and writes
`<id>.truth.json`, the notes as they sound in the file. Onsets there
include the strum spread and a few milliseconds of timing jitter.
`scripts/transcribe-spike/evaluate.py` scores an approach against them
with [mir_eval](https://craffel.github.io/mir_eval/): a note counts when
its onset is within 50 ms and its pitch within 50 cents. "With offsets"
also needs the end within 20% of the note's length.

The strings are synthesized, and synthesized strings are cleaner than a
real guitar: no fret buzz, no string noise, no room, and the same pluck
every time. Treat these numbers as an upper bound. The takes are there so
that a later change can be compared with this one, not to promise
accuracy on a real recording.

## What was measured

Measured on an Apple M1 Pro. Speed is audio duration divided by
wall-clock time.

| Approach | Mean F1 | F1 with offsets | Onset error | Speed | Size | License |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| Basic Pitch, default settings | 0.69 | 0.50 | 10 ms | 130× ¹ | 228 KB | Apache-2.0 |
| Basic Pitch, range set per instrument | 0.70 | 0.46 | 10 ms | 130× ¹ | 228 KB | Apache-2.0 |
| pYIN pitch track and onset detection | 0.55 | 0.48 | 14 ms | 5× | none | ISC (librosa) |
| A hosted transcription API | not measured | | | | | paid service |
| The assistant's provider (M13) | not measured | | | | | |

¹ The whole Python pipeline, with ONNX Runtime running the model.
Running the model alone on 30 windows: Core ML 5.5 ms per window
(360×), ONNX Runtime 9.9 ms (200×) and tract 35 ms (57×).

By take, F1 on onset and pitch:

| Take | Basic Pitch | Basic Pitch, range set | pYIN |
| --- | ---: | ---: | ---: |
| `single-notes` | 0.83 | 0.77 | 1.00 |
| `power-chords` | 0.51 | 0.60 | 0.13 |
| `open-chords` | 0.78 | 0.80 | 0.19 |
| `palm-muted` | 0.58 | 0.72 | 0.52 |
| `bends` | 0.74 | 0.73 | 0.67 |
| `bass-line` | 0.68 | 0.58 | 0.81 |

"Range set" limits Basic Pitch to 75–1400 Hz for guitar and 30–400 Hz
for bass, and lowers its shortest note from 128 ms to 58 ms. The range
helps the driven takes. The shorter minimum adds ghost notes to the bass
line.

## Why this approach

- **Basic Pitch finds the notes.** With the range set, recall was
  0.92–1.00 on four of the six takes. Where it scores low, the cause is extra notes with a
  pattern, and each pattern has a fix (below). pYIN follows a single
  line well but cannot see a chord, which is most of what a guitarist
  plays.
- **On the device.** It is fast enough that transcription feels like an
  edit, not a job, and the player's audio never leaves the machine.
- **tract over ONNX Runtime or Core ML.** All three run the same model.
  At 57× real time tract is already fast enough. Its speed costs nothing
  to ship, while ONNX Runtime adds a native library of about 30 MB to
  sign, and Core ML needs bindings and a compiled model package. If
  speed ever matters more, the model is the same and only the runtime
  changes.
- **Not a hosted API.** None was measured: one needs an account and
  per-minute billing, and it would send the player's recording to a
  third party. That is a poor default for an app whose assistant is off
  until asked for.
- **Not the assistant's provider.** The assistant talks to an
  OpenAI-compatible chat endpoint. Those endpoints take text, not
  audio, and the chat models behind them are not trained to transcribe
  music. They are useful after transcription, to explain a tab or
  suggest a fingering, not to produce the notes.

## Where it goes wrong

The extra and missed notes on the reference takes fall into four
patterns:

- **Harmonics as notes.** On clean single notes and bass, all but one
  extra note was an octave or a twelfth above a real one: 8 on
  `single-notes`, 19 on `bass-line`. Harmonic suppression removes these.
- **Driven chords an octave low.** Distortion makes difference tones: a
  root and fifth (3:2) produce a tone an octave below the root. Basic
  Pitch hears that tone and often loses the real root. On
  `power-chords`, 17 of 39 notes were missed and 14 extra notes appeared
  below the chord's lowest note, most of them an octave under the root. Limiting the range to the
  tuning fixes most of these; it raised `palm-muted` from 0.58 to 0.72.
- **Bends as two notes.** A bend comes out as the starting note and a
  second note at the bent pitch: 4 of the 6 extra notes on `bends`.
  Bend merging, using the contour, turns these back into one bent note.
- **Offsets.** Ends are much less reliable than starts (0.50 against
  0.69), because a note that rings into the next one has no clear end.
  A tab cares about onsets far more than ends, and quantizing to the
  tempo map snaps durations anyway.

Things the takes do not cover, and that will be worse on a real guitar:
fret and string noise read as dead notes, heavy gain on single notes,
fast legato where the onset is quiet, and harmonics played on purpose.
Basic Pitch knows nothing about strings, so string and fret come from
[fitting](https://github.com/jbouder/cantina/issues/173), not from the
model.

## Reproducing the numbers

```sh
cargo run --release -p cantina-dsp --example make_takes
uv venv && uv pip install basic-pitch onnxruntime librosa "setuptools<81"
.venv/bin/python scripts/transcribe-spike/evaluate.py --tract
```

`--tract` builds `scripts/transcribe-spike/tract-probe`, a standalone
crate outside the workspace, runs the model through tract on the same
windows as ONNX Runtime, and reports the time per window and the largest
difference between the two.

## Exporting

`cantina_project::tab_export` turns a `Tab` into ASCII tab, MusicXML and
MIDI: pure functions, written to disk by the shell's `clip_tab_export`.
MusicXML is one voice on a TAB staff, written a quarter note at a time,
each quarter either on a thirty-second grid or as one complete triplet
group, so notation apps never see an incomplete tuplet. Hammer-ons and
pull-offs carry slurs as well, as MusicXML asks. An ignored test,
`cargo test -p cantina-project musescore -- --ignored`, opens the export
in MuseScore 4 (which refuses a file it has warnings about) and checks
what comes back. The PDF is drawn in the UI (`ui/src/lib/tabPdf.ts`) from
the same page layout the tab view uses, and written by `tab_pdf_write`.
