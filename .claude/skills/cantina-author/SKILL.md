---
name: cantina-author
description: Write files for Cantina, the Mac guitar studio. Covers pedals (*.pedal.json effect graphs such as drives, fuzz, chorus, tremolo and delay), drum kit recipes (*.recipe.json, synthesised into a kit), drum patterns (*.pattern.json beats), rigs (*.rig.json, a chain of pedals with settings for a style), jams (*.jam.json, a groove, backing chords, tempo, key and rig to practise with) and projects (project.json). Checks each file with cantina-cli and renders it before putting it in the ~/Music/Cantina library, where the running app picks it up. Use when asked for a Cantina pedal, effect, tone, rig, beat, groove, drum pattern, drum kit, jam, backing track, chord progression or project, or to fix one that will not load.
---

# Authoring for Cantina

Cantina reads everything as JSON: pedals, kits, kit recipes, patterns, rigs,
jams and projects. Every format has a JSON Schema, and `cantina-cli` checks a file
the way the app does and renders it through the app's own processors. Do
not hand a file over until `check` passes and `render` sounds sane by the
numbers.

## The loop

1. **Read the format.** [references/formats.md](references/formats.md) has
   every field of every format. For a pedal, also read
   [references/node-catalog.md](references/node-catalog.md) for the node
   kinds and their fields, and [references/design-rules.md](references/design-rules.md)
   for how the stock pedals are built. Start from the closest worked
   example in [examples/](examples/).
2. **Draft** the file. Put `"$schema"` first so editors validate it:
   `https://cantina-docs.vibeproject.workers.dev/schemas/<format>.schema.json`,
   where `<format>` is `pedal`, `kit`, `kit-recipe`, `pattern`, `rig`, `jam` or `project`.
3. **Check** it, and fix every problem it prints, then check again:

   ```sh
   cantina-cli check my-fuzz.pedal.json
   cantina-cli check half-time.pattern.json --kit ~/Music/Cantina/Kits/<kit>
   ```

   Exit 0 is valid. Exit 1 prints every problem on stderr, each at a JSON
   pointer and, in a pedal, naming the node (`at /nodes/3/freq: ... (node
   "tone", a biquad)`). Exit 2 is a mistake on the command line.
4. **Render** it and read the numbers (add `--json` for a machine-readable
   result):

   ```sh
   cantina-cli render pedal my-fuzz.pedal.json --di         # a guitar DI
   cantina-cli render pedal my-fuzz.pedal.json --tone 110   # a sine, -12 dBFS
   cantina-cli render pedal my-fuzz.pedal.json --set drive=40 --set level=-12
   cantina-cli render pattern half-time.pattern.json --kit <kit dir> --bpm 140
   cantina-cli render rig crunch.rig.json                    # its instrument's DI
   ```

   Then judge it against the design rules:
   - `clipped` must be `no` and the peak below 0 dBFS at the defaults.
   - The level at the defaults is within a few dB of the input (the DI
     peaks at -6 dBFS, RMS about -23 dBFS).
   - `dc` is under 0.001; more means a missing 20 Hz high-pass after an
     asymmetric curve.
   - `silent` or samples that are not numbers make `render` exit 1.
   - Try the extremes of the knobs that matter (`--set drive=<max>`) too.
   - If you can listen, the WAV is written next to you (`-o` to choose).
5. **Place** the file where the app looks. Cantina watches these folders
   and picks up a new or changed file within a second, no restart; a file
   that does not load is listed with the same reason `check` gives:

   | Format | Goes in |
   | --- | --- |
   | Pedal | `~/Music/Cantina/Pedals/<id>.pedal.json` (the id is the file stem: lower case, dashes) |
   | Kit | `~/Music/Cantina/Kits/<id>/` with `kit.json` and the samples |
   | Pattern | `~/Music/Cantina/Patterns/<Name>.pattern.json` |
   | Rig | `~/Music/Cantina/Rigs/<id>.rig.json` |
   | Jam | `~/Music/Cantina/Jams/<id>.jam.json` |
   | Project | `~/Music/Cantina/<Name>.cantina/project.json` |

   Then tell the person where it is and what to do in the app: a pedal is in
   the pedal picker under *Your pedals*; a kit in a drum track's **Kit**
   list; a pattern in the **Pattern** menu under *Import from the library*;
   a rig in the **Rigs** picker on an audio track's pedalboard; a jam in
   the Jam view's **Jams** list.

## Kits

You cannot record a drum. Write a kit recipe instead and synthesise it:

```sh
cantina-cli check lofi.recipe.json
cantina-cli synth-kit lofi.recipe.json ~/Music/Cantina/Kits/lofi
```

`synth-kit` writes one WAV per velocity layer and the `kit.json`, then
checks the result. The same recipe always writes the same bytes. Voices
are `kick`, `snare`, `hat`, `clap`, `tom` and `cowbell`, shaped by `pitch`,
`decay`, `tone` and `noise`; see
[examples/boom-bap.recipe.json](examples/boom-bap.recipe.json). A person
can swap real samples in later by editing `kit.json`.

## Rigs

A rig is a chain of existing pedals with their settings, for a style:
"a metalcore tone". Build it from stock pedals (`cantina-cli check` lists
them when one is wrong), cabinets as `cab:<impulse>` (`4x12-modern`,
`2x12-closed`, `1x12-open-back`, `4x10-bass`) and designed pedals as
`pedal:<id>`. Name parameters by id or label (`"drive": 18`,
`"curve": "Soft"`, `"sync": true`); anything left out is at its default.
Start from a stock rig in `rigs/` of the repository or
[examples/crunch.rig.json](examples/crunch.rig.json).

Level it: `render rig` plays the DI of the rig's `instrument` through it.
The stock rigs sit at about -19 dBFS RMS (cleans a little lower, peaks
under -1.5 dBFS); set the amp's `level` so yours does too, so switching
rigs does not jump in volume.

## Jams

A jam is what plays along while someone practises: drums, a backing, a
tempo, a key and the rig their guitar goes through. Name things the app
ships: a stock groove by its genre (`Rock`, `Blues shuffle`, `Heavy metal`;
the folders of `patterns/` in the repository, each with its tempo range),
a kit by id (`rock`, `vintage`, `jazz`, `metal`…) and rigs by their file
name less `.rig.json` (`blues`, `bass-clean`). Keep the tempo inside the
groove's range. The backing is usually a chord progression in the key:
Roman numerals count from the tonic as a major scale does, whatever the
mode (`i | bVI | bVII | i` in a minor key), upper case major, lower case
minor, with suffixes `7`, `maj7`, `m7`, `5`, `sus4`, `dim`…; `|` between
bars, `-` holds. `cantina-cli check` reads the progression and warns about
anything not installed. Start from a stock jam in `jams/` or
[examples/desert-blues.jam.json](examples/desert-blues.jam.json).

## Running the CLI

In the Cantina repository: `cargo run -q -p cantina-cli -- <command>`.
Installed: `cantina-cli <command>` (`cargo install --path crates/cli`).
`cantina-cli kinds` prints the node catalog and `cantina-cli schema
<format>` a schema, if the references here are older than the build.

## Examples

| File | What it shows |
| --- | --- |
| [warm-drive.pedal.json](examples/warm-drive.pedal.json) | A drive: tight high-pass, gain into an oversampled asymmetric clipper, DC blocker, log-mapped tone, level |
| [vibe-chorus.pedal.json](examples/vibe-chorus.pedal.json) | Modulation: a unipolar `lfo` with stereo spread sweeping a short `delay` |
| [dark-echo.pedal.json](examples/dark-echo.pedal.json) | A delay: `split`, a feedback `delay` darkened by a low-pass, a wet `gain`, summed in a `mix` |
| [boom-bap.recipe.json](examples/boom-bap.recipe.json) | A kit recipe: shared pad ids, hats in a choke group, balanced with `gainDb` |
| [half-time.pattern.json](examples/half-time.pattern.json) | A two-bar half-time beat: snare on 3, accents, ghost notes, open hat, light swing |
| [crunch.rig.json](examples/crunch.rig.json) | A rig: a light overdrive into a mid-gain amp and a two-twelve, a choice by name, leveled with the amp |
| [desert-blues.jam.json](examples/desert-blues.jam.json) | A jam: the stock shuffle at a slow tempo, a minor 12-bar in numerals on a bass through a stock rig |

[fixtures/](fixtures/) holds one-line prompts and what was written for
them; every example and fixture is checked and rendered in CI by
`scripts/skill-fixtures.sh`.
