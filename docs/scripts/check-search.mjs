#!/usr/bin/env node
// Fails the build when Pagefind, Starlight's search, has not indexed every
// page: each HTML page in dist/ except the 404 must be in the index.
//
//   node scripts/check-search.mjs      # after astro build
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const dist = new URL("../dist", import.meta.url).pathname;

function* pages(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (name !== "pagefind" && name !== "_astro") yield* pages(path);
    } else if (name === "index.html") {
      // Redirect pages carry no content to index.
      if (!readFileSync(path, "utf8").includes('http-equiv="refresh"')) yield path;
    }
  }
}

const expected = [...pages(dist)].length;
const entry = JSON.parse(readFileSync(join(dist, "pagefind", "pagefind-entry.json"), "utf8"));
const indexed = Object.values(entry.languages).reduce((sum, lang) => sum + lang.page_count, 0);

if (indexed !== expected) {
  console.error(`search: ${indexed} pages indexed, ${expected} built`);
  process.exit(1);
}
console.log(`search: all ${indexed} pages indexed`);
