<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="" width="96" height="96" />
</p>

<h1 align="center">Cantina</h1>

<p align="center">
  A Mac desktop studio for guitar and bass players.
</p>

<p align="center">
  <a href="https://github.com/jbouder/cantina/actions/workflows/ci.yml"><img src="https://github.com/jbouder/cantina/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/jbouder/cantina/actions/workflows/docs.yml"><img src="https://github.com/jbouder/cantina/actions/workflows/docs.yml/badge.svg" alt="Docs" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%2013%2B-lightgrey.svg" alt="macOS 13 or later" />
</p>

<p align="center">
  <a href="https://cantina-docs.vibeproject.workers.dev/guides/getting-started/">Getting started</a> ·
  <a href="https://cantina-docs.vibeproject.workers.dev/">Documentation</a> ·
  <a href="https://cantina-docs.vibeproject.workers.dev/architecture/">Architecture</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

---

Plug in and play through a pedal chain with no perceptible delay. Record
on a multitrack timeline, build drum parts, jam over a groove and a
backing track, turn a take into a tab, run your Audio Unit and CLAP
plugins, design your own pedals as node graphs, and play on a full-screen
stage of visualizer scenes that move with the beat.

Under the hood, a Rust engine on CoreAudio runs the audio thread without
allocating, locking or blocking. A React UI in a Tauri 2 shell sits on top.
Everything you make (pedals, rigs, kits, beats, jams, tabs and projects)
is a plain JSON file with a published schema, so you, or a coding agent,
can write one and the app picks it up.

<p align="center">
  <img src="docs/src/assets/guides/studio.png" alt="The studio: a guitar and a bass track with recorded clips on the timeline, the transport and tempo above them, and the guitar's pedal chain (gain, then overdrive) below." width="96%" />
</p>

<p align="center">
  <img src="docs/src/assets/designer.png" alt="The pedal designer: an overdrive built from an input, a highpass filter and a gain node, with the selected node's settings on the right." width="48%" />
  &nbsp;
  <img src="docs/src/assets/guides/perform.png" alt="Performance mode: the spectrum bars scene full screen, with the guitar chain of overdrive, chorus, tremolo, amp and cabinet as numbered stomp switches along the bottom." width="48%" />
</p>

<p align="center"><sub>The studio, the pedal designer and the stage.</sub></p>

## Features

### Play

- **A real-time pedal chain.** A round trip of a few milliseconds on a
  class-compliant interface. Reorder, bypass and tweak pedals while you
  play; every parameter is smoothed, so nothing clicks.
- **Stock pedals.** Tuner, noise gate, compressor, overdrive, distortion
  and fuzz (oversampled, with a blend for the clean signal), EQ, chorus,
  flanger, phaser, tremolo, octaver and pitch shifter, tempo-synced delay,
  reverb, and an amp and cabinet with a starter set of impulse responses.
- **Rigs.** Pick a style (blues, funk, metalcore, shoegaze, bass rock and
  more) and the whole chain is set, trimmed to your instrument's level.
  Save your own and share them as files.
- **Plugins.** Audio Units and CLAP plugins in any chain, with their own
  editor windows, saved state and latency compensation across tracks.
  Scanning runs in a child process, so a broken plugin cannot take the
  app down.

### Record

- **Multitrack recording.** Tracks and clips on a timeline: record, move,
  trim and split, with gain, pan, mute and solo in the track headers and a
  master strip under them, a tempo map and metronome, loop playback, and
  mixdown or bounce to WAV. Audio files are never modified.
- **Undo everything.** Every edit is a command with an inverse: undo and
  redo across the whole session, autosave and crash recovery through the
  same journal.
- **Tabs.** Transcribe a recorded clip into tablature on your Mac
  (nothing leaves it), fit it to the fretboard in the track's tuning, fix
  it in the editor, play it back slowed down and looped through your
  pedals, and export MusicXML, ASCII tab, MIDI or PDF.

### Drums and jamming

- **Drums.** Kits with velocity layers, a step sequencer with swing,
  humanize, fills and feel, pads you can play and record live, a pad
  mixer, and 87 stock grooves in 17 genres with intros, verses, choruses,
  fills and endings.
- **Jam.** A practice room apart from the project: a groove, a backing
  track (a chord progression played by a built-in guitar or bass, an audio
  file or one of your tracks), a tempo, a key and your rig, on an endless
  loop you steer while you play. The last minutes are always captured, so
  **Keep that** saves the riff you were not recording.

### Design and perform

- **Pedal designer.** Build a pedal as a graph of filters, gains, shapers,
  delays and modulators, hear it live on your input as you wire it, and
  save it as a file. Real-time safe by construction, with no compiler.
- **Stage.** Full-screen visualizer scenes in WebGL 2 and Canvas 2D,
  locked to the beat, with the pedalboard as stomp switches on the
  keyboard. Jams play on stage too.
- **Six skins.** Classic, Midnight, Brushed metal, Hi-fi, Neon and Toxic.
  Skins change shape, material and displays, not only color.
- **A full menu bar.** Every feature reachable from the menus with
  standard shortcuts.

### Assistants and agents

- **An in-app assistant**, off until you connect it to an OpenAI-compatible
  endpoint, hosted or local. It reads the project, answers questions and
  makes edits that are single undo steps.
- **Built for coding agents.** JSON Schemas for every format, a
  `cantina-cli` that checks a file and renders it through the app's own
  processors, a [`cantina-author`](.claude/skills/cantina-author/SKILL.md)
  skill for Claude Code, and library folders under `~/Music/Cantina` that
  the app watches. Ask for *"a fuzz with an octave-up blend"* and get a
  pedal that loads.

## Install

Cantina runs on macOS 13 or later, on Apple silicon and Intel.

Each tagged version is built as a universal DMG on the
[Releases](https://github.com/jbouder/cantina/releases) page; the app
checks for the next one when it opens. The
[getting started](https://cantina-docs.vibeproject.workers.dev/guides/getting-started/)
guide covers the first open, picking your interface and the first
recording.

To build it yourself you need [Rust](https://rustup.rs) (stable) and
Node 22 or newer:

```sh
git clone https://github.com/jbouder/cantina.git
cd cantina
npm install
npm run tauri dev      # the app, rebuilding as the code changes
npm run tauri build    # target/release/bundle/macos/Cantina.app
```

## Documentation

The [docs site](https://cantina-docs.vibeproject.workers.dev/) has
everything, in three parts:

- **Guides**:
  [getting started](https://cantina-docs.vibeproject.workers.dev/guides/getting-started/),
  [recording](https://cantina-docs.vibeproject.workers.dev/guides/recording/),
  [pedals](https://cantina-docs.vibeproject.workers.dev/guides/pedals/),
  [rigs](https://cantina-docs.vibeproject.workers.dev/guides/rigs/),
  [the designer](https://cantina-docs.vibeproject.workers.dev/guides/designer/),
  [drums](https://cantina-docs.vibeproject.workers.dev/guides/drums/),
  [jam](https://cantina-docs.vibeproject.workers.dev/guides/jam/),
  [tabs](https://cantina-docs.vibeproject.workers.dev/guides/tabs/),
  [the stage](https://cantina-docs.vibeproject.workers.dev/guides/stage/),
  [the assistant](https://cantina-docs.vibeproject.workers.dev/guides/assistant/),
  [authoring with an agent](https://cantina-docs.vibeproject.workers.dev/guides/agents/),
  [latency](https://cantina-docs.vibeproject.workers.dev/guides/latency/).
- **Architecture**: the
  [five layers](https://cantina-docs.vibeproject.workers.dev/architecture/),
  the [`Processor` trait](https://cantina-docs.vibeproject.workers.dev/architecture/processor/),
  the [real-time rules](https://cantina-docs.vibeproject.workers.dev/architecture/real-time/),
  the [project model](https://cantina-docs.vibeproject.workers.dev/architecture/project/)
  and [transcription](https://cantina-docs.vibeproject.workers.dev/architecture/transcription/).
- **Reference**: the
  [pedal](https://cantina-docs.vibeproject.workers.dev/reference/pedal-format/),
  [rig](https://cantina-docs.vibeproject.workers.dev/reference/rig-format/),
  [jam](https://cantina-docs.vibeproject.workers.dev/reference/jam-format/),
  [exercise](https://cantina-docs.vibeproject.workers.dev/reference/exercise-format/),
  [course](https://cantina-docs.vibeproject.workers.dev/reference/course-format/),
  [kit](https://cantina-docs.vibeproject.workers.dev/reference/kit-format/) and
  [tab](https://cantina-docs.vibeproject.workers.dev/reference/tab-format/)
  formats and their schemas, the
  [CLI](https://cantina-docs.vibeproject.workers.dev/reference/cli/),
  [keyboard shortcuts](https://cantina-docs.vibeproject.workers.dev/reference/shortcuts/)
  and the [Rust API](https://cantina-docs.vibeproject.workers.dev/api/).

[PLAN.md](PLAN.md) keeps the decisions and the milestones.

## Development

```sh
npm run dev              # the UI alone in a browser, against a stand-in engine
npm run check            # Biome, color, scale and contrast checks, TypeScript, Vitest
npm run test:scenes      # draws every scene in headless Chrome

cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # engine, DSP, hosting, project and CLI

cargo run -q -p cantina-cli -- check pedals/fuzz.pedal.json
cargo run -q -p cantina-cli -- render pedals/fuzz.pedal.json
```

| Path | What it is |
| --- | --- |
| `crates/engine` | CoreAudio stream, real-time graph runtime, transport, recording and playback |
| `crates/dsp` | Pure audio processing: pedals, the pedal interpreter, the drum sampler, the offline renderer |
| `crates/hosting` | Audio Unit and CLAP hosting, and the plugin scanner |
| `crates/project` | The project model, commands with inverses, history and the file format |
| `crates/cli` | `cantina-cli`: check, render and list what Cantina reads |
| `src-tauri/` | The Tauri 2 shell joining the engine to the UI |
| `ui/` | The React UI, its skins and the stand-in engine |
| `packages/` | `@cantina/scenes` (the visualizer) and `@cantina/skins` (the token system) |
| `pedals/`, `rigs/`, `jams/`, `exercises/`, `courses/` | Stock pedals, rigs, jams, exercises and courses |
| `kits/`, `recipes/`, `patterns/`, `irs/` | Stock drum kits, kit recipes, grooves and impulse responses |
| `schemas/` | JSON Schemas for every file format, generated from the Rust types |
| `docs/` | The documentation site (Astro and Starlight) |

[CONTRIBUTING.md](CONTRIBUTING.md) is the path from a fresh clone to a
merged pull request, and [AGENTS.md](AGENTS.md) is the same for an AI
agent.

## License

[MIT](LICENSE). Third-party audio is credited in
[`kits/CREDITS.md`](kits/CREDITS.md) and [`irs/CREDITS.md`](irs/CREDITS.md).
