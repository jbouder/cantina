# Meter stream cost

What the binary frame stream (`docs/frames.md`) costs the UI at idle, with
the engine idle and 60 frames a second flowing. Measured with
`scripts/measure-ui-cpu.sh`, which launches the release build with
`CANTINA_SYNTHETIC_METERS=1` (the shell pumps frames with no audio stream
behind them) and samples the shell process and its WebKit content process
with `top` for twenty one-second intervals.

## Results

MacBook Pro, macOS 26, Tauri 2.12, release build, 2026-10-31.

| Window | Shell | Web content | Notes |
| --- | --- | --- | --- |
| Hidden or fully covered | 0.0% | 0.0% | Measured. The web view stops its animation frames, so no acknowledgement goes back and the pump sends nothing; 294 of 300 frames counted as dropped over five seconds, the rest were the first frame and the stream's start. |
| Visible | pending | pending | Needs a visible window, which a build launched from an unattended session does not get. Run `scripts/measure-ui-cpu.sh` with the app's window on screen and fill this row in. |

The hidden case is the point of the acknowledgement scheme: a UI that is
not drawing costs nothing, and when it is drawing again it gets a fresh
frame, not a backlog.

## How to run it

```sh
npm run tauri build
scripts/measure-ui-cpu.sh 20
```

Leave the window on screen and uncovered while it samples; cover it to
measure the hidden case. Launching the binary directly from a terminal
in the background, instead of through `open`, gives a hidden window.
