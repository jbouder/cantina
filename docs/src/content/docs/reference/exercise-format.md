---
title: "Exercise format"
description: "An exercise is a *.exercise.json: a phrase to practice as tab notes or chord shapes with a strum, what it teaches, instructions, a tempo ladder, a count-in, a key, the rig it is heard through and a stock groove under it."
---

An exercise is something to practice, as a file of its own:
`<id>.exercise.json`. It holds the phrase to play, what it teaches, a few
sentences on how, the tempos to climb from a first-day start to a goal,
and the sound around it: the rig the example and your guitar go through,
and a stock groove to play over. A metal exercise heard dry over a click
teaches the wrong thing, so the rig and the groove belong to the file.

## Schema

[`exercise.schema.json`](/schemas/exercise.schema.json) is a JSON Schema
(draft 2020-12) generated from the types Cantina reads; the [exercise
schema](/reference/schemas/exercise/) page lists every field. Name it in
the file and an editor validates and completes it:

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/exercise.schema.json" }
```

## An annotated example

The comments are for this page; a real exercise has none.

```jsonc
{
  "version": 1,                        // the format version; 1 for now
  "name": "E to A",                    // shown in the exercise list
  "instrument": "guitar",              // "guitar" or "bass"
  "teaches": "change",                 // chord, change, scale, technique, riff or rhythm
  "style": "Beginner",                 // Metal, Blues, Beginner…
  "instructions": "Two bars of E, then two of A, strummed down, down, down-up. Lift only the fingers that change.",
  "key": { "tonic": "E", "mode": "major" },
  "timeSignature": { "numerator": 4, "denominator": 4 },  // the default
  "tempo": { "start": 70, "goal": 110, "step": 5 },        // bpm
  "countIn": 1,                        // bars of click first, 0 to 4
  "rig": "classic-rock",               // a rig by file name, less .rig.json
  "drums": { "kit": "rock", "groove": "Rock", "levelDb": -4 },
  "phrase": {
    "kind": "chords",
    "chords": [
      { "name": "E", "frets": [0, 2, 2, 1, 0, 0], "fingers": [0, 2, 3, 1, 0, 0],
        "start": 0, "length": 7680 },
      { "name": "A", "frets": [null, 0, 2, 2, 2, 0], "start": 7680, "length": 7680 }
    ],
    "strum": { "pattern": "D-D-D-DU", "stepsPerBeat": 2 }
  }
}
```

## The fields

- **`version`**: 1.
- **`name`**, **`style`**: what the list shows and sorts by.
- **`instrument`**: `guitar` or `bass`. It sets the voice the example
  plays on and how many strings the tuning may have.
- **`teaches`**: `chord` (a shape), `change` (moving between shapes),
  `scale` (a scale or an arpeggio as a shape on the neck), `technique`
  (picking, bends, legato, harmonics), `riff` or `rhythm` (strumming,
  muting, subdivisions).
- **`instructions`**: what the hands do, what to listen for and what goes
  wrong, in a sentence or a few.
- **`key`**: `tonic` is a note name and `mode` is `major` or `minor`.
- **`timeSignature`**: the meter it is counted in; 4/4 when left out.
- **`tempo`**: `start` is playable on the first day, `goal` is the tempo
  the music it is for is at, and `step` is how far each climb goes. All
  three are beats per minute; the start and goal are 30 to 300, the goal
  is not below the start, and the step is above 0 and no more than the
  distance between them. A start equal to the goal is one fixed tempo.
- **`countIn`**: bars of click before the first pass; 1 when left out.
- **`tuning`**: the open strings from the lowest to the highest as MIDI
  notes, as in a [tab](/reference/tab-format/); left out, standard tuning
  for six strings (a bass, four). Drop D is `[38, 45, 50, 55, 59, 64]`.
- **`capo`**: the fret the capo is at; frets in the phrase count from it.
- **`bars`**: bars in one pass of the loop. Left out, the loop is every
  bar the phrase reaches, so a two-bar riff loops every two bars.
- **`rig`**: the [rig](/reference/rig-format/) the example and your input
  are heard through, by its file name less `.rig.json`: yours in
  `~/Music/Cantina/Rigs` first, then the stock ones. Left out, both are
  dry.
- **`drums`**: what it is played over; left out, the click alone.
  - `kit`: a kit by id.
  - `groove`: a stock groove by its genre, as a [jam](/reference/jam-format/)
    names one: `Rock`, `Blues shuffle`, `Heavy metal`, `Djent`…
  - `levelDb`: the drums' level, −60 to 12.
- **`phrase`**: what to play, one of two kinds.

## Notes

```json
"phrase": {
  "kind": "notes",
  "notes": [
    { "string": 6, "fret": 0, "start": 0, "length": 480, "palmMute": true },
    { "string": 6, "fret": 0, "start": 480, "length": 240, "palmMute": true },
    { "string": 3, "fret": 7, "start": 960, "length": 960,
      "bend": { "semitones": 2, "release": true }, "vibrato": true }
  ]
}
```

The notes are a [tab](/reference/tab-format/)'s notes, every field and
technique the same: strings count from 1, the highest; frets count from
the capo; `start` and `length` are ticks, 960 to a quarter note, from the
start of the loop. A note may reach fret 24, the capo included.

## Chords

Each chord shape has a `name`, a `start` and a `length` in ticks, and
`frets`: one per string from the lowest to the highest, as the tuning
lists them, counted from the capo. `null` is a string not played and 0 an
open one. `fingers` is optional and in the same order: 1 the index finger
to 4 the little finger, 0 for none. Diagrams draw it.

Without a `strum`, each chord is struck once, low string to high, and
rings for its length. With one, the pattern repeats for as long as each
chord lasts:

- **`pattern`**: one character a step. `D` is a downstroke (low string to
  high), `U` an upstroke (high to low, a little lighter), `x` a muted
  scratch and `-` a step where the hand moves and misses. Spaces are
  ignored, so `"D-DU -UDU"` reads as two beats.
- **`stepsPerBeat`**: 1, 2, 3, 4, 6 or 8. Eighths are 2 and sixteenths 4.

Each stroke rings until the next.

## Where exercises go

| Folder | Holds |
| --- | --- |
| `exercises/` in the app | the stock exercises, read-only |
| `~/Music/Cantina/Exercises/` | yours: a file written here is read within a second, without a restart |

A file in your folder that does not load is listed with the reason, the
same one `cantina-cli check` gives, as other [library
files](/guides/agents/#the-folders) are.

## The stock exercises

Each is heard through a stock rig over a stock groove, with a tempo a
beginner can start at and a goal worth reaching.

| Exercise | Teaches | Style | Tempo | Rig | Groove |
| --- | --- | --- | --- | --- | --- |
| Barre chords: A shape | chord | Rock | 60 to 110 | `classic-rock` | Half-time |
| Barre chords: E shape | chord | Rock | 60 to 110 | `classic-rock` | Half-time |
| Open A | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open Am | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open C | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open D | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open Dm | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open E | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open Em | chord | Beginner | 60 to 100 | `country` | Half-time |
| Open G | chord | Beginner | 60 to 100 | `country` | Half-time |
| A to D | change | Beginner | 60 to 110 | `country` | Half-time |
| Am, Dm and E | change | Beginner | 60 to 110 | `country` | Half-time |
| E to A | change | Beginner | 60 to 110 | `country` | Half-time |
| Em, C, G and D | change | Beginner | 60 to 110 | `country` | Half-time |
| G to C | change | Beginner | 60 to 110 | `country` | Half-time |
| G, C and D | change | Beginner | 60 to 110 | `country` | Half-time |
| Minor barre chords | change | Rock | 60 to 110 | `classic-rock` | Half-time |
| A blues scale | scale | Blues | 60 to 130 | `blues` | Blues shuffle |
| A natural minor, one position | scale | Rock | 60 to 140 | `jazz-clean` | Half-time |
| A natural minor, three positions | scale | Rock | 60 to 140 | `jazz-clean` | Half-time |
| G major scale, one position | scale | Beginner | 60 to 140 | `jazz-clean` | Pop |
| G major scale, three positions | scale | Beginner | 60 to 140 | `jazz-clean` | Pop |
| Minor pentatonic, box 1 | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Minor pentatonic, box 2 | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Minor pentatonic, box 3 | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Minor pentatonic, box 4 | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Minor pentatonic, box 5 | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Minor pentatonic, the boxes joined | scale | Rock | 60 to 140 | `hard-rock` | Rock |
| Bends and vibrato | technique | Blues | 60 to 100 | `blues` | Blues shuffle |
| Chromatic alternate picking | technique | Rock | 60 to 140 | `classic-rock` | Rock |
| Hammer-ons and pull-offs | technique | Rock | 60 to 130 | `hard-rock` | Rock |
| Slides | technique | Blues | 60 to 120 | `blues` | Blues shuffle |
| 12-bar blues in A | riff | Blues | 70 to 120 | `blues` | Blues shuffle |
| Sixteenth funk strum | rhythm | Funk | 70 to 110 | `funk-clean` | Funk |
| Strumming eighths, down and up | rhythm | Beginner | 60 to 120 | `country` | Half-time |
| Octaves (bass) | technique | Funk | 80 to 116 | `bass-funk` | Funk |
| Walking blues in A (bass) | riff | Blues | 70 to 130 | `bass-clean` | Blues shuffle |
| Root and fifth (bass) | rhythm | Beginner | 70 to 120 | `bass-clean` | Rock |

## Checking and hearing one

```sh
cantina-cli check e-to-a.exercise.json
cantina-cli render exercise e-to-a.exercise.json            # at the start tempo, through its rig
cantina-cli render exercise e-to-a.exercise.json --bpm 110 --passes 2 --dry
```

`check` reads the file as the app does. A note off the neck, a ladder
whose goal is below its start, a chord with a fret for a string the
instrument does not have or lacks one, a strum it cannot read, and a rig
or groove that is not installed each fail it, with the reason. A kit that
is not installed, a rig for the other instrument and a ladder that never
reaches the tempos the groove suits are warnings. `render exercise` plays
the phrase on the plucked strings that play tabs, through the rig, and
prints the [render numbers](/reference/cli/#render-exercise).
