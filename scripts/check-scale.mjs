#!/usr/bin/env node
// Fails when a UI source file steps off the type scale, the spacing
// scale or the label rules.
//
// The skins define four type sizes and one spacing unit (see
// ui/src/skins/classic.css), and styles.css turns them into utilities.
// This holds components to them:
//
//   type     only text-label, text-body, text-readout and text-display;
//            no Tailwind size (text-sm), arbitrary size (text-[10px]),
//            inline fontSize or literal canvas font size
//   spacing  gaps, padding and margins only on the steps below
//   case     capitals come from the `caption` utility alone: no
//            `uppercase`, `normal-case` or `tracking-*` in a component
//   fields   an input, select or textarea is a `surface-field`, never the
//            read-only display material `surface-lcd`
//
//   node scripts/check-scale.mjs            # exit 1 on any finding
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const uiRoot = join(root, "ui", "src");
const isSkin = (file) => /\/ui\/src\/skins\/[^/]+\.css$/.test(file);

/** The spacing steps, in units of --space-unit (4 px): 0 to 32 px. */
export const STEPS = ["0", "px", "0.5", "1", "2", "3", "4", "6", "8", "auto", "window"];

const SIZE = /(?<![\w-])text-(?:xs|sm|base|lg|xl|[2-9]xl|\[[\d.]+(?:px|rem|em|pt)\])(?![\w-])/;
const INLINE_SIZE = /\bfontSize\b|\bfont\s*=\s*[`"'][^`"']*\d(?:px|rem|em|pt)/;
const CSS_SIZE = /font-size:\s*[\d.]/;
const SPACE =
  /(?<![\w-])-?(?:gap|gap-x|gap-y|space-x|space-y|p|px|py|pt|pr|pb|pl|ps|pe|m|mx|my|mt|mr|mb|ml|ms|me)-(\[[^\]]+\]|[\w.]+)(?![\w-])/g;
const CASE = /(?<![\w-])(?:uppercase|lowercase|capitalize|normal-case|tracking-[\w.[\]-]+)(?![\w-])/;
const FIELD = /<(input|select|textarea)\b[^<]*?(?:\/>|>\s*\n)/gs;

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else if (/\.(tsx?|css)$/.test(name) && !/\.test\.tsx?$/.test(name)) yield path;
  }
}

/** Class names live in string literals; prose in comments is not checked. */
function strings(line) {
  return (line.replace(/\/\/.*$/, "").match(/"[^"]*"|'[^']*'|`[^`]*`/g) ?? []).join(" ");
}

function findings(file) {
  const out = [];
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  if (file.endsWith(".css")) {
    lines.forEach((line, i) => {
      if (CSS_SIZE.test(line)) out.push([i + 1, "literal font size; use a --type-* token"]);
    });
    return out;
  }
  lines.forEach((raw, i) => {
    const code = raw.replace(/\/\/.*$/, "");
    const line = strings(raw);
    if (SIZE.test(line)) out.push([i + 1, "size off the type scale; use text-label, -body, -readout or -display"]);
    if (INLINE_SIZE.test(code)) out.push([i + 1, "literal font size; read a --type-* token"]);
    for (const m of line.matchAll(SPACE)) {
      if (!STEPS.includes(m[1])) out.push([i + 1, `${m[0]} is off the spacing scale (${STEPS.slice(0, 9).join(", ")})`]);
    }
    if (CASE.test(line)) out.push([i + 1, "case and tracking come from the caption utility; text is sentence case"]);
  });
  for (const m of text.matchAll(FIELD)) {
    if (m[0].includes("surface-lcd")) {
      const at = text.slice(0, m.index).split("\n").length;
      out.push([at, `a <${m[1]}> is editable; use surface-field, not surface-lcd`]);
    }
  }
  return out;
}

let failed = false;
for (const file of walk(uiRoot)) {
  if (isSkin(file)) continue;
  for (const [line, what] of findings(file)) {
    failed = true;
    console.error(`${relative(root, file)}:${line}: ${what}`);
  }
}
if (failed) {
  console.error("\nSee docs/src/content/docs/architecture/ui.md, \"Type, spacing and labels\".");
  process.exit(1);
}
console.log("check-scale: every component is on the type and spacing scales");
