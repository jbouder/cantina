---
title: Troubleshooting
description: "What to check when there is no sound, when audio crackles or drops out, when the wrong input is heard, or when macOS blocks the microphone."
sidebar:
  order: 2
---

Most audio problems come down to the route: which device, which input, and
whether macOS lets Cantina use it. Work down the section that matches.

If you have connected the [assistant](/guides/assistant/#tone-and-troubleshooting),
you can also ask it: *"why does it crackle?"* It reads the engine's own
numbers (buffer size, dropouts, levels, plugins that would not load) and
these guides before it answers.

## No sound

1. **Is the audio on?** The key left of **Record** reads **Audio on**, its
   light lit, while audio runs. Press it to turn the audio on.
2. **Does the IN meter move when you play?** If it does not, nothing reaches
   Cantina: check [the microphone permission](#macos-blocks-the-input), the
   **Input** in Settings, the cable, and the input gain on the interface.
3. **Is a track armed?** Your input is only heard through an armed track.
   Click the light beside the track's name; it turns red.
4. **Does the OUT meter move?** If it does but you hear nothing, check the
   **Output** in Settings, the interface's monitor or headphone level, and
   that the output is not the Mac's speakers while your headphones are on
   the interface.
5. **Is anything muted?** A track's mute key, a solo on another track, or the
   master's mute silence it. So does a gain or level knob at the bottom of
   its range.

## Crackles and dropouts

Clicks, crackles or short gaps mean the Mac did not finish a buffer in time.

- **Raise the buffer size** in Settings: 128 or 256 frames instead of 32 or
  64. It costs a few milliseconds of [latency](/guides/latency/) and is
  almost always enough.
- **Use one device for input and output.** Two separate devices, such as
  the interface in and the Mac's speakers out, run on two clocks that drift
  apart, and Cantina has to buffer between them.
- **Lighten the chain.** Plugins vary a lot in cost. Remove them one at a
  time to find a heavy one; ⌘Z puts each back. Among the stock pedals, a
  drive's **Oversampling** at 8× costs most; 4× is the default.
- **Quit other audio apps**, and anything busy in the background.

If the sample rate or buffer changes on its own, for example when the
interface is unplugged or Audio MIDI Setup is changed, Cantina rebuilds the
audio graph to match and says so in a notice. Notices show in the bottom
right corner of the window, over the work, and stay until you dismiss them. Takes in progress are closed
and kept.

## The wrong input

Cantina takes the signal from the **first input** of the device chosen as
**Input** in Settings. Plug your instrument into input 1 of the interface.
Choosing another input channel is not in Settings yet.

If **Input** says *System default*, it follows whatever macOS has selected,
which is often the built-in microphone. Choose your interface by name
instead. If you play through an amp and only want the Mac to play along,
choose **No input**.

## macOS blocks the input

macOS asks once whether Cantina may use the microphone, and it treats every
audio input that way, your interface included. If you said no, the IN meter
stays flat.

1. Open **System Settings → Privacy & Security → Microphone**.
2. Turn on **Cantina**.
3. Turn the audio off and on again with the **Audio** key, or reopen Cantina.

If Cantina is not in the list, start the engine once so macOS asks again.

## Feedback

A howl that builds by itself is the output reaching the input. It happens
with the built-in microphone and speakers together. Use headphones, or turn
the track's gain down before arming it.

## Still stuck

Ask the [assistant](/guides/assistant/#tone-and-troubleshooting), if you
use it: it can read what the engine is reporting right now.

[Open an issue](https://github.com/jbouder/cantina/issues/new) with your
macOS version, your interface, the sample rate and buffer size from Settings,
and what you tried.
