---
title: "Rig format"
description: "A rig is a *.rig.json: a chain of pedals with their settings, a genre, a description, the instrument it suits and the input gain it expects."
---

A rig is a chain of pedals with their settings, as a file of its own:
`<id>.rig.json`. The stock rigs in `rigs/` of the repository are rigs, and
so is anything an agent writes or a player saves. The [rigs
guide](/guides/rigs/) is about using them; this page is the file.

## Schema

[`rig.schema.json`](/schemas/rig.schema.json) is a JSON Schema (draft
2020-12) generated from the types Cantina reads; the [rig
schema](/reference/schemas/rig/) page lists every field. Name it in the
file and an editor validates and completes it:

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/rig.schema.json" }
```

## An annotated example

The comments are for this page; a real rig has none.

```jsonc
{
  "version": 1,                        // the format version; 1 for now
  "name": "Metalcore",                 // shown in the rig picker
  "genre": "Metal",                    // the picker groups rigs by it
  "description": "Tight, gated high gain with a scooped mid.",
  "instrument": "guitar",              // "guitar" or "bass"
  "inputGainDb": 0,                    // gain before the first pedal, -24 to 24
  "chain": [                           // first pedal to last
    { "pedal": "gate", "params": { "threshold": -58, "release": 40 } },
    { "pedal": "overdrive",            // a stock pedal by name
      "params": { "drive": 4, "level": 0, "curve": "Hard" } },
    { "pedal": "amp", "params": { "gain": 34, "mid": -5, "level": -20.7 } },
    { "pedal": "cab:4x12-modern" },    // a cabinet with its impulse
    { "pedal": "reverb", "params": { "mix": 8 }, "bypassed": true }
  ]
}
```

## Pedals

`pedal` names what to build, the way a project's chain does:

| `pedal` | What it is |
| --- | --- |
| `overdrive`, `amp`, `reverb`… | A stock pedal. `cantina-cli check` lists them when one is wrong. |
| `cab:<impulse>` | A cabinet with that impulse response: `4x12-modern`, `2x12-closed`, `1x12-open-back`, `4x10-bass`, or a file in `~/Music/Cantina/IRs`. |
| `pedal:<id>` | A [designed pedal](/guides/designer/), `<id>.pedal.json` in `~/Music/Cantina/Pedals`. |
| `au:<id>`, `clap:<bundle>` | An Audio Unit or a CLAP plugin, with its saved state in `state` (base64). |

`bypassed: true` keeps a pedal in the chain with its settings and sends
the signal round it.

## Parameters

`params` names each value rather than listing them in order, so a rig
still reads when a pedal gains a parameter:

- The key is the pedal format's id (`drive`, `freq2`) or the label the
  pedal shows (`Drive`, `Pre-delay`), case aside. When two parameters share
  a label, as the EQ's bands do, use the id.
- A number is in the parameter's own unit: decibels, hertz, milliseconds,
  percent.
- A choice takes its option's name (`"curve": "Hard"`, `"mode":
  "Ping-pong"`) or its index; a switch takes `true` or `false`.
- A parameter left out starts at its default.

A name that is not one of the pedal's parameters, a value outside its
range, or an option it does not have is a problem, and `cantina-cli
check` names the pedal and the parameter.

A plugin's `state` is applied first and its `params` after. Saving a
plugin into a rig writes its state alone, which carries every parameter.

## Missing pedals

A rig that names a plugin, a designed pedal or an impulse that is not
installed still loads: the rest of the chain plays, and Cantina, like
`cantina-cli check`, says which pedals are left out and why.

## Levels

`inputGainDb` is how hot the rig wants its input. Every stock rig plays
the DI of its instrument at about -19 dBFS RMS (cleans a few dB lower so
their peaks stay under -1.5 dBFS), within 6 dB of each other, so
switching rigs does not jump in volume. A test pins each one's peak, RMS
and latency (`crates/cli/fixtures/rig-levels.json`); level a rig of your
own with [`cantina-cli render rig`](/reference/cli/#render-rig) and the
amp's `level`.

## Versions

`version` is 1. A later Cantina that changes the format reads version 1
files and upgrades them; a file newer than the Cantina reading it is
refused with the version it has.
