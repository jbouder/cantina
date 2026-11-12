---
title: "Groove format"
description: "A groove is a *.groove.json: a whole drum part as the drum designer saves it, with its sections, order, kit, tempo and time signature."
---

A groove is a whole drum part as a file of its own: `<id>.groove.json` in
`~/Music/Cantina/Grooves`. The [drum designer](/guides/drum-designer/)
saves one with **Save as groove…** and opens one from its palette, and a
[jam](/guides/jam/) can play one. This page is the file.

The stock grooves are not groove files: they stay one
[pattern file](/reference/schemas/pattern/) per section, with a `groove`
block saying their genre and section, and are read as they are.

## Schema

[`groove.schema.json`](/schemas/groove.schema.json) is a JSON Schema
generated from the types Cantina reads; the [groove
schema](/reference/schemas/groove/) page lists every field.

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/groove.schema.json" }
```

## An example

```json
{
  "version": 1,
  "name": "Late train",
  "genre": "Rock",
  "description": "A driving rock part.",
  "tempo": 118,
  "timeSignature": { "numerator": 4, "denominator": 4 },
  "kit": "rock",
  "sections": [
    {
      "name": "Verse",
      "kind": "verse",
      "pattern": { "name": "Verse", "stepsPerBeat": 4, "length": 16, "swing": 0,
                   "rows": [ { "pad": "kick", "steps": [1,0,0,0, 0,0,0,0, 1,0,0,0, 0,0,0,0] } ] },
      "fill": { "section": "Fill", "every": 8 },
      "variations": [ { "name": "Verse B", "stepsPerBeat": 4, "length": 16, "swing": 0, "rows": [] } ]
    },
    { "name": "Fill", "kind": "fill",
      "pattern": { "name": "Fill", "stepsPerBeat": 4, "length": 16, "swing": 0, "rows": [] } }
  ],
  "order": [ { "section": "Verse", "times": 8 } ]
}
```

## The fields

- **`version`**: 1.
- **`name`**: what the palette shows and what a jam names it by. Unique
  among your grooves; a stock genre of the same name is found first.
- **`genre`**, **`description`**: optional, shown in the palette.
- **`tempo`**: the tempo it was made at, 20 to 400 bpm. **`timeSignature`**:
  the meter it was made in; a jam plays it in that.
- **`kit`**: the kit by id. One that is not installed is a warning from
  `cantina-cli check`; the designer keeps the track's kit instead.
- **`sections`**: at least one. Each has:
  - **`name`**, unique within the groove: the order and fills name
    sections by it.
  - **`kind`**: `intro`, `verse`, `chorus`, `bridge`, `fill` or `ending`,
    or left out for a section of your own.
  - **`pattern`**: a pattern as a pattern file has it, without an `id`:
    its steps, velocities, step marks (chance, ratchet, flam), swing and
    humanize.
  - **`fill`**: optional; another section's first bar played on the last
    bar of `every` so many (0 or left out for none, at most 64), and with
    `atEnd` on the section's last bar in each step of the order.
  - **`variations`**: optional, B to D, each the same steps and resolution
    as `pattern`. **`vary`** says how they take turns a pass each:
    `alternate` (A B A C A D, the default) or `chain` (A B C D).
- **`order`**: the song, steps of `section` played `times` over (1 to
  64). It may be empty.

## In a jam

A jam whose `drums.groove` names one of your grooves plays it by kind: the
first section of each kind, a whole round of its variations, in the
groove's time signature. A groove with no verse plays its first section
that loops (not an intro, fill or ending) as the verse. Sections that are
not whole bars, and sections of your own kind, are left out.
