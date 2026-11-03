---
title: "UI"
description: "The React and TypeScript app in ui/, the skin system it is drawn in, the canvas timeline, and the shared scenes and skins packages."
sidebar:
  order: 10
---

The UI is React and TypeScript in `ui/`, styled with Tailwind v4 over the skin tokens, linted with Biome. It talks to the shell through `ui/src/lib/engine.ts`, which also has a stand-in engine for the browser: `npm run dev` serves the whole UI against it, which is enough for layout and skin work and is what `scripts/screenshot.sh` captures.

## Where things are

| Path | What |
| --- | --- |
| `ui/src/components/` | The window, title bar, transport, timeline, track headers, editor pane, pedal chain and picker, drum grid, designer, settings and stage |
| `ui/src/lib/` | The engine client, the frame decoder, and pure functions for the timeline, drums, pedals and stage, each with tests |
| `ui/src/skins/` | Cantina's own skin tokens and materials, on top of `packages/skins` |
| `packages/skins` | The six skins' shared palette, also used by music-visualizer |
| `packages/scenes` | The visualizer scenes the stage draws, also used by music-visualizer |

## Skins

Every component is drawn through semantic tokens: CSS custom properties defined per skin under `:root[data-skin='…']`. Components never carry a literal colour, radius, shadow or typeface; they name a token through a Tailwind utility or a material utility (`surface-window`, `surface-button`, `surface-lcd`). `npm run lint` fails on a literal colour outside the skin files and on any skin that falls below the contrast ratios. See [skins](/reference/skins/) for writing one.

## The timeline

The timeline is a canvas, not DOM. `Timeline.tsx` draws tracks, clips with waveforms, the ruler, the grid and the playhead from the project status and the frames, and everything it decides (pixels to samples, the grid, snapping, hit testing, what a move or trim does to a clip) is a pure function in `ui/src/lib/timeline.ts`, tested on its own.

A file's samples are reduced once, in a Web Worker (`peaks.worker.ts`), into min and max per bin at five bin sizes from 64 to 16384 samples. Drawing a clip reads the coarsest level whose bin is no larger than the zoom's samples per pixel, so a column costs a few reads at any zoom and a long take is never touched again after its cache is built. The canvas reads its colours from the skin's `--tl-*` tokens on each draw, so it reskins live.

Edits are previewed on the canvas while dragging and become one command when the pointer is released.

## Frames

`ui/src/lib/frame.ts` decodes the binary [frame](/architecture/frames/) the shell sends each display frame. Components that show levels or the stage read the latest one in an animation frame and acknowledge it, so a busy page never builds a backlog.

## Checks

```sh
npm run check        # Biome, the colour and contrast checks, TypeScript and the tests
npm run test:scenes  # every scene drawn in headless Chrome from recorded frames
```
