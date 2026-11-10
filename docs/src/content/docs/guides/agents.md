---
title: "Authoring with an agent"
description: "Ask a coding agent for a pedal, a rig, a beat, a jam or a drum kit and get a file that loads: the cantina-author skill, the write-check-render-place loop, the library folders and example prompts."
---

Everything Cantina reads is JSON: pedals, rigs, jams, drum kits, patterns and projects.
A coding agent such as Claude Code can write any of them. Cantina gives it
the three things it needs to get them right without you in the loop: a
schema for every format, a command line tool that checks a file and
renders it through the app's own processors, and folders the app watches
so a finished file shows up in the app without a restart.

Ask for *"a fuzz with an octave-up blend"*, *"a half-time beat at 140"* or
*"a kit for lo-fi hip hop"*, and the agent writes the file, proves it
loads and sounds sane by the numbers, and puts it where Cantina picks it
up.

:::tip[No coding agent?]
Cantina's own [assistant](/guides/assistant/#write-pedals-beats-and-kits)
runs the same loop inside the app, with the same references and the same
checks. It needs only an API key or a local model, not a coding agent.
:::

## Install the skill

The knowledge an agent needs is packaged as a skill, `cantina-author`, in
the repository at `.claude/skills/cantina-author/`:

```text
cantina-author/
  SKILL.md                the workflow
  references/
    formats.md            every field of every format (generated)
    node-catalog.md       every pedal node kind and field (generated)
    design-rules.md       how the stock pedals, kits and patterns are built
  examples/               a drive, a chorus, a delay, a kit recipe, a beat
  fixtures/               one-line prompts and what was written for them
```

- **Claude Code in the Cantina repository** finds it with no setup: project
  skills in `.claude/skills/` load when they are relevant.
- **Claude Code anywhere else**: copy the folder to your personal skills,
  so it is there in every project:

  ```sh
  cp -R .claude/skills/cantina-author ~/.claude/skills/
  ```

- **Other agents** that read a `skills/` folder take the same folder as it
  is: copy `cantina-author/` into it. An agent without skills can be
  pointed at `SKILL.md` and told to follow it; every link inside is
  relative.

The agent also needs `cantina-cli`. Install it once from the repository so
it is on the `PATH`:

```sh
cargo install --path crates/cli
```

or let the agent run it from the repository as
`cargo run -q -p cantina-cli -- <command>`.

## The loop

The skill has the agent work in four steps, and not hand anything over
until the first three pass:

1. **Write** the file from the format reference and the closest example,
   with a `"$schema"` field naming the format's schema.
2. **Check** it: `cantina-cli check <file>` validates it against the
   schema and the rules the app enforces beyond it, and names the node, pad
   or row of every problem. The agent fixes and checks again until it
   passes.
3. **Render** it: `cantina-cli render pedal <file> --di` plays a guitar DI
   through the pedal as the app would and reports peak, RMS, DC offset,
   latency and clipping; `render rig` plays the DI of a rig's instrument
   through its whole chain; `render pattern` plays a pattern through a kit. The
   [design rules](#what-good-looks-like) say what the numbers should be.
4. **Place** it in a library folder. Cantina shows it within a second.

The [command line](/reference/cli/) reference has every command and flag.

## The folders

| Folder | Holds | Shows up in |
| --- | --- | --- |
| `~/Music/Cantina/Pedals/` | `<id>.pedal.json` | the pedal picker, under *Your pedals* |
| `~/Music/Cantina/Kits/` | a folder per kit with `kit.json` and samples | a drum track's **Kit** list |
| `~/Music/Cantina/Patterns/` | `<Name>.pattern.json` | the **Pattern** menu, under *Import from the library* |
| `~/Music/Cantina/Rigs/` | `<id>.rig.json` | the rig picker on an audio track's pedalboard, marked *yours* |
| `~/Music/Cantina/Jams/` | `<id>.jam.json` | the Jam view's **Jams** list, under *Yours* |

Cantina watches the five folders while it runs. A file added or changed
there appears within a second; a file that does not load is listed as
*will not load* with the same reason `cantina-cli check` gives, and cannot
be picked until it is fixed. See [library folders](/guides/drums/#library-folders).

Drum kits are a special case: an agent cannot record a snare. It writes a
[kit recipe](/reference/kit-format/#kit-recipes) instead, a few numbers per
pad, and `cantina-cli synth-kit` synthesizes the samples and the
`kit.json` straight into the kits folder. You can swap in real samples
later.

## Example prompts

These are the skill's fixtures: the prompt, and the file that came out,
checked and rendered in CI on every change.

| Prompt | What comes out |
| --- | --- |
| a fuzz with an octave-up blend | `octave-fuzz.pedal.json`: a high-passed input into a fuzz gain, split into a germanium clipper and an `octave` waveshaper, the octave branch blended by a knob, then a DC blocker, a log-mapped tone and a level. Both branches are oversampled, so they line up at 22 samples of latency. At the defaults it peaks at -10 dBFS from a -6 dBFS DI. |
| a half-time beat at 140 | `half-time-140.pattern.json`: a bar of sixteenths with the snare on beat 3, a syncopated kick, alternating strong and weak closed hats and an open hat into the bar line, played at 140 bpm. |
| a kit for lo-fi hip hop | `lofi.recipe.json`: a dark, short kick and snare, a soft clap, dusty hats in a choke group, two toms and a rim, balanced with pad gains; `synth-kit` makes it a 15-sample kit. |

More that work well:

- *"A tremolo that can sync to the tempo, with a square option."*
- *"A bass drive that keeps the low end: clip a high-passed copy and blend
  it with the clean signal."*
- *"A shuffle beat in triplets for the electronic kit."*
- *"An 808-style kit with a long kick."*
- *"Fix my pedal; it says will not load."* The agent runs `check` on the
  file and reads the reason.

## What good looks like

The skill's design rules are the ones the stock pedals follow. The short
version, all of which `render` measures:

- At its defaults a pedal comes out within a few dB of the DI, with the
  peak below 0 dBFS and nothing clipped.
- Every waveshaper is oversampled, and asymmetric curves are followed by
  a 20 Hz high-pass, so `dc` stays under 0.001.
- Knobs are in real units (dB, Hz, ms, %), frequencies are log-mapped, and
  defaults are where the pedal sounds good on first insert.
- Rigs play their instrument's DI at about -19 dBFS RMS with the peak
  under -1.5 dBFS, leveled with the amp's `level`, as the [stock
  rigs](/guides/rigs/) are.
- Kits use the shared pad ids (`kick`, `snare`, `hh-closed`, `hh-open`,
  `tom-hi`, `tom-lo`) so patterns move between them; patterns hold exactly
  their length of velocities per row.

## Writing files by hand

Nothing here needs an agent. The same references serve a person:

- the [pedal format](/reference/pedal-format/), [kit format](/reference/kit-format/),
  [rig format](/reference/rig-format/), [jam format](/reference/jam-format/)
  and [pattern files](/guides/drums/#pattern-files);
- a generated page per schema: [pedal](/reference/schemas/pedal/),
  [kit](/reference/schemas/kit/), [kit recipe](/reference/schemas/kit-recipe/),
  [pattern](/reference/schemas/pattern/), [rig](/reference/schemas/rig/),
  [jam](/reference/schemas/jam/) and
  [project](/reference/schemas/project/);
- `cantina-cli check` and `render` from a terminal, and an editor that
  validates against the `"$schema"` as you type.
