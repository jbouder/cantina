#!/usr/bin/env node
// Draws every scene in packages/scenes from the recorded engine frames in
// headless Chrome and checks each one drew something that moves.
//
//   node scripts/scene-harness.mjs            check
//   node scripts/scene-harness.mjs --write    also write the thumbnails
//
// The recording comes from the real analyser: rerun the Rust tests with
// CANTINA_WRITE_FIXTURES=1 to make a new one.

import { execFile } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { createServer } from "vite";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const scenes = join(root, "packages/scenes");
const chrome =
  process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const write = process.argv.includes("--write");

// Below this share of lit pixels a scene drew next to nothing.
const MIN_LIT = 0.01;
// Below this mean change between the middle and the end it did not move.
const MIN_MOTION = 0.0005;

const server = await createServer({
  configFile: false,
  root: join(scenes, "test"),
  logLevel: "error",
  server: { port: 0, fs: { allow: [root] } },
});
await server.listen();
const { port } = server.httpServer.address();
let out;
try {
  ({ stdout: out } = await promisify(execFile)(
    chrome,
    [
      "--headless=new",
      "--disable-gpu-sandbox",
      "--use-angle=swiftshader",
      "--enable-unsafe-swiftshader",
      "--no-first-run",
      "--no-default-browser-check",
      `--user-data-dir=${join(root, "node_modules/.cache/scene-harness-chrome")}`,
      "--virtual-time-budget=60000",
      "--timeout=120000",
      "--dump-dom",
      `http://localhost:${port}/harness.html`,
    ],
    { maxBuffer: 64 * 1024 * 1024, timeout: 180_000 },
  ));
} finally {
  await server.close();
}

const match = out.match(/(RESULTS|FAILED) ([\s\S]*?) END/);
if (!match) {
  console.error("The harness page did not finish.");
  process.exit(1);
}
if (match[1] === "FAILED") {
  console.error(`The harness failed: ${match[2]}`);
  process.exit(1);
}
const results = JSON.parse(match[2].replaceAll("&amp;", "&").replaceAll("&lt;", "<").replaceAll("&gt;", ">").replaceAll("&quot;", '"'));
let failed = 0;
for (const r of results) {
  const problems = [];
  if (!r.ok) problems.push(r.error ?? "did not draw");
  else {
    if (r.lit < MIN_LIT) problems.push(`lit ${(r.lit * 100).toFixed(1)}%`);
    if (r.motion < MIN_MOTION) problems.push(`motion ${r.motion.toFixed(4)}`);
  }
  if (problems.length) failed++;
  console.log(
    `${problems.length ? "FAIL" : "ok  "} ${r.id.padEnd(12)} lit ${(r.lit * 100).toFixed(1).padStart(5)}%  motion ${r.motion.toFixed(4)}${problems.length ? `  ${problems.join("; ")}` : ""}`,
  );
  if (write && r.ok) {
    const png = Buffer.from(r.thumbnail.replace(/^data:image\/png;base64,/, ""), "base64");
    writeFileSync(join(scenes, "thumbnails", `${r.id}.png`), png);
  }
}
console.log(`${results.length - failed} of ${results.length} scenes drew from engine frames.`);
process.exit(failed ? 1 : 0);
