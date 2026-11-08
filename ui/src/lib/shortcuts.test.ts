import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { formatShortcut, shortcutSections, shortcutsMarkdown } from "./shortcuts";

const PAGE = fileURLToPath(
  new URL("../../../docs/src/content/docs/reference/shortcuts.md", import.meta.url),
);

describe("formatShortcut", () => {
  it("writes shortcuts as Mac menus do", () => {
    expect(formatShortcut("CmdOrCtrl+E")).toBe("⌘E");
    expect(formatShortcut("Shift+CmdOrCtrl+Z")).toBe("⇧⌘Z");
    expect(formatShortcut("Alt+CmdOrCtrl+N")).toBe("⌥⌘N");
    expect(formatShortcut("CmdOrCtrl+Comma")).toBe("⌘,");
    expect(formatShortcut("Space")).toBe("Space");
    expect(formatShortcut("R")).toBe("R");
  });
});

describe("the shortcut reference", () => {
  it("lists every menu item that has a shortcut", () => {
    const keys = shortcutSections().flatMap((s) => s.rows.map((r) => r.does));
    expect(keys).toContain("Export Mixdown…");
    expect(keys).toContain("Play");
    expect(keys).not.toContain("Rename…");
  });

  // The docs page is generated; UPDATE_SHORTCUTS=1 rewrites it.
  it("matches the docs page", () => {
    const want = shortcutsMarkdown();
    if (process.env.UPDATE_SHORTCUTS) writeFileSync(PAGE, want);
    expect(readFileSync(PAGE, "utf8")).toBe(want);
  });
});
