---
title: "The processor"
description: "The Processor trait every pedal implements, from a stock gain to a CLAP plugin, and the handle that keeps a plugin's main-thread half on the UI thread."
sidebar:
  order: 2
---

A pedal chain is a list of **processors**. A stock pedal, a pedal built in the designer and a third-party plugin all implement [`cantina_dsp::Processor`](/api/cantina_dsp/trait.Processor.html), so the chain, the mixer, automation, latency compensation and the project file never ask which kind they hold.

## The trait

| Method | Thread | What it does |
| --- | --- | --- |
| `name`, `params`, `param`, `set_param` | either | Name, parameter list (name, unit, range, default) and values in the parameter's own units. Out-of-range values are clamped. |
| `prepare(sample_rate, max_block_frames)` | UI | Allocates everything the processor needs and snaps smoothers to their targets, so a parameter set before the stream starts does not fade in. Called again on any change. |
| `process(buffer)` | audio | Processes a block in place. Follows the [real-time rules](/architecture/real-time/). |
| `set_transport(transport)` | audio | Tempo, position and play state before each block, for a synced LFO or a delay at a note value. |
| `reset` | UI | Clears delay lines and filter memory, as after a seek. |
| `latency` | either | Samples of delay the processor adds, for [compensation](/architecture/engine/#latency-compensation). |
| `state`, `set_state` | UI | An opaque blob for the project file. The default is every parameter as a little-endian `f32`. |
| `handle` | UI | The processor's UI-thread side, taken once before it goes into a graph. Plugins return one; stock pedals return nothing. |
| `readout` | either | Values the processor measures for its display, such as a tuner's pitch. |

Parameter names are `Cow<'static, str>`: stock pedals borrow static tables, plugins report names when they load. `state` takes `&mut self` because both plugin APIs hand out state through mutable handles.

## Moving between threads

A processor is one object that moves between the UI thread and the audio thread with the graph swap. It is prepared on the UI thread, travels to the audio thread inside a compiled schedule, and comes back when it leaves the graph. While it is in a running graph only `process`, `set_transport` and queued parameter changes reach it.

Plugin APIs split a plugin into a main-thread half and an audio-thread half and expect the host to use both at once: to save state, show an editor or read parameters while audio runs. That is what the handle is for.

## The handle

`Processor::handle()` returns a [`cantina_dsp::Handle`](/api/cantina_dsp/trait.Handle.html). The shell keeps it next to the pedal's node id for as long as the pedal is in a chain, and uses it for:

- **state**: `state()` and `set_state()` while the processor runs;
- **latency**: `latency()` as it is now, since a plugin can change it after it was prepared;
- **changes the plugin made on its own**: `take_changes()` returns the parameters that moved since the last call, from the plugin's editor or a preset loaded inside it. The shell polls every handle a few times a second and records each change as an undoable `SetParam` edit, so the generic knobs and undo follow whatever the plugin's own UI does;
- **the format's own features** through `as_any()`, which is how editor windows reach an Audio Unit instance or the CLAP GUI extension.

Nothing on a handle is ever called from the audio thread. How each format shares one plugin instance between the two halves is on the [hosting](/architecture/hosting/) page.

## Writing a stock pedal

1. Add a module to `crates/dsp/src/` with a struct that implements `Processor`. Allocate in `new` or `prepare`, smooth every continuous parameter with a `Smoother`, and oversample anything nonlinear (see `oversample.rs`).
2. Give it a `static` parameter table of `ParamInfo`s.
3. Register its spec in the engine's processor builder (`crates/engine/src/processors.rs`).
4. Add it to the golden-file renders in `crates/dsp/tests/golden.rs` and regenerate them with `UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden`.

Many pedals are better written in the [pedal format](/notes/pedal-format/) instead: a JSON graph of DSP primitives that is real-time safe by construction.
