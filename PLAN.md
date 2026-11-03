# Cantina

A Mac desktop studio for guitar and bass players: plug in, play through a
pedal chain with no perceptible delay, layer tracks, build drum beats, run
third-party plugins, design your own pedals, and watch it all on a stage of
music visualizer scenes.

Open source, built for one person first. The name is the Mos Eisley cantina,
where the band plays.

## Decisions

| Question | Decision | Why |
| --- | --- | --- |
| Platform | macOS only | CoreAudio gives a ~5 ms round trip with a class-compliant interface, aggregate devices for free, one signed DMG to ship. No ASIO, no ALSA, no cross-platform audio abstractions to fight. |
| Engine language | Rust | Real-time audio needs no garbage collector, no allocation on the audio thread, and a good lock-free ecosystem. Rust has all three plus a growing audio ecosystem. |
| UI | React and TypeScript in Tauri 2 | Existing skills, and the visualizer scenes already run in a web view. Tauri 2 channels carry binary payloads, which the analysis stream needs. |
| Browser version | Not a goal | The DSP core stays I/O-free and would compile to WebAssembly, but nothing is built or tested for it. |
| Third-party plugins | Audio Units first, CLAP second, VST3 deferred | AU covers nearly every commercial Mac plugin. CLAP is MIT licensed with a pure Rust host library and is the escape hatch for people who want to write a pedal in code. VST3's SDK is GPLv3 or proprietary and forces the project license. |
| User-made pedals | Visual node graph over DSP primitives, saved as JSON | Real-time safe by construction, shareable, no compiler. Power users write a CLAP plugin instead, so no Faust tier. |
| Skins | Port the six player skins from `music-visualizer` (Classic, Midnight, Brushed metal, Hi-fi, Neon, Toxic) and build the whole UI on their token system | They already exist and define the look. Every component styles through semantic tokens on `data-skin`, so skins change shape, material and displays, not just color. Shared with the visualizer in a package once the scenes are. |
| Visualizer | Extract the scenes from `music-visualizer` into a shared package | 23 WebGL 2 and Canvas 2D scenes plus 395 MilkDrop presets already exist. Cantina adds exact beat and bar positions from the tempo map. |
| Processor threading | One `Processor` object that moves between the UI thread and the audio thread with the graph swap; the engine will own a main-thread handle per processor from M1 | CLAP splits plugins into a main-thread half and an audio-thread half and the M0 spike found the single-object trait cannot express that. Keeping the main half on the UI thread and talking to the audio half through queues matches how parameters already travel. |
| Plugin process model | In-process on the audio thread | What every mainstream DAW except Bitwig does. Scanning runs in a child process so a crashing plugin cannot take the app down at startup. Sandboxing is a later project. |
| Docs | Astro with Starlight in `docs/`, published to Cloudflare | Great docs from the start. Starlight gives search, navigation and dark mode for free, Cloudflare gives a preview deployment per pull request. Tracked in the M10 Docs milestone, which runs alongside the others. |
| License | MIT | AU and CLAP cover the plugin need, so VST3 is dropped for good. Permissive is simplest for contributors and lets anyone reuse the `dsp` crate. Decided 2026-10-31 before the first commit. |

## Architecture

The architecture lives on the docs site, next to the code it describes:
the [five layers](https://cantina-docs.vibeproject.workers.dev/architecture/),
the [`Processor` trait](https://cantina-docs.vibeproject.workers.dev/architecture/processor/),
the [real-time rules](https://cantina-docs.vibeproject.workers.dev/architecture/real-time/),
the [project model](https://cantina-docs.vibeproject.workers.dev/architecture/project/),
the [hard problems](https://cantina-docs.vibeproject.workers.dev/architecture/hard-problems/)
and the [API reference](https://cantina-docs.vibeproject.workers.dev/api/).
This file keeps the decisions and the milestones.

## Milestones

One issue per pull request. Each milestone has a demo you can play. A
M10 Docs milestone runs alongside them: the Astro site goes up first, then each
milestone lands its guides and references on it. M11 Layout was the UX
redesign that ran before M8 Stage; M12 AI runs alongside M8.

### M0 Spike: can this work?

Goal: guitar in to guitar out through CoreAudio with a measured round trip,
then one AU and one CLAP plugin in the path. Go/no-go on latency and on the
processor trait.

Outcome (2026-10-31): trait **go**, with the threading decision above owed
to M4. Latency **pending hardware**: the measurement tool is built and the
bar is in `docs/src/content/docs/notes/latency.md`, but no interface was available to run it.

- Workspace, `dsp` and `engine` crates, CoreAudio passthrough with a gain.
- Measure round trip with a loopback cable and a click; record the number.
- Processor trait; a stock gain and a stock overdrive implement it.
- Load one AU by identifier and run it as a processor.
- Load one CLAP by path and run it as a processor.
- Write up findings and settle the trait.

### M1 Engine

- Graph runtime with atomic swap and parameter smoothing.
- Transport, tempo map, metronome click.
- WAV playback from disk on a streaming thread.
- WAV recording to disk on a streaming thread.
- Offline renderer and golden-file test harness for `dsp`.

### M2 Shell

- Tauri app, React scaffold, Tailwind, Biome.
- Skin system on semantic tokens, and the six visualizer skins ported.
- Device picker, sample rate and buffer size, input and output meters.
- One track: arm, record, play, with a reorderable pedal chain.
- Binary channel for meters.

### M3 Multitrack

- Project model, file format, save and load, versioned loader.
- Tracks and clips on a canvas timeline: move, trim, split, delete.
- Mixer: gain, pan, mute, solo, master.
- Undo and redo over the command log.
- Autosave and recovery.

### M4 Hosting

- Scanner child process, plugin list cache, rescan.
- AU in a chain with parameters, latency, state in the project.
- CLAP in a chain, same.
- Editor windows.
- Latency compensation across tracks with monitoring bypass.

### M5 Drums

- Drum kit format and sampler.
- Step sequencer: steps, velocity, swing, pattern length.
- Patterns as clips on the timeline, following the tempo map.
- Stock kits.

### M6 Stock pedals

- Tuner, noise gate, compressor.
- Overdrive, distortion, fuzz with oversampling.
- EQ, chorus, flanger, phaser, tremolo.
- Delay with tempo sync, reverb.
- Amp and cabinet via impulse response convolution, with a starter IR set.

### M7 Pedal designer

- Pedal JSON format and interpreter in `dsp`.
- Stock pedals rewritten in the format where it makes sense.
- Visual node editor with live preview on the input.
- Save, load, import and export pedals.

### M8 Stage

- Analysis frames on a binary channel.
- Extract the scenes package from `music-visualizer` with the frame contract.
- Butterchurn adapter.
- Beat-locked scene behaviors from the tempo map.
- Beat-driven skin effects, and skins shared with the visualizer.
- Performance mode: full-screen stage, pedalboard overlay, scene switching.

### M9 Ship

- Mixdown export to WAV, bounce a track in place.
- Keyboard shortcuts and accessibility pass.
- Signing, notarization, DMG, auto-update.
- Contributing guide, first release.

### M12 AI

Everything Cantina reads as JSON can be written by a coding agent, checked
without the app, and picked up by the app.

- JSON Schemas for pedals, kits, patterns and projects, generated from the
  Rust types and tested against the stock files.
- `cantina` CLI: `check` a file against the schema and the app's rules,
  `render` a pedal or a pattern offline through the real processors and
  report peak, RMS, DC and latency, `kinds` for the node catalog.
- Patterns as standalone `*.pattern.json` files with import and export.
- Kit recipes: synthesise a complete kit's samples and `kit.json` from JSON.
- A Claude Code skill, `cantina-author`, with generated references, worked
  examples and the design rules the stock pedals follow; evals whose
  outputs must pass `cantina check`, run in CI.
- Library folders the app watches, so an agent-written file appears in the
  picker without a restart and a broken one says why.
- A guide to authoring with an agent, plus generated schema and CLI
  reference pages on the docs site.

### Later

- An MCP server so an agent can load a pedal or pattern into the running
  app, audition it and read the meters.
- MIDI input and virtual instruments.
- Neural Amp Modeler files as a processor.
- Plugin sandboxing out of process.
- A browser build from the `dsp` crate, if anyone wants it.
