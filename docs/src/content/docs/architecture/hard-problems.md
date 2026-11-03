---
title: "Hard problems"
description: "The problems every audio app runs into, and how Cantina handles each one from the start rather than bolting it on later."
sidebar:
  order: 3
---

These are the problems that sink audio apps when they are left until later. Each one is handled in the design.

## Latency compensation

Every processor reports its latency, and a track's chain delays it by the sum. The engine delays every other track's playback to match the slowest, so clips line up at the master. Live input joins after that delay, so monitoring is never compensated: the player hears their own chain's latency and nothing else. Recorded takes are moved earlier by the compensation plus the device's input and output latency. See [the engine](/architecture/engine/#latency-compensation).

## Device changes

Sample rate and buffer size change under you when an interface is unplugged or Audio MIDI Setup is touched. The engine watches the hardware, and on a change the shell drops the whole graph and builds it again from the project: plugins are recreated and given their state, and recordings in progress are closed cleanly.

## Aliasing

Anything nonlinear (drive, fuzz, saturation, an amp) generates harmonics above Nyquist that fold back as inharmonic noise. Nonlinear stages run oversampled, two to eight times, through `cantina_dsp`'s oversampler, and the pedal format's `waveshaper` node takes an `oversample` field.

## Timeline rendering

A long take has millions of samples. The timeline is drawn on a canvas, not in the DOM, from waveform peak caches at five zoom levels built once in a Web Worker. See [the UI](/architecture/ui/#the-timeline).

## Plugin editors

Audio Unit and CLAP editors are native `NSView`s. Embedding one inside the web view is a fight nobody wins, so each opens in its own `NSWindow`, parented to the main window. See [hosting](/architecture/hosting/#editor-windows).

## Crash recovery

Autosave writes the command log, not the whole file: every edit is appended to a journal the moment it is applied, and on the next launch the edits since the last save can be replayed. See [project](/architecture/project/#autosave-and-recovery).
