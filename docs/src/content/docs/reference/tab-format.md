---
title: "Tab format"
description: "A guitar or bass tab as Cantina keeps it: on a clip in project.json or as a .tab.json file; strings, frets, ticks, techniques, limits and checking a file."
---

A tab is a guitar or bass part as strings and frets. Cantina keeps one on
the clip it was transcribed from, inside `project.json`, and the same
object can stand alone as `<name>.tab.json`. Every field is listed on the
[tab schema](/reference/schemas/tab/) page; this page explains how they
fit together. For using tabs in the app, see the [tabs guide](/guides/tabs/).

## Schema

[`tab.schema.json`](/schemas/tab.schema.json) is a JSON Schema (draft
2020-12) generated from the types Cantina reads. Name it in a tab file and
an editor validates and completes it:

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/tab.schema.json" }
```

## An annotated example

The comments are for this page; a real file has none.

```jsonc
{
  "name": "Riff",                       // shown in the tab view and on exports
  "instrument": "guitar",               // or "bass"
  "tuning": [38, 45, 50, 55, 59, 64],   // open strings, lowest first, as MIDI notes: drop D
  "capo": 2,                            // frets count from here; left out for none
  "tempo": { "bpm": 110, "numerator": 4, "denominator": 4 },  // what the ticks mean
  "notes": [
    // string 1 is the highest; start and length are ticks, 960 a quarter note
    { "string": 6, "fret": 0, "start": 0, "length": 240, "palmMute": true },
    { "string": 6, "fret": 0, "start": 240, "length": 240, "palmMute": true },
    // a chord is notes that start together
    { "string": 6, "fret": 3, "start": 480, "length": 480 },
    { "string": 5, "fret": 5, "start": 480, "length": 480 },
    // hammer-ons and pull-offs are marked on the note they sound
    { "string": 3, "fret": 5, "start": 960, "length": 240 },
    { "string": 3, "fret": 7, "start": 1200, "length": 240, "hammerOn": true },
    // a slide is marked on the note it leaves
    { "string": 4, "fret": 7, "start": 1440, "length": 240, "slide": true },
    { "string": 4, "fret": 9, "start": 1680, "length": 480, "velocity": 0.8 },
    { "string": 2, "fret": 8, "start": 2160, "length": 960,
      "bend": { "semitones": 2, "release": true }, "vibrato": true },
    // a natural harmonic over the 12th fret, left to ring
    { "string": 1, "fret": 12, "start": 3120, "length": 720, "harmonic": true, "letRing": true }
  ]
}
```

## How things are counted

- **Strings** count from 1, the highest-pitched, as tab is read: a
  six-string guitar's low E is string 6. A guitar has 6, 7 or 8 strings,
  a bass 4, 5 or 6, and the length of `tuning` says which.
- **`tuning`** runs the other way, lowest string first, as players name a
  tuning: standard guitar is `[40, 45, 50, 55, 59, 64]` (E2 A2 D3 G3 B3
  E4), standard bass `[28, 33, 38, 43]`.
- **Frets** count from the capo, from 0 to 36. A note sounds the open
  string, plus the capo, plus the fret.
- **Time** is in ticks, 960 to a quarter note, from tick 0 at the tab's
  `tempo`. A sixteenth is 240, an eighth-note triplet 320.
- **`velocity`** is 0 to 1, and 1 when left out.
- **A string plays one note at a time.** Two notes that overlap on one
  string are allowed, but `cantina-cli check` warns about them.

## Techniques

Every technique is a field on a note, left out when off.

| Field | Means | In the tab view | In ASCII tab |
| --- | --- | --- | --- |
| `hammerOn` | Sounded by hammering on from the note before on the string | `h` | `5h7` |
| `pullOff` | Sounded by pulling off from the note before | `p` | `7p5` |
| `slide` | Slides into the next note on the string | `/` or `\` | `7/9` |
| `bend` | Bent up `semitones` (more than 0, at most 4); with `release`, back down | `↑full↓` | `8b10r8` |
| `vibrato` | Vibrato | `~~` | `7~` |
| `palmMute` | Muted at the bridge | `P.M. - -` | a `PM` line |
| `letRing` | Left to ring into the next notes | `let ring - -` | a `let ring` line |
| `harmonic` | A natural harmonic over the fret | `<12>` | `<12>` |
| `dead` | A muted click with no pitch; the fret is where the hand rests | `x` | `x` |
| `tap` | Tapped with the picking hand | `T` | `t15` |

A note cannot be both a hammer-on and a pull-off.

## On a clip

In `project.json` a clip's tab has one more field, **`origin`**: the
frame of the clip's audio file where tick 0 falls. The tab is anchored to
the audio rather than the timeline, so it stays on its notes when the clip
is moved, trimmed or split, and notes the clip has trimmed away are kept
but not played. A tab from a transcription also keeps **`heard`**: the
notes as transcription heard them, in seconds from tick 0, which is what
re-quantising builds from. A tab file never has an `origin`, and one
written by hand needs no `heard`.

## Checking a file

```sh
cargo run -q -p cantina-cli -- check riff.tab.json
```

checks a tab against the schema and the rules the schema cannot say: a
string count the instrument has, notes on strings the tuning has, frets
and bends in range. It names the note behind every problem, and warns
when two notes overlap on a string. Cantina does not open `.tab.json`
files on their own yet; to share a tab with other apps, export it as
MusicXML, ASCII tab, MIDI or PDF (see
[exporting](/guides/tabs/#export-a-tab)).

The format is `Tab` in `crates/project/src/tab.rs`, read by
`tab_from_json`.
