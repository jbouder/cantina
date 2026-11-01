# Stock pedals

The pedals that ship with Cantina live in `crates/dsp`
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

## Drives: overdrive, distortion and fuzz

`overdrive`, `distortion`, `fuzz`. One core (`crates/dsp/src/drive.rs`)
with a **voicing** each: a highpass to keep the low end out of the
clipper, an optional mid boost, the drive gain, the waveshaper run
through the oversampler, a highpass to remove the offset a lopsided
curve leaves, a tone lowpass, and the level. Each has the same five
knobs: **Drive** (dB), **Tone** (500 to 8000 Hz lowpass), **Level**
(dB), **Curve** and **Oversampling**.

| Pedal | Drive | Curves | Before the clipper |
| --- | --- | --- | --- |
| Overdrive | 0–40 dB | Soft (`tanh`), Asymmetric, Hard | highpass 80 Hz |
| Distortion | 0–60 dB, +6 dB fixed | Hard, Soft, Fold | highpass 60 Hz, +6 dB at 900 Hz |
| Fuzz | 0–60 dB, +12 dB fixed | Silicon, Germanium, Octave | highpass 40 Hz |

The curves are in `crates/dsp/src/shaper.rs`. *Soft* rounds off
gently; *Hard* clips flat; *Asymmetric* is soft on top and harder and
lower underneath, like a mismatched diode pair, for even harmonics;
*Fold* folds the wave back on itself past full scale, so the harmonics
keep changing as a note dies; *Silicon* is huge gain into a near
square; *Germanium* is softer and lopsided with a bias that makes it
sputter as a note decays; *Octave* rectifies before clipping so the
fundamental doubles. A test checks every curve stays within ±1 and
that no two curves, or pedals, render the same.

The overdrive keeps the M0 pedal's first three parameters in the same
order, so a project saved before this carries its Drive, Tone and
Level over and gets the defaults for the rest.

### Oversampling

Anything nonlinear creates harmonics without end; the ones above half
the sample rate fold back as aliases that do not belong to the note.
The oversampler (`crates/dsp/src/oversample.rs`) runs the waveshaper at
2×, 4× or 8× the rate so those harmonics land where a lowpass can
remove them on the way back down. Each doubling is a halfband FIR
stage (31, 21 and 17 taps, Kaiser-windowed), zero-stuffing and
filtering up, filtering and dropping every other sample down. Halfband
filters have every other tap at zero, and in polyphase form the
interpolator's even outputs are a plain delay, so a stage costs about
a quarter of its length per sample.

The filters are linear phase, so the round trip is a fixed delay. The
stages are sized so it is a whole number of samples at every factor
(15 at 2×, 20 at 4×, 22 at 8×), and lower factors are padded to 22, so
every drive reports **22 samples of latency** (0.46 ms at 48 kHz)
whatever its setting: switching factors never moves the signal in time
and the chain's compensation never changes. The test clips a 5 kHz tone
hard and measures the energy at 3, 7 and 13 kHz, where its harmonics
fold back to at 48 kHz: 8× cuts them by 40 to 58 dB and leaves the
15 kHz harmonic that belongs. Off is available for the cheapest path.

### Cost

`drives_cost_is_measured_at_every_factor` in `crates/dsp/tests/golden.rs`
renders ten seconds through each drive and prints the cost (run it
with `--release -- --nocapture`). On an Apple Silicon MacBook Pro at
48 kHz with 64-frame blocks, per pedal and channel:

| Pedal | Off | 2× | 4× | 8× |
| --- | --- | --- | --- | --- |
| Overdrive | 0.15% | 0.23% | 0.29% | 0.34% |
| Distortion | 0.05% | 0.10% | 0.17% | 0.30% |
| Fuzz | 0.06% | 0.11% | 0.19% | 0.32% |

As a share of real time, so a stereo drive at 8× costs under 0.7% of
one core; a board of six is a few percent. The overdrive's `tanh` is
what makes it the dearest without oversampling. The default is 4×.

## Following the transport

Pedals that keep time (the modulation pedals' oscillators, the delay)
need the tempo and where the transport is. `Processor::set_transport`
hands every processor a `Transport` (tempo in BPM, the position in
quarter-note beats, whether the transport is playing) on the audio
thread before each block; the graph fills it from the tempo map at the
block's position, so a tempo change on the timeline reaches every pedal
on the next block. Most pedals ignore it.

## EQ

`eq`. A four-band parametric equaliser, each band a biquad on both
channels with a **type** (Peak, Low shelf, High shelf, Low cut, High
cut, Notch), **Freq** (20 Hz to 20 kHz; the fader moves in octaves),
**Gain** (±18 dB, ignored by the cuts and the notch) and **Q**. The
defaults are a low shelf at 100 Hz, peaks at 400 Hz and 1.6 kHz and a
high shelf at 5 kHz, all flat. Coefficients are recomputed on the next
block after a knob moves, keeping the filter state, so a sweep does
not click (tested). The designs are the audio EQ cookbook's, in
`crates/dsp/src/biquad.rs`; a test checks every type acts where it says.

## Chorus, flanger, phaser and tremolo

All four run an `Lfo` (`crates/dsp/src/lfo.rs`: sine, triangle,
square, saw or sample-and-hold random) at a **Rate** in hertz, or when
**Sync** is on at a **Note** value (whole note to 1/32, with dotted and
triplet values) from the transport's tempo. While the transport plays
the oscillator's phase is locked to the timeline, so a cycle starts on
the beat and a tempo change is followed on the next block; stopped, it
free-runs at the tempo. The stereo channels read the oscillator a
quarter cycle apart, except the tremolo, which pumps both together.

- **Chorus** (`chorus`) and **flanger** (`flanger`) are one pedal
  (`SweptDelay`) with different sweeps: a delay line read at a
  modulated fractional delay (12 to 20 ms for the chorus, 1 to 5 ms
  for the flanger), fed back (up to 70% and 95%) and mixed with the
  dry signal. **Depth** sets how far the delay swings, **Feedback** and
  **Mix** what you expect. Without a sweep the flanger is a comb
  filter, and the test looks for its notches in white noise.
- **Phaser** (`phaser`): four first-order allpass stages per channel,
  swept from 300 Hz up to four and a half octaves by the depth, with
  feedback around the cascade and a dry mix. Four stages put notches at
  0.41 and 2.41 times the corner frequency (where the shift reaches
  180° and 540°); the test finds them.
- **Tremolo** (`tremolo`): the level pumped by the oscillator with a
  **Shape** of sine, triangle or square. Each shape is read where it
  peaks, so a synced tremolo is loudest on the beat and dips between;
  the test syncs to an eighth note at 90 bpm and checks the dips land
  8000 samples after each beat.

## Delay

`delay` (`crates/dsp/src/echo.rs`). **Time** (1 to 2000 ms) or, with
**Sync** on, a **Note** value at the transport's tempo (clamped to two
seconds); **Feedback** (up to 100%, which still loses 2% a repeat so it
never runs away); **Tone**, a lowpass in the feedback path so repeats
darken as they go; **Mix**; and a **Mode**: *Mono* sums the input into
one line and sends it to both sides, *Stereo* keeps two lines, and
*Ping-pong* sends the input left and crosses each repeat over.

A change of time never slides the read head, which would bend the
pitch of what is in the line like a tape machine. Instead the head
crossfades from the old time to the new over 50 ms and takes the next
change once the fade is done, so a long knob drag, or a tempo ramp on
the timeline, follows in steps with no pitch artifacts. The test runs
a steady 440 Hz tone through a quarter-note delay while the tempo drops
from 120 to 60 bpm and checks the wet signal around the change is still
a clean 440 Hz (everything 20 Hz or more away is 30 dB down).

## Reverb

`reverb` (`crates/dsp/src/reverb.rs`). A feedback delay network: the
input, after **Pre-delay** (0 to 200 ms) and four Schroeder allpass
diffusers, feeds eight delay lines (30 to 73 ms at the default
**Size**; the knob scales them from half to one and a half times) that
feed each other through a Householder matrix, which keeps the energy
while scattering it so the echo density builds fast. A one-pole lowpass
in each loop (**Damping**) loses highs faster than lows, as air and
walls do, and each loop's gain is set from **Decay** (0.1 to 20 s) so
the tail falls 60 dB in that time (tested at 0.5 and 2 s). The sides
take the lines with different signs, so left and right differ.

Each line's length wobbles by a tenth of a millisecond at a slow rate
of its own, so no two paths line up for long: that is what keeps a
sustained chord from turning into a metallic ring. The test holds a G
major chord for two seconds, releases it, and sweeps the tail half a
second later in 24th-octave steps, skipping the chord's own partials:
no resonance may stand more than 15 dB over the median. A wider wobble
fails a different way, by spreading the chord audibly, which is how the
amount was chosen.
