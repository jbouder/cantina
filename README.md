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

M3 Multitrack in progress. The project model landed: a `.cantina`
directory with `project.json` and immutable audio, every edit a command
with an inverse, a versioned loader, and a shell that builds the engine
graph from any number of tracks, records takes into the project and
places them as clips. Clips live on a canvas timeline with waveforms from
peak caches built in a worker: move, trim, split and delete, snapped to a
grid from the tempo. Before that, M2 built the shell: a Tauri 2 app with
a device picker that survives relaunches and rebuilds the stream when an
interface vanishes or changes rate, input and output meters over a binary
frame channel, a pedal chain you can reorder while audio runs, and the
six visualizer skins (Classic, Midnight, Brushed metal, Hi-fi, Neon,
Toxic) on a token system where no component names a color. M1 built the
engine: a graph of processors edited on the UI thread and swapped in on
a block boundary, a transport with tempo map, loop and click, WAV
playback and recording on their own threads, and golden-file tests for
the DSP. M0's latency measurement still needs an audio interface. Spike
findings in [docs/spike.md](docs/spike.md), the project format in
[docs/project.md](docs/project.md), the timeline in
[docs/timeline.md](docs/timeline.md), the skin system in
[docs/skins.md](docs/skins.md), the frame format in
[docs/frames.md](docs/frames.md).

## Build

macOS only. Install Rust with [rustup](https://rustup.rs) and Node 22,
then:

```sh
npm install
npm run tauri dev      # the app, with the UI hot-reloading
npm run tauri build    # target/release/bundle/macos/Cantina.app and a DMG
```

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
