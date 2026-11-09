---
title: "Command line"
description: "cantina-cli checks, renders and lists everything Cantina reads, without the app: pedals, kits, kit recipes, patterns, tabs and projects."
---

`cantina-cli` proves a file works without a window, a microphone or anyone
listening. It checks a file against its schema and the app's rules, renders
a pedal or a pattern through the same processors the app builds, and
prints the node catalog and the schemas. It is built from `crates/cli` in
the repository:

```sh
cargo run -p cantina-cli -- check pedals/fuzz.pedal.json
cargo install --path crates/cli     # or put it on your PATH
```

The binary is `cantina-cli`, not `cantina`, because the app's own binary
has that name.

## Exit status

| Code | Meaning |
| --- | --- |
| 0 | The file is valid, or the render worked |
| 1 | The file is invalid, or the render is silent or not numbers; the problems are on stderr, the first first |
| 2 | A mistake on the command line: an unknown command or flag, a missing file |

## check

```sh
cantina-cli check <file or folder> [--as <format>] [--kit <dir>] [--kits <dir>]... [--json]
```

Validates a pedal (`*.pedal.json`), a kit (a folder with `kit.json`), a kit
recipe (`*.recipe.json`), a pattern (`*.pattern.json`), a tab
(`*.tab.json`) or a project (a folder with `project.json`). The format
comes from the name, then a `$schema` field, then the keys;
`--as pedal|kit|kit-recipe|pattern|tab|project` overrides it.

First the file is checked against its [schema](/schemas/pedal.schema.json),
with each problem at its JSON pointer and, inside a pedal, the node's id
and kind:

```text
invalid pedal: bad.pedal.json
  at /nodes/1: "frequency" is not one of "id", "kind" or 6 other candidates (node "lp", a biquad)
```

Then the rules beyond the schema, the ones the app enforces:

- **Pedal**: one input and one output, everything wired, no cycles,
  bindings that resolve; the pedal is compiled as the app compiles it and
  its latency reported.
- **Kit**: every sample is there and reads as WAV.
- **Pattern**: every row has the pattern's length of velocities. With
  `--kit`, every row names a pad that kit has.
- **Tab**: the instrument has that many strings (guitar 6 to 8, bass 4
  to 6), every note is on a string the tuning has, and bends are more
  than 0 and at most 4 semitones. The summary names the tuning, the capo
  and how many bars the notes cover.
- **Project**: every pedal in a chain is a stock pedal, a plugin, or a
  designed pedal whose `pedals/<id>.pedal.json` is in the project and
  compiles; each drum track's kit is found (in `--kits`, then
  `~/Music/Cantina/Kits`) and has every pad its patterns use. An older
  project is upgraded in memory as Cantina does on open. Each clip's tab
  is checked as a tab file is.

Warnings (a pattern with no hits, a chain with fewer values than its pedal
has parameters, two notes sounding at once on one string of a tab) are printed but do not fail. `--json` prints
`{"valid", "format", "summary", "warnings"}` or `{"valid": false,
"problems"}` on stdout.

## render pedal

```sh
cantina-cli render pedal <file> [--di | --tone <hz> | --input <wav>]
    [--set <param>=<value>]... [--seconds 2] [--tail 0.5] [--bpm 120]
    [-o out.wav] [--json]
```

Runs the pedal in 64-frame blocks with the transport rolling at `--bpm`,
over the guitar DI fixture (the default, four seconds of a synthesised
guitar at DI level), a sine at -12 dBFS, or a WAV at its own rate, plus a
tail of silence. `--set` takes a parameter's id, label or index. It writes
a 32-bit float WAV and prints:

```text
wrote overdrive.wav
216000 frames, 1 channel at 48000 Hz (4.50 s)
peak     -5.1 dBFS
rms      -13.9 dBFS
dc       -0.000002
latency  22 samples
clipped  no
ok
```

A peak above 0 dBFS is runaway gain; a DC offset beyond a few thousandths
is an asymmetric curve without a high-pass after it; silence or samples
that are not numbers fail the render with status 1.

Each stock pedal rendered this way matches the golden files the dsp crate
is tested against, so the CLI hears what the app plays.

## render pattern

```sh
cantina-cli render pattern <file> --kit <dir> [--bpm 120] [--passes 2] [--tail 1] [-o out.wav] [--json]
```

Plays a pattern through a kit on the app's own sequencer and sampler, in
4/4, for a number of passes and a tail, and writes a stereo WAV with the
same numbers. Rows for pads the kit lacks are left out, as the app does,
with a warning naming them.

## synth-kit

```sh
cantina-cli synth-kit <recipe.recipe.json> <out dir>
```

Synthesises a [kit recipe](/reference/kit-format/#kit-recipes) into a kit
folder: one 16-bit WAV per velocity layer and the `kit.json`, then checks
the result. The same recipe always writes the same bytes.

## kinds and schema

```sh
cantina-cli kinds [--json]
cantina-cli schema <pedal|kit|kit-recipe|pattern|tab|project>
```

`kinds` prints the pedal node catalog: every kind, its fields with ranges,
defaults and units, and which fields can be modulated or are smoothed.
`--json` prints the catalog the designer reads. `schema` prints a format's
JSON Schema, the same file as in `schemas/`.
