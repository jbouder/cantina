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

M0 Spike: can this work? The engine passes audio from an input device to an
output device through a gain. Nothing else exists yet.

## Build

macOS only. Install Rust with [rustup](https://rustup.rs), then:

```sh
cargo build --workspace
cargo test --workspace
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
for the callback and dropout counters, `q` to quit.

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
crates/engine   CoreAudio devices and streams, latency measurement; later the graph runtime
crates/hosting  Audio Units and CLAP plugins as Processors
crates/clap-fixture  a tiny CLAP plugin the hosting tests load
docs/           design notes and measurements; the Astro site lands here
```
