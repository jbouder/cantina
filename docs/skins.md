# Skins

Six skins ship, ported from the visualizer: **Classic** (gunmetal frame,
silver transport, green LCD), **Midnight** (rounded navy pod, chrome keys,
glass displays), **Brushed metal** (screwed aluminum faceplate, machined
keys, dot-matrix display), **Hi-fi** (walnut cheeks, champagne panel,
piano-key transport, VFD), **Neon** (angled cuts, outline buttons, an edge
that pulses with the beat) and **Toxic** (lime shell, gel buttons,
hazard-stripe title bar).

Cantina's UI is drawn entirely through **skin tokens**: CSS custom
properties defined per skin under `:root[data-skin='…']` in
`ui/src/skins/`. Components never carry a literal color, radius, shadow or
typeface; they name a token through a Tailwind utility (`bg-lcd-bg`,
`text-ink-dim`, `rounded-control`) or one of the material utilities
(`surface-window`, `surface-button`, `surface-lcd`, …) declared in
`ui/src/styles.css`. Switching `data-skin` on `<html>` reskins everything
live.

This is the same mechanism as `src/ui/skin.css` in
[music-visualizer](https://github.com/jbouder/music-visualizer), and the
six player skins port onto it.

## The contract

`ui/src/skins/classic.css` defines every token with a comment per group:
frame, title bar, displays, transport buttons, panel buttons, text, LEDs,
meters, sheets, focus, **materials** (how the colors compose into window,
title bar, button and LCD surfaces), type, and `--beat`. Classic is also
the fallback for any token a skin leaves out, so a new skin only sets what
differs.

Tailwind's default palette is cleared in `styles.css`, so `bg-red-500`
produces no CSS. `scripts/check-colors.mjs` runs as part of `npm run lint`
and fails on any hex value, `rgb()`-style function or CSS named color
outside `ui/src/skins/`. Together they are the "no literal colors" rule.

### Material tokens

Beyond the palette, a skin shapes the UI with these (all have Classic
defaults, so set only what differs):

- **Window**: `--window-radius`, `--window-bg` (a full `background`
  layer list, so screws and wood cheeks go here), `--window-shadow`,
  `--window-clip` (a `clip-path` for cut corners), `--window-filter` (a
  glow on the window's wrapper, outside the clip), `--window-padding`.
- **Panels** (pedal cards, sheets): `--panel-bg`, `--panel-shadow`,
  `--panel-radius`; default to the window's material.
- **Title bar**: `--titlebar-bg`, `--titlebar-shadow`, `--titlebar-border`,
  `--titlebar-radius`, `--titlebar-margin`, `--titlebar-grip`,
  `--grip-height`, `--grip-opacity`, and for the title itself
  `--title-text-shadow`, `--title-tracking`, `--title-bg`,
  `--title-padding`, `--title-radius`.
- **Buttons**: `--button-radius`, `--button-bg`, `--button-shadow`, the
  `--button-hover-*` and `--button-active-*` variants (including
  `--button-active-ink` and `--button-active-transform` for keys that
  travel), and `--pbutton-radius`, `--pbutton-bg`, `--pbutton-shadow` for
  the panel keys. Faders draw their thumb from the button material.
- **Displays**: `--lcd-radius`, `--lcd-border`, `--lcd-shadow`,
  `--lcd-overlay` (a grid over the display), `--lcd-mask` (turns display
  text into a dot matrix), `--lcd-text-shadow`, `--meter-bg`.
- **Sheets**: `--sheet-bg`, `--sheet-ink`, `--scrim`.
- **Beat**: `--beat` is 0 to 1 within the beat once M8 drives it from the
  tempo map, and 0 under reduced motion. Neon's `--window-filter` grows
  with it; any skin may use it the same way.

## Adding a skin

1. Create `ui/src/skins/<id>.css` with a `:root[data-skin="<id>"]` block.
   Start from Midnight for a skin that changes shape and material, Neon
   for one that changes everything, or Toxic for a different title bar.
   Set the palette first, then the materials that should differ.
2. Import it in `ui/src/styles.css` next to the others.
3. Add `{ id: "<id>", name: "<Name>" }` to `SKINS` in `ui/src/skins/index.ts`.
   The picker in Settings, persistence and the `?skin=` screenshot switch
   all read that list.
4. Run `scripts/screenshot.sh <name> "skin=<id>" "skin=<id>&settings"` and
   look at the result.

### What every skin must keep

- **Sizes are the component's.** Every key and display is 32 px; a skin
  changes their material, never their size. Round keys are fine.
- **Visible focus.** `:focus-visible` draws `2px solid var(--focus)` with an
  offset. Pick a `--focus` that contrasts with both the frame and the
  buttons; on light frames (Brushed metal, Toxic) that means a dark or
  saturated ring, not the pale blue Classic uses.
- **Contrast.** `scripts/check-contrast.mjs`, part of `npm run lint`, reads
  each skin's tokens and fails below 4.5:1 for text (`--ink` and
  `--ink-dim` on `--face`, `--glyph` on `--silver`, `--pbtn-ink` on
  `--pbtn`, `--lcd-fg` and `--lcd-error` on `--lcd-bg`, `--sheet-ink` on
  `--sheet-bg`, `--title-text` on `--title-bg` or the face) and below 3:1
  for the focus ring on `--face` and `--sheet-bg`. Gradients cannot be
  read, so `--face` is the flat color the panel averages to and `--silver`
  and `--pbtn` the faces of the two kinds of key; keep them honest.
- **Reduced motion.** Anything that moves on `--beat` must read as static
  when the property is 0, which it is under `prefers-reduced-motion`.
- **No literal colors outside the skin file.** If a component needs a new
  color, add a token to Classic first and give it a value in every skin
  that should differ.
- **Light frames set `color-scheme: light`** so form controls and scrollbars
  match. Brushed metal, Hi-fi and Toxic do.

## Persistence

The chosen skin is saved to `localStorage` under `cantina.skin` and applied
in `main.tsx` before the first render so a relaunch never flashes Classic.
In a browser (`npm run dev`) `?skin=<id>` overrides the saved choice, which
is how `scripts/screenshot.sh` captures each skin.
