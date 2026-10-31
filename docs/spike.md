# M0 spike: can this work?

The spike asked two questions. Can CoreAudio give a guitar player a round
trip they cannot feel, and can one `Processor` trait hold stock pedals,
Audio Units and CLAP plugins without the chain caring which is which?
This is what was built, what was learned, and what is still open.

## What was built

| Issue | Result |
| --- | --- |
| #2 Workspace and passthrough | `crates/dsp` and `crates/engine`. A duplex CoreAudio stream: one HAL unit when input and output are the same device, two units with one block held back otherwise. Audio crosses between callbacks on a lock-free ring; nothing in the callbacks allocates, locks, blocks or logs. |
| #3 Latency measurement | A click train over a loopback cable with onset detection against the noise floor, next to the latency CoreAudio claims. Method and table in [latency.md](latency.md). |
| #4 Processor trait | `Processor`, `Buffer`, `Chain`, `Smoother`, `Biquad`, stock `Gain` and `Overdrive`. Parameter changes reach the audio thread over an `rtrb` queue. |
| #5 Audio Units | `AudioUnitProcessor` over the v2 API: enumerate, find by `aufx:dely:appl` or name, parameters, latency, render. Tested against Apple's AUDelay. |
| #6 CLAP | `ClapProcessor` over `clack-host`, plus a real CLAP plugin in the workspace as a test fixture. Parameters as events, latency and state extensions. |

Everything is exercised by `cargo test` on a machine with no audio
interface, including both plugin formats.

## Latency: not yet answered

The development machine had only the built-in microphone and speakers,
which are separate devices on separate clocks, and unattended runs block
on the microphone permission prompt. No round trip has been measured.
The tool is ready and the bar is written down in
[latency.md](latency.md): a median of 6 ms or less at 64 frames on a
class-compliant USB interface, with no underruns over ten clicks.

What CoreAudio will report for such an interface at 64 frames is roughly
`in_safety + in_latency + 64 + 64 + out_latency + out_safety`, which on
typical USB interfaces lands between 4 and 7 ms at 48 kHz. The measured
figure is the one that counts; if it sits well above the reported one, the
first suspect is the two-unit path and the fix is an aggregate device so
one duplex unit carries both directions.

**Go/no-go on latency: deferred to the first run on hardware.** Nothing
found in the spike suggests it will fail; the single-unit duplex path adds
no buffering of its own.

## The trait: answered, with changes

`Processor` survived both plugin formats. Three things changed on contact
with them.

1. **Parameter names are `Cow<'static, str>`.** Stock tables stay static;
   plugins report names at load time. Forced by AU (#5).
2. **`state` takes `&mut self`.** Both plugin APIs save state through
   mutable handles. The alternative was an invalid cast. Forced by CLAP (#6).
3. **`prepare` snaps smoothers to their targets.** A parameter set before
   the stream starts should not fade in. Found by the chain test (#4).

And one thing did not change, but should be decided before M4:

4. **One object, two threads.** `Processor` is a single object that moves
   wholesale between the UI thread and the audio thread. CLAP, and clack's
   types, split a plugin into a main-thread half and an audio-thread half.
   `ClapProcessor` is `Send` by promise, not by type: main-thread
   operations (`prepare`, `state`, parameter info) must run while the
   processor is outside a running graph. That is how the graph swap in the
   plan already works, so it holds today, but nothing enforces it. M4
   should either split `Processor` into a main-thread handle and an
   audio-thread processor, or have the engine keep the main-thread half on
   the UI thread and talk to the audio half through queues. The second is
   closer to how the engine already moves parameters and is the
   recommendation. *M4 took it: see "the handle" in
   [hosting.md](hosting.md).*

Other findings:

- **Channel layouts are a plugin property, not a buffer property.** Stock
  pedals process whatever channel count arrives. Plugins fix their layout
  at initialisation. Both wrappers take a channel count at construction
  and duplicate or drop channels when the buffer disagrees. M4's proper
  answer is a channel layout on the chain that processors are prepared
  for.
- **CLAP parameters carry no unit.** The wrapper guesses from the name.
  Good enough for a knob label, not for value formatting; M4 should use
  the plugin's `value_to_text`.
- **Both plugin wrappers bypass on failure** rather than stopping audio,
  and count errors. The engine needs a log channel off the audio thread
  before this is useful to a user.
- **Audio Unit state** is still the parameter blob. The real thing is
  `kAudioUnitProperty_ClassInfo` as a property list; that is #23.

## Real-time rules, as practised

The rules from the plan held up and were easy to keep with three habits:

- Allocate everything in `prepare` or in the constructor; `process` only
  indexes into it. Scratch buffers, `AudioBufferList` storage, CLAP
  `AudioPorts` and event buffers are all preallocated with a capacity and
  cleared, never dropped and rebuilt, on the audio thread.
- Cross threads with `rtrb` rings and atomics only. Audio, parameter
  changes and counters all travel that way.
- Treat a failure in the callback as a counter, not a return path. The
  stream never panics or logs from the callback.

## Verdict

Trait: **go**, with the thread-split decision owed to M4. Latency:
**pending hardware**, with a ready tool and a written bar. M1 can start on
the graph runtime; its design should assume the engine owns a main-thread
handle per processor from the outset so the decision in point 4 does not
have to be retrofitted.
