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
