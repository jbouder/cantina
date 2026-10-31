# Skins

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

## Adding a skin

1. Create `ui/src/skins/<id>.css` with a `:root[data-skin="<id>"]` block.
   Start from Midnight, which shows a skin that changes shape and
   material, not just palette. Set the palette first, then the materials
   that should differ: `--window-radius`, `--window-bg`, `--button-bg` and
   its hover and active variants, `--lcd-radius`, `--lcd-shadow`.
2. Import it in `ui/src/styles.css` next to the others.
3. Add `{ id: "<id>", name: "<Name>" }` to `SKINS` in `ui/src/skins/index.ts`.
   The picker in Settings, persistence and the `?skin=` screenshot switch
   all read that list.
4. Run `scripts/screenshot.sh <name> "skin=<id>" "skin=<id>&settings"` and
   look at the result.

### What every skin must keep

- **44 px targets.** Buttons are `min-h-11 min-w-11`; a skin changes their
  material, never their size. Round buttons are fine at 44 px.
- **Visible focus.** `:focus-visible` draws `2px solid var(--focus)` with an
  offset. Pick a `--focus` that contrasts with both the frame and the
  buttons; on light frames (Brushed metal, Toxic) that means a dark or
  saturated ring, not the pale blue Classic uses.
- **Contrast.** Text on the frame (`--ink` on `--frame`), glyphs on buttons
  (`--glyph` on `--silver`, `--pbtn-ink` on `--pbtn`) and LCD text
  (`--lcd-fg` on `--lcd-bg`) all need at least 4.5:1.
- **Reduced motion.** Anything that moves on `--beat` must read as static
  when the property is 0, which it is under `prefers-reduced-motion`.
- **No literal colors outside the skin file.** If a component needs a new
  color, add a token to Classic first and give it a value in every skin
  that should differ.

## Persistence

The chosen skin is saved to `localStorage` under `cantina.skin` and applied
in `main.tsx` before the first render so a relaunch never flashes Classic.
In a browser (`npm run dev`) `?skin=<id>` overrides the saved choice, which
is how `scripts/screenshot.sh` captures each skin.
