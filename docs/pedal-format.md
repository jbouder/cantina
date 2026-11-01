# Pedal format

A user-designed pedal is a JSON graph of DSP primitives. The format is
read, checked and compiled by `crates/dsp/src/pedal/` into a
`PedalProcessor` that implements `Processor` like every stock pedal, so
the chain, the mixer, latency compensation and the project file never
know the difference. Pedals built this way are real-time safe by
construction: the primitives are the ones the stock pedals already run
on, every buffer is reserved when the pedal is compiled and prepared,
and `process` allocates nothing (a test counts allocations through a
wrapping allocator to make sure).

```json
{
  "version": 1,
  "name": "Warm Drive",
  "params": [
    { "id": "drive", "label": "Drive", "unit": "dB", "min": 0, "max": 30, "default": 12 }
  ],
  "nodes": [
    { "id": "in", "kind": "input" },
    { "id": "pre", "kind": "biquad", "type": "highpass", "freq": 80 },
    { "id": "gain", "kind": "gain", "db": { "param": "drive" } },
    { "id": "clip", "kind": "waveshaper", "curve": "soft", "oversample": 4 },
    { "id": "tone", "kind": "biquad", "type": "lowpass", "freq": 4500 },
    { "id": "out", "kind": "output" }
  ],
  "edges": [["in", "pre"], ["pre", "gain"], ["gain", "clip"], ["clip", "tone"], ["tone", "out"]]
}
```

## The file

- **`version`** is 1. A newer version is refused with a message saying
  which version the build reads.
- **`name`** is what the chain shows.
- **`params`** are the knobs. Each has an `id` that bindings refer to,
  a `label` (the id when missing), a `unit` (`dB`, `Hz`, `s`, `ms`, `%`
  or none), `min`, `max` and `default`. A parameter with **`choices`**
  is a pick from that list: its value is the index and its range is set
  from the list. A parameter with **`toggle`** is a switch, 0 or 1. At
  most 32.
- **`nodes`** are the graph. Each has an `id`, a `kind` from the table
  below, the kind's fields, and optionally a `pos` (`[x, y]`), which is
  where the designer drew it. At most 64. There is exactly one `input`
  and one `output`.
- **`edges`** are `[from, to]` pairs of node ids. Audio flows along
  them. A node's output may feed any number of edges; every kind but
  `mix` takes one incoming edge.

Numbers are written without a fraction when they have none (`"freq":
80`), and a pedal round-trips through its JSON unchanged.

## Fields and bindings

A field is a number, a name from the kind's list, or a switch. Instead
of a constant, a field can be **bound**:

| Binding | Meaning |
| --- | --- |
| `{ "param": "drive" }` | The parameter's value as it is. The parameter's range must fit the field's. |
| `{ "param": "drive", "map": [6, 46] }` | `6` at the parameter's `min`, `46` at its `max`, linear between. |
| `{ "param": "tone", "map": [500, 8000], "log": true }` | The same, interpolated exponentially: right for frequencies. |
| `{ "param": "curve", "values": ["soft", "hard"] }` | For a choice or toggle parameter: the field's value for each index. Numbers or names, as the field takes. |
| `{ "node": "lfo", "map": [12, 20] }` | Follow another node's signal sample by sample: `12` where the signal is 0, `20` where it is 1. `log` works here too. Only fields marked modulatable take a signal. |

A parameter binding is read once per block. Fields marked *smoothed*
ease toward a changed parameter over the node's **`smooth`** time in
seconds (each kind has its own default; 0 applies the change on the
next block). A `gain` in decibels eases in the linear domain, as the
stock gain does, so a fade is even.

A signal binding is read every sample. An `lfo` is the usual source,
but any node's output will do, which is how a ring modulator or an
envelope-shaped filter can be built. An `lfo` carries no audio: it may
only be bound, not wired with an edge.

## Kinds

| Kind | Fields | What it does |
| --- | --- | --- |
| `input` | | The signal coming into the pedal. |
| `output` | | The signal leaving. |
| `gain` | `db` (−96 to 96) **or** `linear` (0 to 16); `smooth` 0.005 | Scales the signal. Both fields are modulatable and smoothed. |
| `biquad` | `type` (`lowpass`, `highpass`, `peak`, `lowshelf`, `highshelf`, `notch`); `freq` 20 to 20 000 Hz; `gain` ±24 dB (peak and shelves); `q` 0.1 to 10 (0.7071); `smooth` 0 | The cookbook filters from `biquad.rs`. The coefficients are recomputed when a field changes, keeping the filter state so a sweep does not click. `freq` is modulatable: a swept filter is redesigned every sample. |
| `waveshaper` | `curve` (`soft`, `hard`, `asymmetric`, `fold`, `silicon`, `germanium`, `octave`); `oversample` 1, 2, 4 or 8 | The curves from `shaper.rs` through the oversampler from `oversample.rs`. At 1 the shaper runs plain with no latency; anything above adds **22 samples**, the same at every factor, so a bound factor can switch without moving the signal in time. |
| `delay` | `time` 0.1 to 10 000 ms; `feedback` 0 to 0.99; `mix` 0 to 1 (1); `max` ms (2000); `smooth` 0.02 | A delay line with feedback and a dry/wet blend. `time` is modulatable; a change slides the read head, so a bound lfo makes a chorus or a flanger and a knob drag bends the pitch like tape (the stock delay pedal crossfades instead). The line is reserved for `max` at prepare. |
| `lfo` | `rate` Hz; `shape` (`sine`, `triangle`, `square`, `saw`, `random`); `sync`; `note` (`1/1` to `1/32`, dotted and triplet); `depth` 0 to 1 (smoothed); `phase` 0 to 1; `spread` 0 to 1; `unipolar`; `smooth` 0.02 | The oscillator from `lfo.rs`, free-running or locked to the transport at a note value. Its output, scaled by `depth`, is what bindings read: −1 to 1, or 0 to 1 with `unipolar`, which lines a map's ends up with the wave's. `phase` offsets where the cycle is read; `spread` reads the right channel that much further on. |
| `allpass` | `stages` 1 to 16 (4); `freq` Hz; `feedback` 0 to 0.95; `mix` 0 to 1 (1); `smooth` 0.02 | A cascade of first-order allpasses with feedback around it, flat in level: mixed half and half with the dry signal it is a phaser, and `freq` is modulatable for the sweep. |
| `mix` | | Sums every incoming edge. Put gains in front to weight them. |
| `split` | | Passes the signal through, as a junction to fan out from. |

The catalog is `kinds()` in `crates/dsp/src/pedal/kinds.rs`, with each
field's range, default and summary; the designer builds its palette
and side panel from it rather than from a copy.

## Rules

Compiling checks everything and reports every problem at once, each
naming the node (and field) it is about, so an editor can show them
inline:

- one `input`, one `output`; node ids unique; kinds known; fields known
  to the kind, of the right type, in range;
- every edge between existing nodes, none into the `input` or an `lfo`,
  none out of the `output` or an `lfo`, none twice, and at most one
  into anything but a `mix`;
- every node but the sources has an input, and every node but the
  output feeds something (an edge or a binding);
- no cycles, through edges or bindings alike: the graph is ordered so
  everything a node reads comes before it;
- bindings name an existing parameter or node, a parameter's range fits
  the field or a map is given, `log` has a map, `values` match the
  field, and a signal is only bound to a modulatable field.

## Latency

A pedal reports the longest latency along any path from its input to
its output (the oversampled waveshapers are the only nodes with any).
Parallel branches are summed as they arrive, so a shaped branch beside
a dry one is 22 samples behind it; a designer who wants them aligned
puts a `delay` of 22 samples on the dry side.

## Channels

A pedal runs at the chain's channel count (mono or stereo). Every node
keeps state per channel; the `lfo`'s `spread` is the one place the
channels are treated differently on purpose.

## Equivalence with the stock pedals

The engines in `crates/dsp/src/pedal/compile.rs` do what the stock
pedals do, sample for sample: a `gain` is the stock `Gain`'s loop, a
`biquad` with `smooth` is the drives' tone control, an `lfo` bound to a
`delay` is the chorus, an `lfo` bound to an `allpass` is the phaser, an
`lfo` read at its trough and bound to a `gain` over `[1, 0]` is the
tremolo. A test renders a `gain` into a `biquad` and compares it with
the stock gain and filter bit for bit. That is what lets the stock
pedals be shipped in this format (the next step) and still pass their
golden files.
