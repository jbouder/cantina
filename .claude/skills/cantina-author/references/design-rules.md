# Design rules

What the stock pedals, kits and patterns do, so what you write sounds like
it belongs. `cantina-cli render` gives you the numbers to check each rule.

## Gain staging

- The input is a guitar DI: peaks at -6 dBFS, RMS about -23 dBFS.
  `render pedal --di` plays exactly that.
- **A pedal at its defaults should come out within a few dB of what went
  in**, peak and RMS. A drive raises gain inside the pedal and takes it
  back with a `level` gain at the end; the stock drives default `level` to
  -6 to -8 dB.
- The peak never goes over 0 dBFS at the defaults. `render` reports
  `clipped`; anything but `no` is a bug, not character.
- Give every pedal that changes level a `level` (or `volume`) parameter in
  dB, about -24 to +6, so the player can match it bypassed and on.

## Nonlinearities

- Every `waveshaper` is oversampled (`oversample` 2, 4 or 8). 4 is the
  stock default; 8 for heavy fuzz, 2 if latency matters. Without it, high
  notes alias into inharmonic fizz.
- Put a `biquad` high-pass at about 20 Hz after any `asymmetric`,
  `germanium` or `octave` curve. Asymmetric curves make DC; `render`
  reports it as `dc`, which should be under 0.001.
- Cut lows *before* the clipper (a high-pass at 80 to 150 Hz) for a tight
  drive, and shape highs *after* it (a low-pass tone control, 2 to 6 kHz).
  Clipping a full-range signal sounds muddy.
- Put the gain into the clipper with a `gain` node bound to a `drive`
  parameter in dB, 0 to 40.

## Parameters

- Ranges in real units, with the unit named: `dB` for levels, `Hz` for
  frequencies, `ms` for times, `%` for amounts, choices for modes. Never a
  bare 0 to 1 knob when a unit exists.
- Bind a frequency with `"log": true` and a `map`, so the knob's travel is
  even to the ear: `{ "param": "tone", "map": [500, 8000], "log": true }`.
- `%` parameters run 0 to 100 and are mapped to the field's 0 to 1:
  `{ "param": "mix", "map": [0, 1] }`.
- Defaults are where the pedal sounds good on first insert, not at an
  extreme.
- Give parameter-bound fields a `smooth` of about 0.01 s (gains, filters) so
  turning a knob does not zipper.
- Ids are short and lower case (`drive`, `tone`, `level`, `rate`, `depth`,
  `mix`, `time`, `repeats`); labels are the words on the pedal.

## Modulation

- An `lfo` is never wired with an edge: bind fields to it with
  `{ "node": "lfo", "map": [lo, hi] }`. Only fields marked modulatable in
  the catalog take a node binding (`gain` `db`/`linear`, `biquad` `freq`,
  `delay` `time`, `allpass` `freq`).
- Use `"unipolar": true` on an `lfo` whose map ends should be the wave's
  ends. Add `"spread": 0.25` for a stereo chorus.
- Offer tempo sync where it makes sense (tremolo, delay-time modulation):
  a `sync` toggle and a `note` choice bound to the `lfo`'s `sync` and
  `note`, as the stock tremolo does.
- Chorus: a `delay` with `time` swept over about 8 to 20 ms, `max` 20.
  Flanger: 0.5 to 5 ms with feedback. Vibrato: chorus with `mix` 1.

## Delay and parallel paths

- Set `delay`'s `max` to the longest time any binding can ask for; it is
  the memory reserved.
- For a wet/dry blend with processing on the wet side only, `split` the
  input, process one branch, and sum both in a `mix` node; scale the wet
  branch with a `gain` bound to a mix parameter.
- Keep `feedback` at or below 0.9 at the top of its knob, or the repeats
  run away.

## Latency

- Only oversampled `waveshaper`s add latency: 22 samples each, whatever
  the factor. `check` and `render` report the total.
- If one branch of a parallel path has a waveshaper and the other does not,
  put a `delay` with `time` of 22 samples (0.458 ms at 48 kHz, `mix` 1) on
  the dry branch, or the blend comb-filters.

## Kits and recipes

- Use the shared pad ids so patterns move between kits: `kick`, `snare`,
  `hh-closed`, `hh-open`, `tom-hi`, `tom-lo`, plus `clap`, `crash`, `ride`,
  `hh-pedal` and whatever else the kit needs.
- Put the hats in one choke group, `"choke": "hat"`, so the closed hat
  cuts the open one.
- Two velocity layers for anything that is played dynamically; one is
  fine for a clap or a cowbell.
- Hats and percussion a few dB under the kick and snare (`gainDb` -6 to
  -9), panned a little (`pan` ±0.2 to 0.3) as from a drummer's seat.
- `synth-kit` normalises every sample, so balance the kit with `gainDb`,
  not with `decay` or `noise`.

## Patterns

- `stepsPerBeat` 4 (sixteenths) unless the feel needs triplets (3 or 6).
  `length` is steps, so a bar of 4/4 in sixteenths is 16, two bars 32.
- Every row holds exactly `length` velocities. Use 1 for accents, 0.6 to
  0.8 for normal hits, 0.3 to 0.5 for ghost notes; hats alternate strong
  and weak.
- Only include rows that have hits.
- A pattern has no tempo: the project sets it. Render at the tempo the
  prompt names with `render pattern --bpm`.
- Swing 0 is straight; 0.1 to 0.3 is a light shuffle; 1 is full triplet.
