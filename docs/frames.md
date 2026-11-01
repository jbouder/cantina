# Frames

The engine streams one binary **frame** per displayed frame to the UI
over a Tauri channel. In M2 a frame carries the input and output meters
and the transport state; M6 added the pedal readouts (a tuner's pitch, a
compressor's gain reduction); in M8 the same frame carries the analysis
stream (spectrum, waveform, beat) for the stage. The encoder is
`src-tauri/src/frame.rs`, the decoder `ui/src/lib/frame.ts`, and both have
tests pinned to the layout below.

## Layout

All fields are little-endian. The header is 48 bytes, a multiple of four,
so the UI views the body as `Float32Array`s directly on the buffer.

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | bytes | magic `CNTF` |
| 4 | 2 | u16 | version, currently 2 |
| 6 | 2 | u16 | flags: bit 0 playing, bit 1 recording, bit 2 beat fields valid |
| 8 | 4 | u32 | sequence, wraps |
| 12 | 4 | u32 | frames dropped so far (see below) |
| 16 | 8 | f64 | timeline position in seconds |
| 24 | 2 | u16 | meter count `M`; each meter is a peak and an RMS |
| 26 | 2 | u16 | spectrum bins `S` (0 in M2) |
| 28 | 2 | u16 | waveform samples `W` (0 in M2) |
| 30 | 2 | u16 | readout values `R` (0 in version 1) |
| 32 | 4 | f32 | beat: tempo in BPM |
| 36 | 4 | f32 | beat: phase, 0 at the beat rising to 1 before the next |
| 40 | 4 | u32 | beat: bar |
| 44 | 4 | u32 | beat: beat within the bar |
| 48 | 4·2·M | f32 | meters, `[peak0, rms0, peak1, rms1, …]`, linear |
| 48 + 8M | 4·S | f32 | spectrum magnitudes, log-spaced bins |
| 48 + 8M + 4S | 4·W | f32 | recent waveform samples |
| 48 + 8M + 4S + 4W | 4·R | f32 | pedal readouts, see below |

Meter 0 is the input, meter 1 the master (post-fader), and meters 2
onward one per track in the project's track order, post-fader; a stereo
meter reports the louder of its two channels. `M` changes when tracks are
added or removed. A decoder that knows only the meters reads `M` and
ignores the rest; a newer frame with more sections is still readable
because the header length is fixed.

## Readouts

A pedal can measure things for its display: the tuner publishes the
pitch it hears and how clearly, the compressor its gain reduction, the
gate whether it is open. In `crates/dsp` such a processor returns a
`Readout` (named `f32` cells behind atomics, written from `process`)
and the shell keeps a clone of it next to the pedal's node. The pump
reads every readout in the running graph each frame and writes the
values flat into the `R` section, in track order and chain order, the
same order the status lists pedals in. Each pedal's `readouts` in the
status names its cells, so `readoutsByPedal` in `ui/src/lib/readouts.ts`
cuts the section up by pedal id: a tuner with two names takes the next
two values, a gain pedal with none takes nothing. A frame that does not
add up to the status (one sent across a chain edit) is shown as no
readouts rather than as the wrong pedal's values. See `docs/pedals.md`
for what each pedal publishes.

## Rate and dropping

The shell's pump wakes 60 times a second. Each subscriber (one per page)
has an *in-flight* flag: a frame is sent only when the flag is clear, and
the UI clears it with `meter_ack` from the animation frame in which it
drew the frame. While the flag is set the pump skips the frame and counts
it in `dropped`. So a UI that is busy, throttled or hidden receives a
fresh frame when it next draws, never a backlog, and the rate is capped at
the display rate from both sides. `frames_dropped` returns the count for
diagnostics.

## Transport

Tauri delivers a raw channel payload below a size threshold by
evaluating a script that builds a `Uint8Array` from a JSON array of the
bytes, and larger ones through a fetch from a queue. A meters-only frame
is 64 bytes, well inside the direct path. M8's analysis frame will be
several kilobytes and take the fetch path, which is why the ack scheme
rather than the channel itself bounds the queue.

## Cost

See `docs/meters.md` for the measured CPU use of the UI at idle with the
stream running.
