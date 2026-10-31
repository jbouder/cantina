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

M1 Engine done. The engine runs a graph of processors that is edited on the
UI thread and swapped in on a block boundary, with parameters smoothed on
the audio thread; a transport with a tempo map, loop and metronome click;
WAV playback and recording streamed on their own threads; and an offline
renderer with golden-file tests for the DSP. M0's latency measurement still
needs an audio interface. Spike findings in [docs/spike.md](docs/spike.md).

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
crates/clap-fixture  a tiny CLAP plugin the hosting tests load
src-tauri/      the Tauri 2 shell: owns the engine handle, exposes commands to the UI
ui/             the React and TypeScript app, styled with Tailwind v4 through skin tokens
scripts/        developer tools, such as the PR screenshot capture
docs/           design notes, measurements and screenshots; the Astro site lands here
```
