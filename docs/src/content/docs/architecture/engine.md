---
title: "Engine"
description: "The engine crate: CoreAudio streams, the real-time graph runtime, transport and tempo map, disk streaming, latency compensation and analysis."
sidebar:
  order: 4
---

The engine is the only layer that touches the audio device, and the only code that runs on the audio thread is in here or called from here. The code is `crates/engine`; the API is [`cantina_engine`](/api/cantina_engine/).

## The stream

`stream` opens a duplex CoreAudio stream from an input device to an output device. When they are the same device, one HAL output unit runs both directions in a single I/O cycle, so nothing is buffered between the callbacks. Two different devices run on separate clocks, so one block of input is held back and drift shows in the counters. `device` lists devices, sets sample rate and buffer size, and watches for hardware changes: a device plugged in or removed, a default changed, a rate changed under the stream. The shell rebuilds the graph on any of them.

## The graph runtime

A `Graph` is built and edited on the UI thread: nodes are added, prepared and wired between the stream input, each other and the stream output. `Controller::commit` compiles it into a flat schedule with every buffer preallocated and hands it to the `Runtime` on the audio thread over a lock-free queue. The runtime swaps it in at a block boundary.

Nodes are shared between consecutive schedules, so an edit that adds a pedal leaves the others running with their state intact. Nodes that were removed go back to the UI thread inside the old schedule and are dropped there. Parameter changes go over a second queue and land at the start of the next block.

A track in the graph, with everything summing into the master strip:

```
input -> mute -> bus -> pedals... -> strip -> master -> output
           |      ^                    |         |
      recorder  delay <- player      meter   output meter
```

How the shell builds this from a project is on the [project](/architecture/project/#the-graph) page.

## Transport and tempo

The `Transport` plays, stops, locates and loops, and cuts a block at the loop wrap so each `BlockContext` covers contiguous timeline samples. It can also stop on an exact sample (`stop_at`): a take recorded over a loop stops at the loop's end instead of wrapping, and stopping is checked before wrapping so the playhead stays at the end. The shell sets the loop from the project's `loop` region on every sync. A count-in lives in the `Transport` too: the position holds and `playing` stays false while it runs, `BlockContext::count_in` tells the click where the beats fall, and the block is cut where it ends, so an armed `Recorder` starts on its exact sample and no timer on the UI side is involved. The UI thread drives it through a `TransportHandle` and reads it through a `TransportClock`. Musical time is counted in ticks, 960 per quarter note, and the `TempoMap` converts sample-accurately between samples, seconds, ticks and bar.beat.tick positions across tempo and time signature changes. The metronome (`click`) and the drum `sequencer` read the tempo map directly, so they land on the sample. The click's sounds are synthesised as each beat triggers, from up to two tones (sine or square, the first able to glide down) and a burst of high-passed noise, so nothing is loaded or allocated; its `Sound`, `Level` and `On` parameters are set through the controller like any node's, and a count-in clicks regardless of `On`. The shell wires it straight to the output, beside the master, so it is heard but not metered, analysed or mixed down, and keeps its settings in `settings.json`.

## Disk streaming

The `Player` plays clips from WAV files without touching the disk on the audio thread. A disk thread renders clips into chunks tagged with their timeline position and pushes them through a ring; the node takes the chunk for the block it is asked for and plays silence (counting an underrun) when there is none. A locate or loop change starts a new generation, and the disk thread prefetches across the loop wrap. The `Recorder` does the reverse: the node pushes the input to a ring and a writer thread writes the WAV file.

## Offline rendering

Mixdown export and bounce run the graph without a stream
(`crates/engine/src/offline.rs`). `Offline` drives a `Runtime` block by
block and, before each block, waits until every player's disk thread has
rendered past its end (`PlayerStats::is_ready_through`). The waiting
happens between blocks on the rendering thread; inside a block the nodes
run exactly the code they run on the audio thread, so the output is what
playback produces with a disk that keeps up, as fast as the machine
allows. A tenth of a second of stopped blocks runs before play, as a
stream idles before someone presses play, so pedals that move on their
own are where playback would have them.

The shell builds the graph from the project (`src-tauri/src/mixdown.rs`):
player or sequencer, compensation delay, bus, pedals, strip and master,
made the same way as the live graph, without the input, recorder, meters
and scope, which carry nothing from the timeline to the output. A render
always rolls from the top of the song and keeps the range asked for, so
tails from before a range ring into it; the compensated latency is
rendered past the end and cut from the front. A bounce builds one track
and stops before its strip. Exports at another rate go through
`cantina_dsp::resample`, a Kaiser-windowed sinc, and integer files are
written with TPDF dither from a fixed seed (`wav::write_as`), so the same
mix always writes the same bytes.

`Offline::set_paced` runs the same graph at a stream's pace without
waiting for the disk, which is how the tests hold an offline render
against playback: the shell's mixdown tests check the two are identical
sample for sample, that a bounced track plays back as its chain did, and
pin the export of a fixture song in `src-tauri/tests/golden/mixdown.wav`
(`UPDATE_GOLDEN=1 cargo test -p cantina mixdown` regenerates it).

## Latency compensation

Not the [round trip](/guides/latency/) through the interface, but what plugins add. Every
processor reports the frames it delays the signal by, and a track's
chain delays it by the sum. Without correction a track with a lookahead
limiter plays late against one without, and a take recorded along to it
lands late on the timeline.

The engine levels the tracks (`crates/engine/src/delay.rs`): `plan`
takes every chain's latency and gives each track the difference to the
slowest, and a `Delay` node on each track's *playback* branch, between
the player and the bus, holds that many frames back. Live input joins
the bus after the delay, so monitoring is never compensated: the player
hears their own chain's latency and nothing else. The compensation is
recomputed whenever a chain changes and whenever a plugin reports a new
latency (the shell polls the plugin handles a few times a second). The
engine test plays an impulse on two tracks, one through a 10-frame
lookahead, and checks both land on the same output frame once
compensated and 10 frames apart when not.

Recorded takes are moved earlier when they are placed on the timeline,
by the compensation plus the input and output latency CoreAudio reports
for the open devices (device latency, safety offset and one buffer each
way). What the player heard was that late against the timeline, and what
they played in time with it came back that much later still. A take
that would start before zero is trimmed at the front instead.

The UI shows the compensation next to the engine's device line
(`comp 12.3 ms`) and on each pedal that adds latency, and warns when it
goes above a threshold set in Settings (20 ms by default, kept in
`settings.json`).

## Analysis

A `Scope` node taps the master or one track and writes recent samples into a ring of atomics. The shell's pump runs an `Analyzer` over them each display frame, off the audio thread: a 128-band log spectrum, the 2048-sample waveform and the level. They travel to the UI with the beat position in the [frame](/architecture/frames/).

## Try it without the app

The examples run the engine from a terminal:

```sh
cargo run -p cantina-engine --example passthrough -- --list
cargo run -p cantina-engine --example passthrough -- --in Scarlett --out Scarlett --buffer 64
```

`--chain gain,overdrive` picks the pedals, `--chain gain,au:AUDelay` or `--chain 'clap:Surge XT'` a plugin, `--click 120` adds the metronome and `--play file.wav` streams a file. While it runs, type `list` to see processors and parameters, `2.drive 30` to change one, `s` for the counters and `q` to quit. `measure_latency` measures the [round trip](/guides/latency/).
