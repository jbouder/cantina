---
title: "Drum kit format"
description: "A drum kit is a folder with a kit.json and its samples: pads, velocity layers, choke groups and the sample formats Cantina reads."
---

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

## Schema

[`kit.schema.json`](/schemas/kit.schema.json) is a JSON Schema (draft
2020-12) generated from the types Cantina reads. Name it in `kit.json` and
an editor validates and completes the file:

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/kit.schema.json" }
```

## An annotated example

The comments are for this page; a real `kit.json` has none.

```jsonc
{
  "version": 1,                  // the format version; always 1 for now
  "name": "Acoustic",            // shown in the kit picker
  "pads": [                      // one to 64, in the order the grid shows them
    {
      "id": "kick",              // what patterns refer to; keep the common names
      "name": "Kick",            // shown in the grid; the id when missing
      "layers": [                // one sample per velocity range
        { "file": "kick-soft.wav", "velocity": [0, 0.6] },
        { "file": "kick-hard.wav", "velocity": [0.6, 1] }
      ]
    },
    // pan is -1 (left) to 1 (right); one layer needs no velocity range
    { "id": "snare", "name": "Snare", "pan": -0.1, "layers": [{ "file": "snare.wav" }] },
    // pads in the same choke group cut each other off
    { "id": "hh-closed", "name": "Closed hat", "choke": "hat", "pan": 0.3,
      "layers": [{ "file": "hh-closed.wav" }] },
    { "id": "hh-open", "name": "Open hat", "choke": "hat", "pan": 0.3, "gainDb": -3,
      "layers": [{ "file": "hh-open.wav" }] }
  ]
}
```

## Fields

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

## Where kits are found

Cantina looks for kits in `~/Music/Cantina/Kits/`, then in the kits that
ship with the app, then in any folder the `CANTINA_KITS` environment
variable names. A kit in your folder replaces a stock kit with the same id.
Cantina watches your folder, so a kit added or changed while it runs shows
up within a second; one that does not load is listed with the reason (see
[library folders](/guides/drums/#library-folders)).

The stock kits in `kits/` in the repository are working examples: copy one
to `~/Music/Cantina/Kits/<id>/`, change the samples, and it appears in the
picker.

The format is `Kit` in `crates/dsp/src/kit.rs`, validated by
`Kit::from_json` with a message that names the pad and the rule.


## Kit recipes

A kit can also be described rather than recorded. A recipe,
`<name>.recipe.json`, names each pad's voice and shapes it with a few
numbers, and `cantina-cli synth-kit` synthesises the samples and writes the
`kit.json`:

```sh
cargo run -p cantina-cli -- synth-kit recipes/lofi.recipe.json ~/Music/Cantina/Kits/lofi
```

```jsonc
{
  "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/kit-recipe.schema.json",
  "version": 1,
  "name": "Lo-fi (synth)",
  "seed": 7, // seeds the noise; the same recipe and seed give the same bytes
  "pads": [
    { "id": "kick", "voice": "kick", "pitch": 50, "decay": 0.45, "tone": 0.35 },
    { "id": "hh-closed", "voice": "hat", "decay": 0.045, "choke": "hat" },
    { "id": "hh-open", "voice": "hat", "decay": 0.35, "choke": "hat" }
  ]
}
```

| Field | What it does |
| --- | --- |
| `voice` | `kick`, `snare`, `hat`, `clap`, `tom` or `cowbell` |
| `pitch` | The fundamental in Hz, 20 to 12 000: where a kick or tom settles, a snare's body, a hat's lowest partial, a clap's band |
| `decay` | Seconds to fall by 60 dB, 0.005 to 4 |
| `tone` | Brightness, 0 to 1 |
| `noise` | How much noise is blended with the pitched part, 0 to 1 |
| `layers` | Velocity layers, 1 to 4 (2 when missing); soft layers are darker and shorter, not just quieter |
| `name`, `gainDb`, `pan`, `choke` | As on a kit's pad |

Every number but the id and voice is optional and starts at the voice's
default. The output depends only on the recipe, so a synthesised kit can
live in git and be regenerated. [`kit-recipe.schema.json`](/schemas/kit-recipe.schema.json)
is the schema; the stock recipe is `recipes/lofi.recipe.json`. Swap in
recorded samples later by editing the `kit.json` it wrote.
