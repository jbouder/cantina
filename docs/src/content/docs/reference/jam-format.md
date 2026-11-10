---
title: "Jam format"
description: "A jam is a *.jam.json: a stock groove or patterns of its own, a backing (a chord progression, an audio file or a project's track), a tempo, a key and the rig the input plays through."
---

A jam is what plays along while you practice, as a file of its own:
`<id>.jam.json`. The stock jams in `jams/` of the repository are jams, and
so is anything an agent writes or a player saves with **Save…** in the
Jam view. The [jam guide](/guides/jam/) is about playing them; this page
is the file.

## Schema

[`jam.schema.json`](/schemas/jam.schema.json) is a JSON Schema (draft
2020-12) generated from the types Cantina reads; the [jam
schema](/reference/schemas/jam/) page lists every field. Name it in the
file and an editor validates and completes it:

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/jam.schema.json" }
```

## An annotated example

The comments are for this page; a real jam has none.

```jsonc
{
  "version": 1,                         // the format version; 1 for now
  "name": "Blues in A",                 // shown in the Jams list
  "genre": "Blues",                     // the list groups jams by it
  "description": "A 12-bar shuffle with a walking-root bass under it.",
  "tempo": 96,                          // beats per minute, 30 to 300
  "timeSignature": { "numerator": 4, "denominator": 4 },
  "key": { "tonic": "A", "mode": "major" },  // what the numerals count from
  "drums": {
    "kit": "vintage",                   // a kit by id
    "groove": "Blues shuffle",          // a stock groove, by its genre
    "levelDb": 0                        // -60 to 12
  },
  "rig": "blues",                       // the input's rig, by file name
  "backing": {
    "kind": "progression",
    "chords": "I7 | I7 | I7 | I7 | IV7 | IV7 | I7 | I7 | V7 | IV7 | I7 | V7",
    "instrument": "bass",               // "guitar" or "bass"
    "rhythm": "quarter",                // "whole", "half", "quarter", "eighth"
    "rig": "bass-clean",                // the voice's rig, by file name
    "levelDb": -2
  }
}
```

## The fields

- **`version`**: 1.
- **`name`**, **`genre`**, **`description`**: what the Jams list shows.
- **`tempo`**: beats per minute, 30 to 300. With an audio backing the jam
  plays at the backing's tempo instead.
- **`timeSignature`**: the meter every section plays in. A stock groove
  sets it from its verse, and only that genre's grooves in that meter are
  used: the 3/4 jazz waltz is not a 4/4 jazz bridge.
- **`key`**: `tonic` is a note name (`A`, `F#`, `Bb`) and `mode` is
  `major` or `minor`. Roman numerals in a progression count from the
  tonic.
- **`drums`**:
  - `kit`: a kit by id, as a drum track names it. One that is not
    installed plays on the first kit Cantina has.
  - `groove`: a stock groove by its genre, as the groove browser shows
    it: `Rock`, `Blues shuffle`, `Heavy metal`, `Half-time`… Each
    section, `intro`, `verse`, `chorus`, `bridge`, `fill` and `ending`,
    plays that genre's pattern for it, if it has one.
  - `patterns`: patterns of the jam's own, by section, written as
    [pattern files](/guides/drums/#pattern-files) are, without an id. A
    section with its own pattern plays it in place of the groove's; a
    jam may have patterns and no groove. Each is a whole number of bars.
  - `levelDb`: the drums' level.
- **`rig`**: the [rig](/reference/rig-format/) the input plays through, by
  its file name less `.rig.json`: the user's in `~/Music/Cantina/Rigs`
  first, then the stock ones. Left out, the input is dry.
- **`backing`**: one of these, by `kind`; left out, the drums alone.
  - `progression`: `chords` played by `instrument`, the built-in guitar or
    bass, in `rhythm`, through `rig`, at `levelDb`. It follows the jam's
    tempo.
  - `audio`: the WAV at `file`, which is at `bpm`, round and round from
    bar 1 in whole bars, at `levelDb`.
  - `track`: the audio `track` of the `project`, by name, its clips
    through its chain, round and round from the start to the end of its
    last clip, at `levelDb`. The jam plays at the project's tempo.

## Chord progressions

`chords` is text. With `|` anywhere, the chords between two bar lines
share the bar equally; without, each chord is a bar. `-` (or `%`) holds
the chord before it.

A chord is a Roman numeral or a name, a suffix, and an optional `/` and a
bass note:

- **Numerals** count from the key's tonic as a major scale does, in any
  mode: `I` to `VII`, with `b` or `#` before for the notes between. Upper
  case is a major chord and lower case a minor one: in A minor, `i` is A
  minor, `bIII` is C major and `bVII` is G major.
- **Names** are a letter, `#` or `b`, and the suffix, and ignore the key:
  `Am`, `F#m7`, `Bbmaj7`.
- **Suffixes**: none (a triad), `m` (minor, on a name), `5` (a power
  chord), `6`, `7`, `9`, `add9`, `maj7` (or `M7`, `Δ`), `maj9`, `m7`,
  `sus2`, `sus4`, `7sus4`, `dim` (or `°`), `dim7`, `m7b5` (or `ø`), `aug`
  (or `+`).
- **A bass note** after `/` is a numeral or a name: `C/E`, `V/vii`.

`cantina-cli check` names any chord it cannot read. The [jam
guide](/guides/jam/#chord-progressions) lists the stock progressions.

## Where jams go

| Folder | Holds |
| --- | --- |
| `jams/` in the app | the stock jams, read-only |
| `~/Music/Cantina/Jams/` | yours: **Save…** writes here, and a file dropped here appears in the Jams list within a second |
| `~/Music/Cantina/Jams/Takes/` | what Keep that wrote, not jams |

A file that does not load is listed with the reason, as other library
files are. `cantina-cli check` reads a jam as the app does and warns
about a kit, rig or audio file that is not installed.
