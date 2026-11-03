---
title: "Latency"
description: "What the buffer size trades, how to measure your own round trip with a loopback cable, and how to contribute the result."
sidebar:
  order: 3
---

Latency is the time from plucking a string to hearing it through Cantina.
Under about 10 ms it feels like an amp; much more and your timing starts to
drift. Most of it is set by one choice: the **Buffer size** in Settings.

## Buffer size

The Mac hands audio to Cantina a buffer at a time, and Cantina hands it back
the same way, so every buffer of input waits for one buffer of output. One
buffer lasts its size divided by the sample rate:

| Buffer | At 44.1 kHz | At 48 kHz | At 96 kHz |
| --- | --- | --- | --- |
| 32 frames | 0.7 ms | 0.7 ms | 0.3 ms |
| 64 frames | 1.5 ms | 1.3 ms | 0.7 ms |
| 128 frames | 2.9 ms | 2.7 ms | 1.3 ms |
| 256 frames | 5.8 ms | 5.3 ms | 2.7 ms |
| 512 frames | 11.6 ms | 10.7 ms | 5.3 ms |

The round trip is about two buffers, plus the interface's converters and
driver, which add a fixed amount each way. Settings shows that amount under
**Warn when plugins delay playback**.

- **Smaller buffers** respond faster, but give the Mac less time to work
  through the chain. Too small and audio crackles.
- **Larger buffers** are safe with heavy chains and plugins, at the cost of
  feel.

Start at 128 frames. Try 64 once your chain is set; if it crackles, go back
up. When recording a part you have already played in, or mixing, a large
buffer costs nothing.

## What else adds latency

- **Pedals.** Most stock pedals add none. The drives and the amp add 22
  samples (0.46 ms at 48 kHz) for oversampling. Some plugins add much more,
  for lookahead or linear-phase filters; Settings warns past the limit you
  set there.
- **Two devices.** An input and an output on different devices run on
  separate clocks, and Cantina holds back a buffer between them. Use one
  interface for both.

Recorded takes are moved earlier by the latency the chain and the interface
added, so they land on the beat they were played on.

## Measure your round trip

The `measure_latency` tool plays clicks out of the interface, listens for
them coming back in, and reports the time between. It measures everything
you hear: converters, driver, macOS and Cantina itself.

1. Connect output 1 of the interface to input 1 with an instrument cable.
2. Set the input gain to about half, and turn off direct monitoring on the
   interface so the click does not take a shortcut.
3. From a checkout of the repository, run it once per buffer size, with the
   start of your interface's name:

   ```sh
   cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 64
   ```

   The passthrough example's `--list` names the devices
   (`cargo run -p cantina-engine --example passthrough -- --list`). Try 32,
   64, 128 and 256.

The tool times ten clicks and prints the median, the latency CoreAudio
reports for comparison, and a table row. A `warning: N underrun frames` line
means the buffer is too small for your Mac; keep the row and say so.

## Results

Measured round trips, interface to interface through a loopback cable:

| Interface | Rate | Buffer | Measured frames | Measured ms | Reported frames | Reported ms | Delta |
| --- | --- | --- | --- | --- | --- | --- | --- |

No results yet. Yours can be the first.

## Contribute a result

Run the tool at a few buffer sizes, then either
[open an issue](https://github.com/jbouder/cantina/issues/new) with the
printed rows, your Mac and your macOS version, or add the rows to this page
in a pull request (it is `docs/src/content/docs/guides/latency.md`).
