import { describe, expect, it } from "vitest";
import type { RecentProject } from "./engine";
import { defaultProject, savedLabel } from "./start";

const NOW = Date.UTC(2026, 10, 9, 12);
const at = (ago: number): RecentProject => ({ name: "A", saved: NOW - ago, missing: false });

describe("start dialog", () => {
  it("says when each project was saved", () => {
    expect(savedLabel(at(5_000), NOW)).toBe("Saved just now");
    expect(savedLabel(at(60_000), NOW)).toBe("Saved 1 minute ago");
    expect(savedLabel(at(20 * 60_000), NOW)).toBe("Saved 20 minutes ago");
    expect(savedLabel(at(3 * 3_600_000), NOW)).toBe("Saved 3 hours ago");
    expect(savedLabel(at(30 * 3_600_000), NOW)).toBe("Saved yesterday");
    expect(savedLabel(at(9 * 86_400_000), NOW)).toBe("Saved 9 days ago");
    expect(savedLabel(at(90 * 86_400_000), NOW)).toMatch(/^Saved .*2026/);
    // A clock that went backwards is not "in the future".
    expect(savedLabel(at(-60_000), NOW)).toBe("Saved just now");
  });

  it("says so when a project is gone", () => {
    expect(savedLabel({ name: "B", saved: null, missing: true }, NOW)).toBe("Moved or deleted");
  });

  it("picks the most recent project that is still there", () => {
    const gone: RecentProject = { name: "Gone", saved: null, missing: true };
    expect(defaultProject([gone, { ...at(1), name: "Song" }, at(2)])).toBe("Song");
    expect(defaultProject([gone])).toBeNull();
    expect(defaultProject([])).toBeNull();
  });
});
