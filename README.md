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

M7 Pedal designer done. User pedals are a JSON graph of DSP
primitives (gain, filter, oversampled waveshaper, delay, lfo, allpass,
mix, split) that the `dsp` crate validates, naming the node behind
every problem, and compiles into a processor that allocates nothing
while it runs. Nine of the stock pedals now ship in that format, five
of them bit for bit what they were and the rest within float
rounding. The
designer edits a pedal on a canvas while it plays in the chain, with
a side panel for each node's fields (constants, parameters with maps,
signals from other nodes) and the pedal's knobs; a broken graph shows
its problems inline and the last valid pedal keeps playing. Pedals
save to a library, projects keep their own copies, and single files
import and export by dialog or by dropping them on the window. See
[docs/pedal-format.md](docs/pedal-format.md) and
[docs/designer.md](docs/designer.md). Next: M8 Stage.

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

- **Engine** in the title bar opens the audio devices. macOS asks for
  microphone permission the first time; grant it or there is no input.
  Pick devices, sample rate and buffer size in **Settings** (the gear);
  they are remembered in
  `~/Library/Application Support/com.jbouder.cantina/settings.json`.
- A project called `Untitled` is created in `~/Music/Cantina/` with one
  track carrying the stock gain and overdrive, so playing makes a sound
  as soon as a track is armed.
- The **+** slot at the end of a pedalboard opens the picker: stock
  pedals, your own, and the installed plugins. The first time, it scans
  the installed Audio Units and CLAP bundles in a separate process and
  caches the list next to the settings file.

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
