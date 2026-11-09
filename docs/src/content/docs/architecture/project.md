---
title: "Project"
description: "The project directory, the model, commands with inverses, undo, versioned files and crash recovery: the project crate and how the shell uses it."
sidebar:
  order: 8
---

A project is a directory. `Song` lives at `~/Music/Cantina/Song.cantina/`:

```
Song.cantina/
  project.json    tracks, clips, chains, tempo, mixer; written by Save only
  journal.jsonl   every edit since the last save, one per line
  audio/          recorded and imported WAV files, never modified
  plugins/        third-party plugin state, one <pedal id>.state per plugin
  pedals/         the project's own copy of every designed pedal it uses
```

The model lives in `crates/project` ([`cantina_project`](/api/cantina_project/)) and knows nothing about audio devices.
The shell (`src-tauri/src/engine.rs`) owns the open project and builds the
engine graph from it; the UI reads the same model through the status it
is pushed.

## The model

`project.json` is the `Project` struct in `crates/project/src/model.rs`,
serialized in camelCase. [`project.schema.json`](/schemas/project.schema.json)
is its JSON Schema (draft 2020-12), generated from the struct with each
field's doc comment as its description; a file may name it in a
`"$schema"` field, which the loader ignores:

```json
{
  "version": 8,
  "name": "Song",
  "sampleRate": 48000.0,
  "tempo": { "bpm": 120.0, "numerator": 4, "denominator": 4 },
  "tracks": [
    {
      "id": 1,
      "name": "Guitar",
      "armed": true,
      "chain": [{ "id": 2, "spec": "overdrive", "values": [20.0, 3000.0, -6.0] }],
      "clips": [
        { "id": 3, "name": "Guitar 1", "file": "Guitar-1.wav",
          "start": 0, "offset": 0, "length": 96000 }
      ]
    }
  ]
}
```

- **Ids** are one number space across tracks, clips and pedals. They are
  never reused while an object exists, and a command that creates an
  object carries its id, so replaying a command log lands on the same ids.
- **Clips** reference a file in `audio/` by name with an `offset` and
  `length` in frames and a `start` on the timeline in samples. Editing is
  non-destructive: trimming moves `offset` and `length`, the file is never
  touched, and deleting a clip leaves the file for undo.
- **Pedals** hold a spec the engine builds a processor from (a stock name,
  `au:…` or `clap:…`) and the value of each parameter in the order the
  processor lists them. Names, units and ranges come from the processor,
  not the file. A third-party plugin's own state is saved next to the
  file as `plugins/<pedal id>.state`; see [hosting](/architecture/hosting/#state-in-the-project). A
  pedal with `"bypassed": true` keeps its settings but the signal goes
  round it; the field is left out when false.
- **Strips** (`strip` on a track, `master` on the project) hold
  `gainDb`, `pan`, `mute` and `solo`.
- **`loop`**, when present, is the loop region: `start` and `end` in
  timeline samples and `on`, whether playback loops over it. The region
  stays when looping is off; a project that never set one has no field.
  Version 6 added it; older files have no loop.
- **`sampleRate`** is the rate every file in `audio/` is at. Opening a
  project switches the engine to that rate; changing the device rate is
  only allowed while the project has no audio.
- **Drum tracks** carry a `kit` id and **`patterns`** on the project
  hold the step grids; see [drums](/architecture/drums/).
- **Tabs** sit on the clip they describe, as `tab`. A tab's notes are
  in ticks (960 per quarter note) at the tempo it was written against,
  and its `origin` is the frame of the clip's file where tick 0 falls.
  Anchored to the file rather than the timeline, the tab stays on its
  notes when the clip is moved, trimmed or split: both halves of a split
  keep the tab and each plays its part of it. `Clip::tab_sample` turns a
  tick into a timeline sample. Version 7 added it; older clips have no
  tab. The format is on the [tab schema](/reference/schemas/tab/) page.
- **Designed pedals** have the spec `pedal:<id>`; the definition is
  `pedals/<id>.pedal.json` in the project, written on Save for every
  such pedal the chains use (drafts included) and read back on open,
  so a song plays the same on its own, whatever the library holds. See
  [designer](/architecture/designer/).

## Schemas

The formats Cantina reads have JSON Schemas in `schemas/` at the root
of the repository, served by this site under `/schemas/`:
[pedal](/schemas/pedal.schema.json), [kit](/schemas/kit.schema.json),
[kit recipe](/schemas/kit-recipe.schema.json),
[pattern](/schemas/pattern.schema.json), [tab](/schemas/tab.schema.json) and
[project](/schemas/project.schema.json). They are generated from the serde
types with `schemars`, behind the `schema` feature of `cantina-dsp` and
`cantina-project`, plus the limits that live in validation (step counts,
ranges, each node kind's fields). One test writes them and fails when they
are stale, and checks every stock pedal, kit and project fixture against
them:

```sh
UPDATE_SCHEMAS=1 cargo test -p cantina-project schemas
```

Every field is on the generated [project schema](/reference/schemas/project/)
page; [authoring with an agent](/guides/agents/) covers writing projects,
pedals, kits and patterns from outside the app.

## Commands

Every edit is a `Command` (`crates/project/src/command.rs`): insert, remove,
rename, move and arm tracks; insert, remove, move, trim and split clips;
insert, remove and move pedals; set a parameter; set the tempo; set a
drum track's kit; insert, remove, rename and edit patterns; replace a
track whole (`BounceTrack`, what a bounce in place does, whose inverse
carries the track as it was, chain and all); and `Batch`, several as one
edit. `Command::apply` changes the project and
returns the inverse command, so undo is "apply the inverse" and redo is
"apply the original". A command either applies completely or leaves the
project untouched: a batch that fails part way reverts what it had done.

Every command goes through a `History` (`crates/project/src/history.rs`):
apply records the inverse, undo applies it and moves the step to the redo
stack, redo applies the command again, and a new edit clears redo. The log
keeps the last 1000 steps. Continuous changes coalesce: a run of commands
with the same `coalesce_key` (one pedal parameter, one strip's gain or pan,
the tempo) within 800 ms of each other is one step whose inverse is the
first value and whose forward command is the last, so a fader drag undoes
in one go. A property test applies random command sequences, undoes them
all and checks the model is back to the original, then redoes them all
and checks it is back at the end.

The shell's `Engine::apply` is the one place commands go through. After
applying, it brings the live graph in line: parameter and arm changes go
over the audio thread's queue; anything structural reconciles the graph's
tracks and pedals against the project (nodes that survive keep their
state), rewires and commits a new schedule, which the audio thread swaps
in on a block boundary.

## The graph

Per track, with everything summing into the master strip before the
output:

```
input -> mute -> bus -> pedals... -> strip -> master -> output
           |      ^                    |         |
      recorder  delay <- player      meter   output meter
input -> input meter
```

The graph runs in stereo: the mono input is copied to both channels on
the way in, pedals process both, and the strips pan. The mute is the
monitor gate, open while the track is armed. The recorder taps the dry
input, mono, so a take can be played back through a chain that changed
after the fact. The delay before the bus is latency compensation: each
track's playback waits for the slowest chain so clips line up at the
master, while live input, which joins after it, is never delayed (see
[the engine](/architecture/engine/#latency-compensation)). Each strip is gain, pan and mute, every one smoothed on
the audio thread so a fader move never clicks; its meter is a tap after
it. Solo is computed in the shell: a solo anywhere mutes every track that
is not soloed, and solos add up. The master strip has gain and mute.
Recording starts a take on every armed track from
the playhead; stopping finalizes each file (`<Track>-<n>.wav` in `audio/`)
and inserts it as a clip, through a command like any other edit.

## Versions and migrations

`project.json` carries a `version`. The loader (`crates/project/src/file.rs`)
reads the version first, walks the file through one migration per version
step until it reaches the current one, and only then deserializes. A file
from a newer Cantina is refused with a message that says which version it
is and which this build reads. Malformed JSON, a missing version, a shape
mismatch and an invalid model (duplicate ids, a clip with no length, a
file outside `audio/`) each produce their own error rather than a crash.

Every fixture under `crates/project/tests/fixtures/` is loaded by the
tests and must come out at the current version; when the format changes,
the old shape stays there as a fixture and a migration step is added.

### Changing the format

1. Bump `FORMAT_VERSION` in `crates/project/src/model.rs`.
2. Add a migration step in `crates/project/src/file.rs` that turns the
   previous version's JSON into the new shape.
3. Copy a file of the previous version into `crates/project/tests/fixtures/`
   and add a test that it loads with the defaults you expect.
4. Update the project example on this page.

Saves write `project.json.tmp` and rename it over `project.json`, so a
crash mid-save leaves the previous file whole.

## Autosave and recovery

`project.json` is only written by Save. Between saves, every applied
command, undo and redo inverses included, is appended as one line of JSON
to `journal.jsonl` in the project directory the moment it is applied
(`crates/project/src/journal.rs`), with the revision it brought the
project to. The file carries the revision it was saved at, so the lines
with a higher revision are exactly the edits since the last save. A
clean save clears the journal. Appending a line never touches
`project.json`, so a crash mid-edit cannot corrupt the project.

On open, lines newer than the saved file are held and the UI offers to
recover or discard them. Recover replays them in order (the replayed edits
are not undoable; they stay in the journal until the next save); Discard
empties the journal. Editing before deciding counts as discarding, since
the old session's edits would collide with the new ones. A line that does
not parse, such as one cut short by the crash, is skipped with a warning,
and the lines after it still replay, since each is self-contained. A
replay that hits an edit the project refuses stops there and says how far
it got.

To try it: make some edits, do not save, kill the app (`kill -9`), relaunch
and recover.

## Where projects live

`~/Music/Cantina/`, or the app's data folder when there is no music
folder. The Projects sheet lists every `*.cantina` directory there with a
`project.json` inside, opens one, or creates a new one from a name. The
name is the directory name, so it cannot be empty or contain a slash. On
launch the shell reopens the project that was open last (remembered in
`settings.json`), or `Untitled`, which it creates with one track carrying
the stock gain and overdrive so the first launch makes a sound.
