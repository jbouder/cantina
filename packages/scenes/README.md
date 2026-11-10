# @cantina/scenes

The visualizer scenes Cantina's stage draws and
[music-visualizer](https://github.com/jbouder/music-visualizer) plays:
9 WebGL 2 shader scenes, each with a classic Canvas 2D counterpart, and
13 low-resolution Canvas 2D scenes, plus the hidden intro. They moved here
from music-visualizer's `src/render/` so both apps keep one copy.

A scene never touches audio. Each draw it gets one `AudioFrame`: 1024
linear spectrum bins and 2048 waveform samples as bytes, the way a Web
Audio analyzer reports them, with RMS, bass, mid and treble levels and an
onset envelope worked out from them. Where the frame comes from is a
`FrameSource`:

- `EngineFrames`: push Cantina's `AnalysisFrame`s (128 log-spaced
  magnitudes, float samples) and it reads them out as `AudioFrame`s at
  the same heights an analyzer would give.
- `AnalyserFrames`: wraps an `AnalyserNode`, for an app with no engine.
  `configureAnalyser` sets the analyzer the way the scenes are tuned.

```ts
import { AnalyserFrames, configureAnalyser, Stage } from "@cantina/scenes";

const analyser = configureAnalyser(ctx.createAnalyser());
source.connect(analyser);
const stage = new Stage(host, new AnalyserFrames(analyser), {
  sampleRate: ctx.sampleRate,
  quality: 1,
  reduceFlashing: false,
  retroLook: false,
  palette: "auto",
  options: {},
});
stage.start();
await stage.setScene("tunnel");
```

`SCENES` lists every scene with its id, name, family, options, a
`thumbnail` URL and `load()`, which makes a `Renderer` whose
`render(frame, dt)` draws one frame. `Stage` runs the animation loop,
crossfades between scenes and steps quality down on slow frames.

## Tests

- `npm test` runs the unit tests in `test/` with the app's.
- `npm run test:scenes` draws every scene in headless Chrome from
  `test/recording.bin`, a second of frames from the engine's real
  analyzer over a kick, a hat and a chord at 120 BPM, and checks each one
  lit pixels and moved. `--write` redraws the thumbnails. Rerun the Rust
  tests with `CANTINA_WRITE_FIXTURES=1` to record new frames.

The scenes keep music-visualizer's code style and compiler settings
(`tsconfig.json` here) rather than the app's, so changes can go back and
forth between the two.
