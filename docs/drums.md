# Drums

Beats in Cantina are a **kit** (sounds) played by a **sampler** (the
`Processor` that makes the sound) under a **sequencer** (what decides
when), from **patterns** edited in a step grid. This page covers all
four; pattern clips on the timeline are documented as M5 adds them.

## Kit format

A kit is a folder with a `kit.json` and its samples beside it:

```text
kits/
  acoustic/
    kit.json
    kick-soft.wav
    kick-hard.wav
    snare.wav
    hh-closed.wav
    hh-open.wav
```

The folder's name is the kit's id; a project refers to a kit by that id.
`kit.json` is:

```json
{
  "version": 1,
  "name": "Acoustic",
  "pads": [
    {
      "id": "kick",
      "name": "Kick",
      "layers": [
        { "file": "kick-soft.wav", "velocity": [0, 0.6] },
        { "file": "kick-hard.wav", "velocity": [0.6, 1] }
      ]
    },
    { "id": "snare", "name": "Snare", "pan": -0.1, "layers": [{ "file": "snare.wav" }] },
    { "id": "hh-closed", "name": "Closed hat", "choke": "hat", "pan": 0.3,
      "layers": [{ "file": "hh-closed.wav" }] },
    { "id": "hh-open", "name": "Open hat", "choke": "hat", "pan": 0.3, "gainDb": -3,
      "layers": [{ "file": "hh-open.wav" }] }
  ]
}
```

- **`pads`**: one to 64, each with a unique `id` that patterns refer to
  (`kick`, `snare`, `hh-closed`… the stock kits share these names so a
  pattern moves between kits), an optional display `name` (the id when
  missing), `gainDb` (default 0), `pan` from −1 to 1 (default 0) and an
  optional `choke` group.
- **`layers`**: one to 16 per pad, each a `file` next to the kit with the
  `velocity` range it answers, inclusive, inside 0 to 1 (the whole range
  when missing), and its own `gainDb`. A hit picks the first layer whose
  range holds its velocity, or the nearest when none does, so a kit with
  a soft and a hard kick plays the right one.
- **Choke groups**: hitting a pad stops every voice still ringing in
  the same group, the closed hat silencing the open one. The cut is a
  3 ms fade, never a click. Hitting the same pad again chokes its own
  earlier hit too; a pad without a group overlaps with itself.
- **Samples** are WAV, mono or stereo, 16 or 24-bit integer or 32-bit
  float, at any rate. A sample file name is a plain name next to the
  kit (`..` and absolute paths are refused), so a kit never reads
  outside its folder.

The format lives in `crates/dsp/src/kit.rs` (`Kit`, validated by
`Kit::from_json` with a message that names the pad and the rule), and the
loader in `crates/engine/src/kit.rs` (`kit::load` reads a folder,
`kit::discover` lists the kits under a set of folders, user kits
first so one can override a stock kit of the same id).

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
block it asks the tempo map which ticks the block covers, works out
which steps (with their swing) land inside it and at which frame, and
queues each hit on the sampler for exactly that frame. Nothing is
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

The drum grid is the track panel of a drum track (`ui/src/components/DrumGrid.tsx`):
the kit picker, the pattern picker with New, Rename and Remove, the
resolution, the length (in steps, with its musical size alongside) and
the swing, then the grid itself, a table of pads against steps with beat
columns set apart.

- **Click** a cell to toggle it. A new hit is at full velocity.
- **Drag up or down** on a cell to set its velocity; the fill shows it.
- **Keyboard**: one cell is in the tab order; the arrows move around,
  Space or Enter toggles, Shift with up and down nudges the velocity by
  10%, Home and End jump along the row. Every cell says its pad, step
  and velocity.
- **Loop** auditions the pattern: it loops on the track from the start
  of the timeline while the transport plays, so a pattern can be heard
  and edited before it is placed. The column under the playhead lights
  up. Looping is session state, not an edit, and ends when the pattern
  or the track goes.

The arithmetic (which step the playhead is on, what a drag makes of a
velocity, the length label) is `ui/src/lib/drums.ts`, tested on its own.
