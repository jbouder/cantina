# Drums

Beats in Cantina are a **kit** (sounds) played by a **sampler** (the
`Processor` that makes the sound) under a **sequencer** (what decides
when). This page covers the kit format and the sampler; the sequencer,
patterns and pattern clips are documented as M5 adds them.

## Kit format

A kit is a folder with a `kit.json` and its samples beside it:

```text
kits/
  acoustic/
    kit.json
    kick-soft.wav
    kick-hard.wav
    snare.wav
    hh-closed.wav
    hh-open.wav
```

The folder's name is the kit's id; a project refers to a kit by that id.
`kit.json` is:

```json
{
  "version": 1,
  "name": "Acoustic",
  "pads": [
    {
      "id": "kick",
      "name": "Kick",
      "layers": [
        { "file": "kick-soft.wav", "velocity": [0, 0.6] },
        { "file": "kick-hard.wav", "velocity": [0.6, 1] }
      ]
    },
    { "id": "snare", "name": "Snare", "pan": -0.1, "layers": [{ "file": "snare.wav" }] },
    { "id": "hh-closed", "name": "Closed hat", "choke": "hat", "pan": 0.3,
      "layers": [{ "file": "hh-closed.wav" }] },
    { "id": "hh-open", "name": "Open hat", "choke": "hat", "pan": 0.3, "gainDb": -3,
      "layers": [{ "file": "hh-open.wav" }] }
  ]
}
```

- **`pads`**: one to 64, each with a unique `id` that patterns refer to
  (`kick`, `snare`, `hh-closed`… the stock kits share these names so a
  pattern moves between kits), an optional display `name` (the id when
  missing), `gainDb` (default 0), `pan` from −1 to 1 (default 0) and an
  optional `choke` group.
- **`layers`**: one to 16 per pad, each a `file` next to the kit with the
  `velocity` range it answers, inclusive, inside 0 to 1 (the whole range
  when missing), and its own `gainDb`. A hit picks the first layer whose
  range holds its velocity, or the nearest when none does, so a kit with
  a soft and a hard kick plays the right one.
- **Choke groups**: hitting a pad stops every voice still ringing in
  the same group, the closed hat silencing the open one. The cut is a
  3 ms fade, never a click. Hitting the same pad again chokes its own
  earlier hit too; a pad without a group overlaps with itself.
- **Samples** are WAV, mono or stereo, 16 or 24-bit integer or 32-bit
  float, at any rate. A sample file name is a plain name next to the
  kit (`..` and absolute paths are refused), so a kit never reads
  outside its folder.

The format lives in `crates/dsp/src/kit.rs` (`Kit`, validated by
`Kit::from_json` with a message that names the pad and the rule), and the
loader in `crates/engine/src/kit.rs` (`kit::load` reads a folder,
`kit::discover` lists the kits under a set of folders, user kits
first so one can override a stock kit of the same id).

## The sampler

`cantina_dsp::Sampler` is a `Processor` built from a `LoadedKit` (the
kit with its samples in memory). It is polyphonic, 32 voices, with a
fixed pool so nothing is allocated on the audio thread; a 33rd hit steals
the oldest voice. Hits are queued with `trigger(pad, velocity, frame)`
and fired by `process` at exactly that frame of the block, which is what
makes a sequenced beat sample-accurate: the sequencer works out the frame
each step lands on from the tempo map and the sampler does the rest.

- **Velocity** is gain, linearly: a hit at 0.5 is half the amplitude of
  one at 1. Zero is not a hit.
- **Pan** is constant power. A mono sample sits at −3 dB a side when
  centred; a stereo sample plays at unity when centred and the pan
  becomes a balance.
- **Rate**: a sample recorded at 44.1 kHz plays at the right pitch in a
  48 kHz project; the voice steps through it at the ratio of the rates
  with linear interpolation. At equal rates playback is bit-exact.
- **Level**: the one parameter, in dB, smoothed like every other.
- A one-channel buffer gets the mono mix; a stereo one gets the pan.

`crates/dsp/tests/sampler.rs` renders a bar of a beat on a kit made of
arithmetic (decaying sines and seeded noise, so no binary fixtures are
needed) in 64-frame blocks and compares it to
`tests/golden/sampler_beat.wav`; a second test checks that every hit
starts on its frame, at the sample's first value times the velocity, the
pad's gain and the pan, with silence before the first. Regenerate the
reference after an intended change with
`UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test sampler`.
