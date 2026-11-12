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

Every component is drawn through semantic tokens: CSS custom properties defined per skin under `:root[data-skin='…']`. Components never carry a literal color, radius, shadow or typeface; they name a token through a Tailwind utility or a material utility (`surface-window`, `button-standard`, `surface-lcd`). `npm run lint` fails on a literal color outside the skin files, on a size or gap off the scales below, and on any skin that falls below the contrast ratios. See [skins](/guides/skins/) for writing one and the [skin tokens](/reference/skin-tokens/) for every token.

## The timeline

The timeline is a canvas, not DOM. `Timeline.tsx` draws tracks, clips with waveforms, the ruler, the grid and the playhead from the project status and the frames, and everything it decides (pixels to samples, the grid, snapping, hit testing, what a move or trim does to a clip) is a pure function in `ui/src/lib/timeline.ts`, tested on its own.

A file's samples are reduced once, in a Web Worker (`peaks.worker.ts`), into min and max per bin at five bin sizes from 64 to 16384 samples. Drawing a clip reads the coarsest level whose bin is no larger than the zoom's samples per pixel, so a column costs a few reads at any zoom and a long take is never touched again after its cache is built. The canvas reads its colors from the skin's `--tl-*` tokens on each draw, so it reskins live.

Edits are previewed on the canvas while dragging and become one command when the pointer is released.

The arrangement's height comes from `splitHeight` in `ui/src/lib/timeline.ts`. With no split of the user's the editor gets `EDITOR_ROOM`, enough for a pedal's controls, and the arrangement the rest, but never less than its tracks up to half the body; the timeline's canvas fills the room, so the grid runs on under the last track. A dragged split is kept as long as each pane keeps its minimum. The master strip is outside the scrolling tracks, pinned under them. In a browser `?tracks=<n>` gives the stand-in project n audio tracks.

## The tab view

`TabView.tsx` draws a clip's tab as SVG in the display material. Layout is
`ui/src/lib/tabLayout.ts`, pure and tested: bars from the tab's own tempo
and time signature, a note's x from its tick within its bar, rows of bars
for the page view, the written rhythm nearest each beat's length, and the
labels for frets and techniques. The playhead becomes a tick through the
clip's anchor (`clip.start - clip.offset + tab.origin`), so the cursor
stays on the notes when the clip is moved or trimmed; a click or a key
turns the tick back into a sample for `transportLocate`. The shell sends
the whole tab only when the view asks for it (`clip_tab`); the status
carries a summary.

## Dragging

Drags inside the window use pointer events, not HTML drag and drop. On
macOS the shell keeps Tauri's file-drop handler on, so pedal files can be
dropped on the window, and that handler claims every drop: the webview
never fires `drop`, though `dragover` still arrives. A drag that works
in the browser against the stand-in can still fail in the app. The pedal
chain finds where a card lands from the cards' positions
(`ui/src/lib/reorder.ts`). A pattern dragged from the drum grid crosses
components, so `ui/src/lib/pattern-drag.ts` holds the drag: the grid
publishes it, a chip follows the pointer, and the timeline registers its
canvas as the drop target.

## Type, spacing and labels

These are the rules every component follows. `scripts/check-scale.mjs` runs in `npm run lint` and fails on a component that breaks the first three.

**Type.** Four sizes, set by the skin and named by use. Tailwind's own sizes are cleared, so `text-sm` or `text-[10px]` draws nothing.

| Utility | Token | Classic | Use |
| --- | --- | --- | --- |
| `text-label` | `--type-label` | 11 px | Captions, button text, hints, badges. The floor: nothing is smaller. |
| `text-body` | `--type-body` | 13 px | Sentences, form labels, field text, names. |
| `text-readout` | `--type-readout` | 16 px | Text on a display, in the LCD face. |
| `text-display` | `--type-display` | 24 px | A figure read from across the room: the tuner note, a count-in. |

A canvas reads `--type-label` from the computed style instead of writing a size.

**Spacing.** Gaps, padding and margins are steps of `--space-unit` (4 px): `0`, `0.5`, `1`, `2`, `3`, `4`, `6` and `8`, which is 0 to 32 px. `gap-2` is fine; `gap-1.5`, `px-5` and `p-[6px]` fail the check. Widths and heights are not held to the steps.

**Case.** Captions are capitals. Nothing else is. A caption is a legend printed on the panel: the name of a section, a column, a meter, a knob or a fader. It uses the `caption` utility, which sets the size, weight, tracking and capitals together. Button text, form labels, messages, field text and anything the user typed (a track, a clip, a pedal) are sentence case. A component never writes `uppercase` or `tracking-*` itself. Inside a caption, `hint` turns a note back into plain text.

**Placement.** A label comes before its control, on the same row and to its left. It goes above only when the control is as wide as its container: a list, a grid, a textarea. A label is never below its control and never only a placeholder.

**Buttons.** Three tiers, chosen with the `tier` prop on `Button` or the matching utility:

| Tier | Utility | Use |
| --- | --- | --- |
| `primary` | `button-primary` | The one action the view is for. At most one per view. |
| `standard` | `button-standard` | Every other action. The default. |
| `quiet` | `button-quiet` | Low-stakes and repeated actions, such as Rename or a row's Remove. No key shows until hover. |

**States.** A state comes from ARIA, never from a class or a different tier, and looks the same on every tier:

| State | Attribute | Look |
| --- | --- | --- |
| On | `aria-pressed="true"` | Lit, from `--state-on`. A toggle such as Snap or Mute. |
| Selected | `aria-selected`, `aria-checked`, `aria-current` | Filled, from `--state-selected`. The chosen one of several: a tab, a radio key, the open project. |
| Disabled | `disabled` | Flat and dim, from `--state-disabled`. Never brighter than a live key. |

**Messages.** A notice, a warning, an error, the update prompt and the recovery question are toasts (`Toast` inside `Toasts`), stacked in the bottom right corner of the body. They sit over the work and out of the layout, so one arriving never moves the arrangement or the editor. Each has its own role for assistive tech: `status` for something to know, `alert` for an error, `alertdialog` for a question. A message with nothing to answer takes `onDismiss`; one that asks takes `actions`. Never add a full-width row above the content for a message.

**Tabs and overflow.** Views of one thing are tabs: `role="tab"` buttons with the `tab` utility in a `tab-rule` tablist. The selected tab is filled from `--state-selected`. Actions are not tabs. An action used rarely goes in an overflow menu: a quiet square key with three dots that opens a `ContextMenu` under itself (`alignEnd`).

**Parameters.** A pedal parameter is one line (`Fader`): a caption, the control, and for a fader its value in a small display. Cards are a fixed width and as tall as their knobs, so nothing is clipped.

**Sheets and forms.** A `Sheet` is centered and never taller than the window. Its body scrolls between a fixed title bar and a fixed footer. The footer holds the sheet's keys, right-aligned, with its one primary button last; a sheet that passes no `footer` gets a single **Done**. A submit button in the footer names its form with the `form` attribute.

A form row is a `Field`: the label with its helper text under it at the left, the control at the right, in the same two columns in every sheet. `TextInput` and `Select` are fields. A setting that is on or off is a `Toggle`, which draws a switch (`role="switch"`); a `Button` with `aria-pressed` is for a key on the panel, never for a setting in a sheet. Secondary text on a display uses `text-lcd-muted`, not `text-lcd-dim`, which is the color of an unlit segment and is too faint to read.

**One label per control.** When a group already names a control, the control inside it does not name it again: `Fader` takes `bare` to draw the control alone, keeping the name as its accessible label.

**Side sheets.** `Sheet` with `side` opens down the right edge with nothing dimmed. Use it when what the sheet changes should stay in view, as the rig picker does with the chain.

**Scrolling.** A region that can scroll uses `scroll-area`, which always draws a slim bar while there is more to see. The system's bars hide until touched, which leaves clipped content looking like a bug.

**Narrow windows.** The toolbar and the transport strip never wrap. Each is a container (`@container`), and what is optional carries a container variant: a key's caption is `@5xl:inline hidden`, so below that width the key shows its glyph alone. Such a key always has an `aria-label` and a `title`. A key that only ever holds a glyph takes `square` on `Button`.

`Button` also takes `tight`, for a key in a strip whose caption drops out: the padding shrinks with it, so the glyph alone makes a 32 px key.

**Fields and readouts.** An `<input>`, `<select>` or `<textarea>` is a `surface-field`: the UI face in a well with an edge. A value that can only be read is a `surface-lcd`. The check fails on a field in the display material.

## Frames

`ui/src/lib/frame.ts` decodes the binary [frame](/architecture/frames/) the shell sends each display frame. Components that show levels or the stage read the latest one in an animation frame and acknowledge it, so a busy page never builds a backlog.

## Checks

```sh
npm run check        # Biome, the color, scale and contrast checks, TypeScript and the tests
npm run test:scenes  # every scene drawn in headless Chrome from recorded frames
```
