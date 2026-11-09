---
title: "Drums"
description: "How beats are played: the sampler, the drum track's graph, the pattern and pattern clip formats, and the sequencer."
sidebar:
  order: 7.4
---

Beats in Cantina are a **kit** (sounds) played by a **sampler** (the
`Processor` that makes the sound) under a **sequencer** (what decides
when), from **patterns** edited in a step grid and placed on the timeline as
**pattern clips**. This page is how they work; making beats is in the
[drums guide](/guides/drums/) and the kit files are in the
[kit format](/reference/kit-format/).

## The sampler

`cantina_dsp::Sampler` is a `Processor` built from a `LoadedKit` (the
kit with its samples in memory). It is polyphonic, 32 voices, with a
fixed pool so nothing is allocated on the audio thread; a 33rd hit steals
the oldest voice. Hits are queued with `trigger(pad, velocity, frame)`
and fired by `process` at exactly that frame of the block, which is what
makes a sequenced beat sample-accurate: the sequencer works out the frame
each step lands on from the tempo map and the sampler does the rest.

- **Velocity** is gain, linearly: a hit at 0.5 is half the amplitude of
  one at 1. Zero is not a hit.
- **Pan** is constant power. A mono sample sits at −3 dB a side when
  centred; a stereo sample plays at unity when centred and the pan
  becomes a balance.
- **Rate**: a sample recorded at 44.1 kHz plays at the right pitch in a
  48 kHz project; the voice steps through it at the ratio of the rates
  with linear interpolation. At equal rates playback is bit-exact.
- **Level**: the one parameter, in dB, smoothed like every other.
- A one-channel buffer gets the mono mix; a stereo one gets the pan.

`crates/dsp/tests/sampler.rs` renders a bar of a beat on a kit made of
arithmetic (decaying sines and seeded noise, so no binary fixtures are
needed) in 64-frame blocks and compares it to
`tests/golden/sampler_beat.wav`; a second test checks that every hit
starts on its frame, at the sample's first value times the velocity, the
pad's gain and the pan, with silence before the first. Regenerate the
reference after an intended change with
`UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test sampler`.

## Drum tracks

A track is an audio track or a drum track, decided when it is made
(**Add drums** in the timeline, which needs at least one installed kit).
A drum track records its kit's id in `project.json` (`"kit": "acoustic"`)
and has no input: in the graph the sequencer takes the place of the
input gate, player and recorder, and feeds the same delay, bus, pedals
and strip an audio track has, so a drum track can carry a pedal chain
and is latency-compensated like any other.

```text
sequencer -> delay -> bus -> pedals... -> strip -> master
```

Kits are found at launch under the user's `~/Music/Cantina/Kits/` (which
wins on a duplicate id), the kits that ship with the app, and any folder
`CANTINA_KITS` names. A kit is loaded the first time a track asks for it
and kept for the session. A track whose kit is not installed plays
silence and its grid says so; pick another kit from the panel's picker.
The picker's **Rescan** looks for kits again, for one installed while
the app runs.
Changing a track's kit is an edit (`SetTrackKit`) and rebuilds its
sequencer. The shell's `Kits` (`src-tauri/src/engine.rs`) holds all of
this.

## Patterns

A pattern is a grid: pads against steps, a velocity per cell. Patterns
belong to the project (`patterns` in `project.json`), not to a track,
and refer to pads by the ids the kit format uses, so a pattern written
for one kit plays on another with the same pad names.

```json
{
  "id": 7, "name": "Beat", "stepsPerBeat": 4, "length": 16, "swing": 0.2,
  "rows": [
    { "pad": "kick",  "steps": [1, 0, 0, 0, 0, 0, 0, 0, 0.9, 0, 0.6, 0, 0, 0, 0, 0] },
    { "pad": "snare", "steps": [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0.3] }
  ]
}
```

- **`stepsPerBeat`** is the resolution: 1, 2, 4 or 8 for quarters,
  eighths, sixteenths and thirty-seconds, 3 or 6 for triplets. A step is
  `960 / stepsPerBeat` ticks, the tempo map's units, so the pattern
  follows the tempo rather than a sample count.
- **`length`** is in steps, 1 to 128; the pattern loops after the last.
  Changing it cuts or pads every row.
- **`swing`** from 0 to 1 delays every second step by up to a third of a
  step: at 1 the off-beats fall where triplets would.
- **`rows`** hold only the pads that have hits, exactly `length`
  velocities each, 0 for no hit, sorted by pad. A row appears when its
  first cell is set and goes when its last is cleared, so a pattern's
  JSON is as small as what it plays.

Every change is a command (`crates/project/src/command.rs`):
`InsertPattern`, `RemovePattern`, `RenamePattern`, `SetPatternStep` (one
cell; a velocity drag coalesces), `SetPatternLength`,
`SetPatternResolution`, `SetPatternSwing` (coalesces) and
`ReplacePattern`, the inverse of a length change since cutting rows
loses velocities. All of them undo and replay through the journal like
any edit. The format went to version 3 when patterns and drum tracks
arrived; a version 2 file loads with no patterns and every track an
audio track.

## The sequencer

`cantina_engine::Sequencer` (`crates/engine/src/sequencer.rs`) is a graph
node that owns a `Sampler` and a **score**: the patterns placed on the
timeline as `Placement`s, each a pattern from a tick for a number of
ticks, looping inside that span from an offset into the pattern. Every
block it asks the tempo map which ticks the block covers, looks up which
hits land inside it and at which frame, and queues each on the sampler
for exactly that frame.

Hits come from a table each `PatternData` works out when it is built,
off the audio thread (`PatternData::built`): every hit of every row with
its swing (the row's own, or the pattern's), humanize, ratchets (2 to 4
evenly spaced hits, a little softer each), flams (a grace note 30 ticks
early at 45% velocity) and chance. When the pattern humanizes or plays
by chance the table covers eight passes, each different, then repeats;
otherwise one. Randomness comes from a hash of the pattern's seed, row,
step and pass (splitmix64), so the same pattern plays the same way every
time and an offline render is identical from run to run. Each hit keeps
its step's own tick, and a clip plays a hit when its step is inside the
clip, so a humanized hit or a flam just before a clip's start still
plays. The audio thread only binary-searches and reads the table. Nothing is
remembered between blocks, so a locate, a loop wrap or a tempo change
needs no special handling: the next block is computed from where the
transport is, and the engine's tests check hits land once each across
odd block sizes, through a transport loop, and at the right samples
after a tempo change mid-timeline.

Scores reach the audio thread the way graph schedules do: pushed over a
lock-free queue, swapped in at a block boundary, with the old one
retired back to the UI thread to be dropped. A pattern edit sends every
drum track a fresh score without touching the graph's structure, so
editing while the pattern plays is seamless: the next block already
plays the edit.

## The grid

The drum grid is the Beats tab of a drum track's editor,
`ui/src/components/DrumGrid.tsx`. Its arithmetic (which step the playhead
is on, what a drag makes of a velocity, the length label) is
`ui/src/lib/drums.ts`, tested on its own. Auditioning with **Loop** is
session state, not an edit: while it is on, the track's score is the
pattern alone, looping from the start of the timeline.

## Pattern clips

A pattern becomes part of a song by being placed on a drum track. A
pattern clip is like an audio clip, but kept in ticks rather than
samples:

```json
"patternClips": [
  { "id": 8, "pattern": 7, "start": 0, "offset": 0, "length": 15360 },
  { "id": 9, "pattern": 7, "start": 19200, "offset": 960, "length": 6720 }
]
```

`start` is the tick the clip begins at, `length` how many ticks it
lasts, and `offset` how far into the pattern it starts, all in the tempo
map's 960 ticks per quarter note. The pattern loops inside the clip, so
a one-bar pattern in a four-bar clip plays four times: **drag the
clip's right edge to loop it for more bars**. Because the positions are
musical, changing the tempo leaves every clip on its bars; the clips
only move in the sample timeline. The format went to version 4 with
them.

A clip may also play a **fill** (version 8):

```json
{ "id": 8, "pattern": 7, "start": 0, "offset": 0, "length": 30720,
  "fill": { "pattern": 12, "every": 4, "atEnd": true } }
```

- **`pattern`** is the pattern played on fill bars; it counts as in use,
  so it cannot be removed while a clip plays it.
- **`every`** makes the last bar of every that many a fill (0 for none,
  at most 64); **`atEnd`** makes the clip's last bar one.
- Bars count from the clip's start, at the project's time signature. A
  fill bar plays the fill pattern's first bar; the bars between play the
  clip's own pattern exactly where they would have, so the groove carries
  on after the fill instead of starting again. `score_for` cuts the clip
  into placements accordingly, and `SetPatternClipFill` (with an inverse)
  sets it.
- **Generate fill** (`pattern_clip_generate_fill`) runs
  `cantina_project::generate_fill`: one bar of the clip's pattern on the
  pads the kit has, whose last one or two beats break into a snare roll,
  a run down the toms, snare and toms in pairs, or kick-and-snare
  accents, louder towards the bar line. The seed picks the length, the
  style and small velocity differences, so the same seed writes the same
  fill. The result is an ordinary pattern, added and set as the clip's
  fill in one undoable edit.

- **Place**: in the drum grid, **Place at playhead** puts the chosen
  pattern on the track at the playhead for one pass, or drag the same
  button onto the timeline and drop it on a drum track where it should
  start (snapped to the grid). Dropping on an audio track is refused.
- **Move, trim, split, delete** work as for audio clips (see
  [timeline](/guides/recording/)): the timeline converts between samples
  and ticks on the way in and out, trimming the front keeps the beats
  where they were by moving the offset, and a pattern clip may grow
  past its pattern's length. A clip dragged onto an audio track stays
  on its own track at the new position. Each edit is a command
  (`InsertPatternClip`, `RemovePatternClip`, `MovePatternClip`,
  `TrimPatternClip`, `SplitPatternClip`) with an inverse.
- **Editing a pattern updates every clip of it**: clips refer to the
  pattern by id, the engine's score shares one copy of the pattern's
  data between its clips, and a pattern edit sends every drum track a
  fresh score.
- A pattern on the timeline cannot be removed; delete its clips first.
- The clip body shows a preview: one band per pad of the kit, a block
  per hit with the velocity as its brightness, looping from the offset.
- **Loop** (auditioning) on a track replaces that track's clips in the
  score while it is on, so the pattern is heard alone from the start of
  the timeline; turn it off to hear the arrangement.
- A groove from the browser is auditioned the same way, but from its
  file: `groove_audition` reads it with `groove_from_json`, keeps the
  pattern beside the track id (`Audition::Groove`, session state, never
  in the project), and places it from the next bar line while playing,
  or from the playhead (starting the transport) when stopped. Adding it
  is the ordinary `pattern_import`; dragging it onto the timeline adds
  it, then drops the new pattern through the same pointer-event drag the
  grid's place key uses (`pattern-drag.ts`).
