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

## Where kits are found

Cantina looks for kits in `~/Music/Cantina/Kits/`, then in the kits that
ship with the app, then in any folder the `CANTINA_KITS` environment
variable names. A kit in your folder replaces a stock kit with the same id.
**Rescan** in the kit picker finds a kit added while Cantina runs.

The stock kits in `kits/` in the repository are working examples: copy one
to `~/Music/Cantina/Kits/<id>/`, change the samples, and it appears in the
picker.

The format is `Kit` in `crates/dsp/src/kit.rs`, validated by
`Kit::from_json` with a message that names the pad and the rule.

