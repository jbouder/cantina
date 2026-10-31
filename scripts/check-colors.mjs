#!/usr/bin/env node
// Fails when a UI source file carries a literal color.
//
// Every color comes from a skin token, so the only files allowed to spell
// one out are the skin definitions in ui/src/skins/*.css. Everywhere else
// a hex value, an rgb()/hsl()/oklch()/color() call or a CSS named color
// in a stylesheet is an error. Tailwind's palette is cleared in
// styles.css, so this is the check that catches arbitrary values such as
// `bg-[#fff]`, inline styles and hand-written CSS.
//
//   node scripts/check-colors.mjs            # exit 1 on any finding
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const uiRoot = join(root, "ui", "src");
const allowed = (file) => /\/ui\/src\/skins\/[^/]+\.css$/.test(file);

// Named colors are only checked in CSS, where they appear as values; in
// TSX "black" or "red" could be any word.
const NAMED =
  /(?<![\w-])(?:aliceblue|antiquewhite|aqua|aquamarine|azure|beige|bisque|black|blanchedalmond|blue|blueviolet|brown|burlywood|cadetblue|chartreuse|chocolate|coral|cornflowerblue|cornsilk|crimson|cyan|darkblue|darkcyan|darkgoldenrod|darkgray|darkgreen|darkgrey|darkkhaki|darkmagenta|darkolivegreen|darkorange|darkorchid|darkred|darksalmon|darkseagreen|darkslateblue|darkslategray|darkslategrey|darkturquoise|darkviolet|deeppink|deepskyblue|dimgray|dimgrey|dodgerblue|firebrick|floralwhite|forestgreen|fuchsia|gainsboro|ghostwhite|gold|goldenrod|gray|green|greenyellow|grey|honeydew|hotpink|indianred|indigo|ivory|khaki|lavender|lavenderblush|lawngreen|lemonchiffon|lightblue|lightcoral|lightcyan|lightgoldenrodyellow|lightgray|lightgreen|lightgrey|lightpink|lightsalmon|lightseagreen|lightskyblue|lightslategray|lightslategrey|lightsteelblue|lightyellow|lime|limegreen|linen|magenta|maroon|mediumaquamarine|mediumblue|mediumorchid|mediumpurple|mediumseagreen|mediumslateblue|mediumspringgreen|mediumturquoise|mediumvioletred|midnightblue|mintcream|mistyrose|moccasin|navajowhite|navy|oldlace|olive|olivedrab|orange|orangered|orchid|palegoldenrod|palegreen|paleturquoise|palevioletred|papayawhip|peachpuff|peru|pink|plum|powderblue|purple|rebeccapurple|red|rosybrown|royalblue|saddlebrown|salmon|sandybrown|seagreen|seashell|sienna|silver|skyblue|slateblue|slategray|slategrey|snow|springgreen|steelblue|tan|teal|thistle|tomato|turquoise|violet|wheat|white|whitesmoke|yellow|yellowgreen)(?![\w-])/i;

const HEX = /#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})\b/i;
const FUNC = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i;

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else if (/\.(tsx?|css)$/.test(name)) yield path;
  }
}

function findings(file) {
  const out = [];
  const css = file.endsWith(".css");
  const lines = readFileSync(file, "utf8").split("\n");
  lines.forEach((raw, i) => {
    // Strip comments and `url(#id)` references before matching.
    const line = raw.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "").replace(/url\([^)]*\)/g, "");
    // In TSX a `#` followed by hex digits is also how SVG paths and ids
    // look; only flag it in a string or CSS-like position.
    if (HEX.test(line) && (css || /["'`]#[0-9a-f]{3,8}\b/i.test(line))) out.push([i + 1, "hex color"]);
    if (FUNC.test(line)) out.push([i + 1, "color function"]);
    if (css && /:\s*[^;{}]*$/.test(line) && NAMED.test(line.replace(/^[^:]*:/, "")))
      out.push([i + 1, "named color"]);
  });
  return out;
}

let failed = false;
for (const file of walk(uiRoot)) {
  if (allowed(file)) continue;
  for (const [line, what] of findings(file)) {
    failed = true;
    console.error(`${relative(root, file)}:${line}: literal ${what}; use a skin token`);
  }
}
if (failed) {
  console.error("\nColors belong in ui/src/skins/*.css as tokens. See docs/skins.md.");
  process.exit(1);
}
console.log("check-colors: no literal colors outside ui/src/skins/");
