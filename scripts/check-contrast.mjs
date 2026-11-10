#!/usr/bin/env node
// Checks every skin's text and focus contrast, WCAG 2.2 style.
//
// Reads the hex tokens out of each skin: the shared palette in
// packages/skins/tokens/ and Cantina's own in ui/src/skins/, Classic
// first as the fallback, and checks the pairs below. Gradients cannot be read, so the
// transport key is represented by --silver, the panel key by --pbtn, and
// the frame by --frame; skins keep those honest for what the eye sees.
//
//   node scripts/check-contrast.mjs            # exit 1 on any failure
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const dir = join(root, "ui", "src", "skins");
const shared = join(root, "packages", "skins", "tokens");

/** A skin's tokens: the shared palette, then Cantina's on top. */
function skin(name) {
  return { ...tokens(join(shared, name)), ...tokens(join(dir, name)) };
}

function tokens(file) {
  const out = {};
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-f]{3,8}|var\(--[a-z0-9-]+\))\s*;/gi)) {
    out[m[1]] = m[2];
  }
  return out;
}

/** Follows `var(--x)` references until a plain color or nothing. */
function resolve(t, name, depth = 0) {
  const v = t[name];
  if (!v || depth > 8) return undefined;
  const ref = /^var\(--([a-z0-9-]+)\)$/i.exec(v);
  return ref ? resolve(t, ref[1], depth + 1) : v;
}

function luminance(hex) {
  let h = hex.slice(1);
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  const [r, g, b] = [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16) / 255);
  const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a, b) {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

// [foreground, background, minimum, what]. The focus ring is drawn with an
// offset, so it sits on the surface around a control: the face or a sheet.
const PAIRS = [
  ["ink", "face", 4.5, "text on the panel face"],
  ["ink-dim", "face", 4.5, "secondary text on the panel face"],
  ["glyph", "silver", 4.5, "label on a transport key"],
  ["pbtn-ink", "pbtn", 4.5, "label on a panel key"],
  ["lcd-fg", "lcd-bg", 4.5, "display text"],
  ["lcd-error", "lcd-bg", 4.5, "display error text"],
  ["sheet-ink", "sheet-bg", 4.5, "text in a sheet"],
  ["title-text", "title-bg", 4.5, "the window title"],
  ["field-fg", "field-bg", 4.5, "text in a field"],
  ["field-dim", "field-bg", 4.5, "placeholder text in a field"],
  ["state-on-ink", "state-on", 4.5, "label on a key that is toggled on"],
  ["state-selected-ink", "state-selected", 4.5, "label on a selected key"],
  ["state-disabled-ink", "state-disabled", 3, "label on a disabled key"],
  ["focus", "face", 3, "focus ring on the panel face"],
  ["focus", "sheet-bg", 3, "focus ring in a sheet"],
];

/** Straight-line distance between two colors, 0 to 441. */
function distance(a, b) {
  const rgb = (hex) => {
    let h = hex.slice(1);
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
    return [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
  };
  const [x, y] = [rgb(a), rgb(b)];
  return Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);
}

// The three states a key can be in must not pass for one another, and
// neither may a state pass for a key at rest: each pair of fills is at
// least this far apart.
const STATES = ["state-on", "state-selected", "state-disabled"];
const APART = 40;
// A disabled key is never brighter than a live one of either tier.
const LIVE = ["silver", "pbtn"];

const base = skin("classic.css");
let failed = false;
for (const name of readdirSync(dir).filter((f) => f.endsWith(".css")).sort()) {
  const t = { ...base, ...skin(name) };
  // A title bar that is transparent or a gradient shows the face through.
  t["title-bg"] = resolve(t, "title-bg") ?? t.face;
  for (const [fg, bg, min, what] of PAIRS) {
    const [f, b] = [resolve(t, fg), resolve(t, bg)];
    if (!f || !b) {
      console.error(`${name}: cannot check ${what}: --${fg} or --${bg} is not a plain color`);
      failed = true;
      continue;
    }
    const r = ratio(f, b);
    if (r < min) {
      failed = true;
      console.error(`${name}: ${what} (--${fg} ${f} on --${bg} ${b}) is ${r.toFixed(2)}:1, needs ${min}:1`);
    }
  }
  const fills = STATES.map((name) => [name, resolve(t, name)]);
  for (const [state, value] of fills) {
    if (!value) {
      console.error(`${name}: --${state} is not a plain color`);
      failed = true;
    }
  }
  if (fills.some(([, value]) => !value)) continue;
  for (let i = 0; i < fills.length; i++) {
    for (let j = i + 1; j < fills.length; j++) {
      const d = distance(fills[i][1], fills[j][1]);
      if (d < APART) {
        failed = true;
        console.error(
          `${name}: --${fills[i][0]} ${fills[i][1]} and --${fills[j][0]} ${fills[j][1]} look alike (${d.toFixed(0)} apart, needs ${APART})`,
        );
      }
    }
  }
  const disabled = luminance(resolve(t, "state-disabled"));
  for (const key of LIVE) {
    const live = resolve(t, key);
    if (live && disabled > luminance(live)) {
      failed = true;
      console.error(`${name}: a disabled key (--state-disabled) is brighter than a live one (--${key} ${live})`);
    }
  }
}
if (failed) process.exit(1);
console.log("check-contrast: every skin meets its text and focus ratios");
