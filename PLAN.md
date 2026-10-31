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
| Processor threading | One `Processor` object that moves between the UI thread and the audio thread with the graph swap; the engine will own a main-thread handle per processor from M1 | CLAP splits plugins into a main-thread half and an audio-thread half and the M0 spike found the single-object trait cannot express that. Keeping the main half on the UI thread and talking to the audio half through queues matches how parameters already travel. See `docs/spike.md`. |
| Plugin process model | In-process on the audio thread | What every mainstream DAW except Bitwig does. Scanning runs in a child process so a crashing plugin cannot take the app down at startup. Sandboxing is a later project. |
| Docs | Astro with Starlight in `docs/`, published to Cloudflare | Great docs from the start. Starlight gives search, navigation and dark mode for free, Cloudflare gives a preview deployment per pull request. Tracked in the Docs milestone, which runs alongside the numbered ones. |
| License | MIT | AU and CLAP cover the plugin need, so VST3 is dropped for good. Permissive is simplest for contributors and lets anyone reuse the `dsp` crate. Decided 2026-10-31 before the first commit. |

## Architecture

Five layers. Data flows down from the UI as commands and up as state,
meters and analysis frames. Only the engine touches the audio device, and
only the audio thread touches the graph while it runs.

```
┌────────────────────────────────────────────────────────────┐
│ ui (React, TypeScript)                                     │
│ timeline · mixer · pedalboard · drum grid · designer · stage│
└───────────────▲──────────────────────────┬─────────────────┘
                │ state, meters,            │ commands
                │ analysis (binary channel) │ (Tauri invoke)
┌───────────────┴──────────────────────────▼─────────────────┐
│ src-tauri (shell)                                          │
│ owns the engine handle, project file, windows, menus       │
└───────────────▲──────────────────────────┬─────────────────┘
                │                           │
┌───────────────┴───────────────────────────▼────────────────┐
│ engine                                                     │
│ CoreAudio stream · real-time graph runtime · transport and │
│ tempo map · lock-free queues · disk streaming · analysis   │
├────────────────────────┬───────────────────────────────────┤
│ hosting                │ dsp                               │
│ AU and CLAP: scan,     │ pure processing, no I/O: filters, │
│ instantiate, latency,  │ waveshapers, delays, modulation,  │
│ state, editor windows  │ convolution, sampler, drum synth  │
├────────────────────────┴───────────────────────────────────┤
│ project                                                    │
│ serde model · undo command log · file format · migrations  │
└────────────────────────────────────────────────────────────┘
```

### The processor abstraction

A pedal chain is a list of **processors**. A processor has a name, a
parameter list, a latency in samples, a state blob, and a `process(buffers)`
call. Stock pedals, user-designed pedals and third-party plugins all
implement it, so the chain, the mixer, automation, latency compensation and
the project file never ask which kind they hold. Getting this trait right is
the point of the M0 spike.

### Real-time rules

The audio callback never allocates, locks, blocks, does I/O, or calls into
anything that might. Everything else talks to it through lock-free queues:
parameter changes and graph edits go in, meters and analysis frames come
out. Graph edits are built on the UI thread and swapped in atomically; the
old graph is dropped on the UI thread. Parameters are smoothed on the audio
thread so changes never click.

### Project model

Non-destructive from day one. A project is a directory:

```
MySong.cantina/
  project.json        tracks, clips, chains, patterns, tempo map, mixer
  audio/              recorded and imported WAV files, never modified
  plugins/            third-party plugin state blobs
  pedals/             user pedal definitions used by this project
```

Clips reference files in `audio/` with an offset and length. Undo is a
command log over the model, so audio edits and parameter changes share one
history. `project.json` carries a version and the loader upgrades older
files in place.

### Pedal definition format

A user pedal is a JSON graph of DSP primitives with exposed parameters:

```json
{
  "version": 1,
  "name": "Warm Drive",
  "params": [{ "id": "drive", "label": "Drive", "min": 0, "max": 1, "default": 0.4 }],
  "nodes": [
    { "id": "in", "kind": "input" },
    { "id": "pre", "kind": "biquad", "type": "highpass", "freq": 80 },
    { "id": "gain", "kind": "gain", "db": { "param": "drive", "map": [0, 30] } },
    { "id": "clip", "kind": "waveshaper", "curve": "tanh", "oversample": 4 },
    { "id": "tone", "kind": "biquad", "type": "lowpass", "freq": 4500 },
    { "id": "out", "kind": "output" }
  ],
  "edges": [["in", "pre"], ["pre", "gain"], ["gain", "clip"], ["clip", "tone"], ["tone", "out"]]
}
```

The same format describes the stock pedals, so the designer can open any of
them as a starting point. Drum kits and patterns are JSON too.

### Visualizer

The engine runs an FFT and waveform capture on the master bus, or a selected
track, and sends frames at display rate over a binary Tauri channel. The
scenes package takes one frame per draw:

```ts
interface AnalysisFrame {
  spectrum: Float32Array;   // magnitudes, log-spaced bins
  waveform: Float32Array;   // recent samples
  rms: number;
  beat: { bar: number; beat: number; phase: number; bpm: number } | null;
}
```

Butterchurn expects an AnalyserNode, so it gets a thin adapter. Scenes
that know the beat phase lock to it exactly instead of guessing from onsets.

## Hard problems to respect from the start

- **Latency compensation.** Every processor reports latency; the engine
  delays the other tracks to match, and live input monitoring bypasses the
  compensation so the player never hears a delay.
- **Device changes.** Sample rate and buffer size change under you when an
  interface is unplugged or Audio MIDI Setup is touched. The graph rebuilds,
  plugins are reinitialized, recordings in progress are closed cleanly.
- **Aliasing.** Anything nonlinear (drive, fuzz, saturation) oversamples.
- **Timeline rendering.** Canvas, not DOM, with waveform peak caches at
  several zoom levels computed off the main thread.
- **Plugin editors.** AU and CLAP editors are native NSViews. They open in
  their own NSWindow, parented to the main window, which avoids embedding a
  native view inside the web view.
- **Crash recovery.** Autosave the command log, not the whole file, and
  replay it on the next launch.

## Repository shape

```
cantina/
  Cargo.toml              workspace
  crates/
    dsp/                  pure processing, golden-file tests
    engine/               CoreAudio, graph runtime, transport, queues
    hosting/              AU and CLAP, scanner binary
    project/              model, undo, file format
  src-tauri/              shell, commands, channels, menus
  ui/                     React app
  packages/
    scenes/               visualizer scenes, extracted from music-visualizer
    skins/                the six player skins, shared with music-visualizer
  docs/                   Astro docs site, published to Cloudflare
  PLAN.md
```

Likely crates: `cpal` or `coreaudio-rs` for the stream, `rtrb` for lock-free
queues, `realfft` for analysis, `hound` and `symphonia` for audio files,
`clack` for CLAP hosting, `objc2` and `coreaudio-sys` for Audio Units,
`serde` for everything persisted. UI: React, TypeScript, Tailwind v4, Biome.

## Milestones

One issue per pull request. Each milestone has a demo you can play. A
Docs milestone runs alongside them: the Astro site goes up first, then each
milestone lands its guides and references on it.

### M0 Spike: can this work?

Goal: guitar in to guitar out through CoreAudio with a measured round trip,
then one AU and one CLAP plugin in the path. Go/no-go on latency and on the
processor trait.

Outcome (2026-10-31): trait **go**, with the threading decision above owed
to M4. Latency **pending hardware**: the measurement tool is built and the
bar is in `docs/latency.md`, but no interface was available to run it.
Findings in `docs/spike.md`.

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

### Later

- MIDI input and virtual instruments.
- Neural Amp Modeler files as a processor.
- Plugin sandboxing out of process.
- A browser build from the `dsp` crate, if anyone wants it.
