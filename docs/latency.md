# Round-trip latency

The number the whole project rests on. The plan's go/no-go is a round trip
of about 5 ms with a class-compliant interface at a small buffer.

## Method

`measure_latency` plays a four-sample click every 500 ms and records what
the input hears. The first 500 ms are silence and set the noise floor. For
each click, the delay is the distance from the sample the click was played
on to the first captured sample that rises above both eight times the noise
floor and 30% of that window's peak. The median over ten clicks is the
reported round trip. This includes everything the player hears: converters,
driver, CoreAudio, the cable and our own ring buffer.

CoreAudio also *reports* a latency: the device latency and safety offset in
each direction plus one buffer each way. The tool prints both so the gap is
visible.

## Procedure

1. Connect output 1 of the interface to input 1 with an instrument cable.
2. Set the input gain to about half. Mute monitoring on the interface so the
   direct path does not short-circuit the test.
3. Run, once per buffer size:

   ```sh
   cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 32
   cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 64
   cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 128
   cargo run --release -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 256
   ```

4. Paste the printed row into the table below.

A result with `warning: N underrun frames` means the buffer is too small for
this machine and the row should say so.

## Results

| Interface | Rate | Buffer | Measured frames | Measured ms | Reported frames | Reported ms | Delta |
| --- | --- | --- | --- | --- | --- | --- | --- |
| *not yet measured* | 48000 | 32 | | | | | |
| *not yet measured* | 48000 | 64 | | | | | |
| *not yet measured* | 48000 | 128 | | | | | |
| *not yet measured* | 48000 | 256 | | | | | |

Only the built-in microphone and speakers were present on the development
machine when the tool was written, and they are separate devices with
separate clocks, so no measurement has been taken yet. The first run on a
real interface fills this table in.

## Go/no-go

Pending the first measurement. The bar: a median round trip at 64 frames
of 6 ms or less, with no underruns over ten clicks, on a class-compliant
USB interface. If the measured number sits far above the reported one, the
two-unit path or the ring buffer is adding latency and the engine should
move to a single duplex unit on an aggregate device before M1.

## Compensation across tracks

A different latency from the round trip above: what plugins add. Every
processor reports the frames it delays the signal by, and a track's
chain delays it by the sum. Without correction a track with a lookahead
limiter plays late against one without, and a take recorded along to it
lands late on the timeline.

The engine levels the tracks (`crates/engine/src/delay.rs`): `plan`
takes every chain's latency and gives each track the difference to the
slowest, and a `Delay` node on each track's *playback* branch, between
the player and the bus, holds that many frames back. Live input joins
the bus after the delay, so monitoring is never compensated: the player
hears their own chain's latency and nothing else. The compensation is
recomputed whenever a chain changes and whenever a plugin reports a new
latency (the shell polls the plugin handles a few times a second). The
engine test plays an impulse on two tracks, one through a 10-frame
lookahead, and checks both land on the same output frame once
compensated and 10 frames apart when not.

Recorded takes are moved earlier when they are placed on the timeline,
by the compensation plus the input and output latency CoreAudio reports
for the open devices (device latency, safety offset and one buffer each
way). What the player heard was that late against the timeline, and what
they played in time with it came back that much later still. A take
that would start before zero is trimmed at the front instead.

The UI shows the compensation next to the engine's device line
(`comp 12.3 ms`) and on each pedal that adds latency, and warns when it
goes above a threshold set in Settings (20 ms by default, kept in
`settings.json`).
