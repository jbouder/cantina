# Cantina

A Mac desktop studio for guitar and bass players: plug in, play through a
pedal chain with no perceptible delay, layer tracks, build drum beats, run
third-party plugins, design your own pedals, and watch it all on a stage of
music visualizer scenes.

Open source under the MIT license. See [PLAN.md](PLAN.md) for the design,
architecture and milestones, and the
[GitHub milestones](https://github.com/jbouder/cantina/milestones) for
progress.

## Status

M5 Drums done. Beats are in: a drum kit format (pads with velocity
layers, choke groups, gain and pan) and a sample-accurate polyphonic
sampler; a step sequencer that computes every block's hits from the
tempo map, so patterns follow tempo changes and loops; a drum grid with
click-to-toggle, drag-for-velocity and full keyboard access, with
swing, resolution and length; patterns as clips on the timeline, kept
in ticks so they stay on their bars, with a step preview and the same
move, trim and split as audio clips; and three stock kits (acoustic,
tight rock, electronic) built from public-domain samples with every
source credited. Drums in [docs/drums.md](docs/drums.md). M4 Hosting
came before. Third-party plugins are in: a scanner that examines
Audio Units and CLAP bundles in a child process, so a plugin that
crashes on load is listed as failed instead of taking the app down, with
a cache that only rescans what changed; Audio Units and CLAP plugins as
full processors with parameters, latency and state saved in the project;
native editor windows parented to the main window, with Apple's generic
view for units without one; and latency compensation that delays every
track's playback to the slowest chain while live monitoring is never
delayed, with recorded takes placed back where they were played. The
threading decision the spike left open is settled: the engine keeps a
plugin's main-thread half on the UI thread as a handle. M3 built the project model: a `.cantina` directory with `project.json` and immutable
audio, every edit a command with an inverse, a canvas timeline with
peak caches, a mixer, undo and redo, and autosave with recovery. M2
built the shell: a Tauri 2 app with a device picker that survives
relaunches, meters over a binary frame channel, a pedal chain, and the
six visualizer skins. M1 built the engine: a graph of processors swapped
in on a block boundary, a transport with tempo map, loop and click, WAV
playback and recording on their own threads, and golden-file tests for
the DSP. M0's latency measurement still needs an audio interface. Spike
findings in [docs/spike.md](docs/spike.md), hosting in
[docs/hosting.md](docs/hosting.md), latency and compensation in
[docs/latency.md](docs/latency.md), the project format in
[docs/project.md](docs/project.md), the timeline in
[docs/timeline.md](docs/timeline.md), the skin system in
[docs/skins.md](docs/skins.md), the frame format in
[docs/frames.md](docs/frames.md).

## Start the app

macOS only. You need Rust (install with [rustup](https://rustup.rs),
which puts `cargo` in `~/.cargo/bin`; make sure that is on your `PATH`)
and Node 22. Then, from the repository root:

```sh
npm install            # once
npm run tauri dev      # builds the shell, starts Vite and opens the window
```

The first build compiles the Rust workspace and takes a few minutes;
after that a launch is seconds. The UI hot-reloads when files under
`ui/` change, and the shell rebuilds and relaunches when Rust files
change. Stop it with `Ctrl-C` in that terminal.

On first launch:

- **Start engine** opens the audio devices. macOS asks for microphone
  permission the first time; grant it or there is no input. Pick devices,
  sample rate and buffer size under **Devices…**; they are remembered in
  `~/Library/Application Support/com.jbouder.cantina/settings.json`.
- A project called `Untitled` is created in `~/Music/Cantina/` with one
  track carrying the stock gain and overdrive, so playing makes a sound
  as soon as a track is armed.
- **Plugins…** on a track opens the plugin browser; the first time, it
  scans the installed Audio Units and CLAP bundles in a separate process
  and caches the list next to the settings file.

For a release build, which embeds the UI and needs no Vite:

```sh
npm run tauri build    # target/release/bundle/macos/Cantina.app
open target/release/bundle/macos/Cantina.app
```

## Build and test

The engine alone, without the shell:

```sh
cargo build --workspace
cargo test --workspace
```

The UI is checked with Biome and TypeScript; `npm run check` runs both,
`npm run format` fixes what Biome can. `npm run dev` serves the UI in a
browser against a stand-in engine, which is enough for layout and skin work
and is what `scripts/screenshot.sh` captures for pull requests.

The DSP tests include golden-file renders in `crates/dsp/tests/golden/`:
every stock processor over generated fixtures, compared sample by sample.
A change that is meant to alter a processor's sound regenerates them, and
the summary next to them shows what moved:

```sh
UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden
```

## Run the passthrough

List devices, then open a stream. Names match a prefix, case-insensitively,
so `--in Scarlett` finds a Scarlett 2i2.

```sh
cargo run -p cantina-engine --example passthrough -- --list
cargo run -p cantina-engine --example passthrough -- --in Scarlett --out Scarlett --buffer 64
```

The first run asks for microphone permission; macOS blocks the stream until
you answer. The default chain is a gain into an overdrive; `--chain` picks
another, for example `--chain gain,overdrive,gain` or
`--chain gain,au:AUDelay` for an Audio Unit (`--list-au` names them) or
`--chain 'clap:Surge XT'` for a CLAP plugin (`--list-clap`). While it runs, type
`list` to see processors and parameters, `2.drive 30` to change one, `s`
for the callback and dropout counters, `q` to quit. `--click 120` mixes in a
metronome and `--play file.wav` streams a mono WAV at the stream rate from
the start of the timeline; `play`, `stop`, `loop 4`, `pos`, `rec take.wav`
and `rec stop` drive the transport and the recorder while it runs.

Using the same device for input and output runs one duplex audio unit with
no buffering between the callbacks. Two different devices run on separate
clocks, so one block of input is held back and drift shows up in the
counters. Turn the gain down before using the built-in microphone and
speakers together, or it will feed back.

## Measure round-trip latency

Connect an output of your interface to an input with a cable, then:

```sh
cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 64
```

It plays clicks, times their return and prints a row for
[docs/latency.md](docs/latency.md).

## Layout

```
crates/dsp      pure processing: the Processor trait, Chain, stock Gain and Overdrive
crates/engine   CoreAudio devices, streams and hardware watcher; graph runtime, meters, transport, WAV streaming
crates/hosting  Audio Units and CLAP plugins as Processors
crates/project  the project model, commands with inverses, the .cantina file format
crates/clap-fixture  a tiny CLAP plugin the hosting tests load
src-tauri/      the Tauri 2 shell: owns the engine and the open project, exposes commands to the UI
ui/             the React and TypeScript app; Tailwind v4 reads the six skins' tokens
scripts/        developer tools, such as the PR screenshot capture
docs/           design notes, measurements and screenshots; the Astro site lands here
```
