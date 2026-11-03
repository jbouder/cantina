---
title: "DSP"
description: "The dsp crate: pure audio processing with no I/O, the Processor trait, the stock pedals, the pedal format's interpreter, the sampler and golden-file tests."
sidebar:
  order: 7
---

`crates/dsp` holds everything that turns samples into other samples, and nothing else: no devices, files, threads or UI. That makes it the part of Cantina most worth reusing, and the API is [`cantina_dsp`](/api/cantina_dsp/).

## What is in it

- **The abstraction**: [`Processor`](/architecture/processor/), `Handle`, `Buffer`, `Chain`, `ParamInfo` and `Transport`.
- **Building blocks**: `Biquad`, `Allpass`, `DelayLine`, `Envelope`, `Lfo`, `Smoother`, waveshaping `Curve`s, an `Oversampler` for anything nonlinear, a radix-2 FFT and a partitioned `Convolver` with no added latency.
- **Stock pedals**: `Gain`, `NoiseGate`, `Compressor`, `Tuner`, the drives, `Echo`, `Reverb`, `Cabinet` and more. What each does is on the [stock pedals](/architecture/pedals/) page.
- **User pedals**: the `pedal` module reads a [pedal file](/reference/pedal-format/), checks it and names the node behind every problem, and compiles it into a `PedalProcessor` that allocates nothing while it runs. `pedal::kinds()` lists every node kind and field, which is what the designer's menus and the format reference come from.
- **Drums**: the `kit` module is the drum kit format, and `Sampler` plays a loaded kit with velocity layers and choke groups. The sequencer that decides when to hit a pad is in the engine, next to the tempo map.
- **Rendering**: `Renderer` runs a processor over whole signals in blocks, as a stream would, for tests and offline work.

## Rules

Every type here obeys the [real-time rules](/architecture/real-time/):

- `process` never allocates, locks, blocks or performs I/O.
- All memory a processor needs is reserved in `prepare`, which runs off the audio thread.
- Parameter changes are smoothed, so a step change never clicks.
- Anything nonlinear is oversampled.

The crate also stays free of platform code, so it would compile to WebAssembly if anyone wanted a browser build.

## Golden-file tests

`crates/dsp/tests/golden/` holds a render of every stock processor over generated fixtures (an impulse, a sine, a square wave and a sweep), compared sample by sample. A change that is meant to alter a processor's sound regenerates them, and the summary next to them shows what moved:

```sh
UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden
```

A change that is not meant to alter the sound must leave them as they are.
