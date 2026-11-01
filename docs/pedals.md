# Stock pedals

The pedals that ship with Cantina live in `crates/dsp`, one module each,
as `Processor`s (see the trait in `crates/dsp/src/processor.rs`). This
page is what each one does, what its knobs mean, and the design choices
behind it. Every pedal has golden-file tests (`crates/dsp/tests/golden`)
that pin its output over an impulse, a sine, a sweep and a square burst,
plus its own tests for the behaviour that matters.

## Parameters and readouts

Knobs are `ParamInfo`s with a unit. Besides decibels, hertz, seconds and
percent, a parameter can be in **milliseconds**, a **toggle** (0 or 1,
shown as a switch) or a **choice** from a fixed list (the value is the
index, shown as a drop-down). The shell sends the names of a choice's
entries with the status; the UI never needs to know a pedal's list.

Some pedals measure things for their display. A processor can return a
`Readout`: named `f32` cells behind atomics that it writes from
`process` and that the shell reads at display rate and streams to the
UI in the same frame as the meters (`docs/frames.md`). The UI draws each
pedal's readout for its kind (`ui/src/components/PedalReadout.tsx`).

## Tuner

`tuner`. A chromatic tuner. Audio passes through untouched, or is muted
for silent tuning. Parameters: **Reference** (A4, 415 to 466 Hz, 440)
and **Mute**. Readouts: **Pitch** in hertz (0 when nothing periodic is
heard) and **Clarity** (0 to 1). The UI turns the pitch into a note
name, an octave, and cents against the reference, and lights a strip
from 50 cents flat to 50 cents sharp.

The detector is the McLeod pitch method (`crates/dsp/src/pitch.rs`): the
normalised square difference function of a window scores every
candidate period by how well the signal matches itself one period
later, and the first peak that comes within 93% of the highest is the
period, which keeps a bass note from reading an octave up when its
second harmonic is stronger than its fundamental. The peak is located
between samples with a parabola, then refined by finding the same peak
two, four, eight… periods out and dividing: an error of a fraction of a
sample at lag `m·τ` is `m` times smaller in `τ`. That is what gets a
high note, whose period is only a handful of samples, to within a
cent; the tests check every string's open note and the top of the neck
on pure tones, on tones with six decaying harmonics, at 44.1, 48 and
96 kHz, and with 32 to 1000-frame blocks.

The analysis runs on a copy of the input decimated to about 12 kHz
(fourth-order Butterworth anti-aliasing, then every fourth sample at
48 kHz), on a window of 1024 samples (85 ms, two periods of a low B)
every 256 samples (21 ms). The NSDF costs 512 lags of up to 1024
multiplies, far too much for one audio callback, so it is spread: each
block computes one lag per input frame, and the result is published
when the last lag is done, about 10 ms after the window closed. Every
block pays the same small amount. Silence (RMS under −80 dBFS) is not
analysed and reads as no pitch.

## Gate

`gate`. A noise gate. Parameters: **Threshold** (−80 to 0 dB),
**Attack** (0.1 to 100 ms), **Hold** (0 to 1000 ms), **Release** (1 to
2000 ms). Readout: **Open**.

The level is a fast peak follower (0.5 ms up, 20 ms down) on the
loudest channel, so a stereo pair opens and closes together. The gate
opens when the level reaches the threshold and closes only when it
falls 6 dB below it *and* has stayed there for the hold time; a note
hovering around the threshold, as a dying note with vibrato does, is
held open rather than chopped. The test plays such a note (2 dB of
wobble at 6 Hz around the threshold, slowly dying) and counts state
changes: one open, at most one close. The gain eases between 0 and 1
with the attack and release times so neither edge clicks.

## Compressor

`compressor`. A feed-forward compressor with a soft knee. Parameters:
**Threshold** (−60 to 0 dB), **Ratio** (1 to 20), **Attack** (0.1 to
200 ms), **Release** (10 to 2000 ms), **Knee** (0 to 24 dB), **Makeup**
(0 to 24 dB). Readout: **Reduction** in dB (zero or negative; the UI
draws it as a gain reduction meter).

The level is the loudest channel's peak through a detector with an
instant attack and a 20 ms release, so a steady note reads as a steady
level rather than a rectified wave that would ripple the gain at the
note's frequency. The gain computer draws the static curve in decibels
(nothing below the knee, the full ratio above it, a parabola between);
the reduction it asks for is smoothed with the attack and release
times and applied with the makeup. Both channels get the same gain.
The tests pin the curve (10 dB over at 4:1 is 7.5 dB of reduction, the
knee is continuous), the settled output of a steady tone, that nothing
below the threshold is touched, and that the attack reaches 90% in a
few blocks while the release takes hundreds.
