---
title: "Pedals"
description: "Build a chain from the stock pedals, plugins and your own designs, and what every stock pedal's knobs do."
---

Each audio track has a pedalboard: its pedals in order, from the input on
the left to the mixer on the right. Select a track to see its board under
the timeline.

## The chain

For a whole chain at once, pick a [rig](/guides/rigs/): **Rigs** above the
pedalboard lists ready-made chains by genre, auditions one on your live
input against your own chain, and applies it in one undoable step. **Save
chain as rig…** there keeps a chain you built for other projects.

- **Add** a pedal with the dashed **Add a pedal** slot at the end. Type in
  the picker to filter it.
- **Reorder** by dragging a pedal by its header, the row with its name,
  onto another place in the chain. A box marks where it will land; Esc
  puts it back. Or focus the name and press ← or →.
- **Turn off** a pedal with the light at the left of its name. Off, the
  signal goes round the pedal and it adds no latency; its knobs dim but
  keep their settings. Click the light again to turn it back on. On and off
  are saved with the project and can be undone.
- **Remove** a pedal with the small **×** at the right of its name.
- **Change** a knob by dragging its fader, or from the keyboard. Every
  change is heard at once, and every change can be undone.

The badge on a card says where a pedal comes from: **stock** for one built
into Cantina, **json** for a stock pedal written in the
[pedal format](/reference/pedal-format/), and **yours** for one you
designed. A pedal that delays the signal says by how much under its name.

## Plugins

The picker lists the Audio Unit effects and CLAP plugins installed on your
Mac. The first time it opens, Cantina checks each one in a separate process,
so a plugin that crashes is listed as failed instead of taking Cantina with
it. **Rescan** looks for plugins installed since; **Full rescan** checks
every plugin again.

A plugin with its own window has an **Edit…** key on its card, which opens
the window beside Cantina's. Its settings
are saved in the project and restored when the project opens.

## Stock pedals

### Tuner

A chromatic tuner. It shows the note, and how many cents flat or sharp you
are on a strip from −50 to +50. **Reference** sets the pitch of A (415 to
466 Hz, 440 by default). **Mute** silences the output while you tune. Put it
first in the chain.

### Gate

Silences the hum and hiss between notes. **Threshold** is the level a note
has to reach to open the gate. **Attack** is how fast it opens, **Hold** how
long it stays open after the level falls, and **Release** how fast it then
closes. A note that wavers around the threshold is held open rather than
chopped. The light shows when the gate is open.

### Compressor

Evens out the level: anything louder than **Threshold** is turned down by the
**Ratio**, 4 meaning 4 dB over becomes 1 dB over. **Attack** and **Release**
set how fast it reacts and lets go, **Knee** how gently it starts, and
**Makeup** brings the level back up. The meter shows how much it is turning
down.

### Overdrive, Distortion and Fuzz

Three drives that share their knobs:

- **Drive**: how hard the signal is pushed.
- **Tone**: a filter on the top end, from 500 Hz (dark) to 8 kHz (bright).
- **Level**: the output volume.
- **Curve**: the character of the clipping.
- **Oversampling**: the quality of the clipping, from off to 8×. Higher is
  cleaner on high notes and costs more processing; 4× is the default.
- **Blend**: how much of the drive you hear over your clean signal. At
  100%, the default, it is all drive. Lower, the clean signal comes
  through underneath, untouched by the pedal's filters, which is how bass
  players drive a bass and keep its low end whole. Set **Level** so the
  drive is about as loud as the clean signal, then blend to taste.

| Pedal | Sound | Curves |
| --- | --- | --- |
| Overdrive | Warm, touch-sensitive breakup | Soft, Asymmetric, Hard |
| Distortion | More gain and a mid push | Hard, Soft, Fold |
| Fuzz | Huge gain into a square wave | Silicon, Germanium, Octave |

*Asymmetric* clips the top and bottom of the wave differently, like a
mismatched pair of diodes. *Fold* folds the wave back on itself, so the tone
keeps changing as a note dies. *Germanium* is softer and sputters as a note
decays. *Octave* adds the octave above.

### EQ

Four bands, each with a **type** (peak, low shelf, high shelf, low cut, high
cut or notch), a **frequency**, a **gain** of ±18 dB and a **Q** for how
narrow it is. All four start flat.

### Chorus, Flanger, Phaser and Tremolo

The modulation pedals. Each moves at a **Rate** in hertz, or with **Sync** on
at a **Note** value locked to the project's tempo: whole notes to 1/32, with
dotted and triplet values. Synced, a cycle starts on the beat while the
transport plays.

- **Chorus** thickens the sound with a slightly detuned copy.
- **Flanger** is the jet-plane sweep of a very short, moving delay.
- **Phaser** sweeps notches up and down the spectrum.
- **Tremolo** pumps the volume, with a **Shape** of sine, triangle or square.

**Depth** sets how far each one sweeps, **Feedback** how strong the effect
is, and **Mix** how much of it you hear.

### Pitch

An octaver and a pitch shifter. It follows one line at a time, as an
octaver does: single notes track cleanly, chords come out rougher.
Four voices, each with its own level:

- **Two down**: two octaves below, for sub bass under a riff.
- **Octave down**: an octave below, the synth-like bass line or the
  doubled drop-tuned riff. On at 100% by default.
- **Octave up**: an octave above. All wet in front of a long reverb,
  it is the shimmer of ambient and shoegaze pads.
- **Shift**: the level of a free voice **Semitones** away, up to an
  octave either way: a fifth (7) for power-chord harmonies, −5 for a
  fourth below. Turning **Semitones** glides between intervals.

**Mix** blends the voices over your dry signal. The pedal delays the
dry signal by about 16 ms to keep it in step with the voices, so it
adds that much to what you hear while you play; the recorded tracks are
kept in line for you.

### Delay

Echoes. **Time** sets the gap, up to two seconds, or with **Sync** on a
**Note** value at the project's tempo. **Feedback** is how many repeats.
**Tone** darkens each repeat a little more. **Mix** sets their level. The
**Mode** is *Mono*, *Stereo*, or *Ping-pong*, which bounces each repeat from
side to side. Changing the time never bends the pitch of the repeats.

### Reverb

A room around the sound. **Size** sets how big, **Decay** how long the tail
lasts (0.1 to 20 seconds), **Damping** how fast the highs fade, and
**Pre-delay** the gap before the reverb starts. **Mix** sets the level.

### Amp

A guitar amp's preamp and tone stack: **Gain**, then **Bass**, **Mid**,
**Treble** and **Presence**, each ±12 dB, and **Level**. Follow it with a
cabinet.

### Cabinet

The sound of a speaker cabinet, from an impulse response. Pick one from the
card. Four come with Cantina: a 1×12 open back, a 2×12 closed back, a 4×12
and a 4×10 for bass. To use your own, put `.wav` impulse responses in
`~/Music/Cantina/IRs` and press **Rescan** on the card. **Level** sets the
volume.

### Gain

A plain volume knob, in dB.

## Your own pedals

**New design…** in the picker builds a pedal from parts while it plays; see
[Designing pedals](/guides/designer/). Your pedals appear in the picker under
*Your pedals*.
