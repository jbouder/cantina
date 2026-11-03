import { describe, expect, it } from "vitest";
import { brokenOf, type LibraryStatus, patternFileName } from "./library";

describe("library", () => {
  it("names pattern files safely", () => {
    expect(patternFileName("Half-time")).toBe("Half-time.pattern.json");
    expect(patternFileName("Half/time: 140")).toBe("Half-time- 140.pattern.json");
    expect(patternFileName("  ")).toBe("Pattern.pattern.json");
  });

  it("picks the broken files of one kind", () => {
    const library: LibraryStatus = {
      pedalsDir: "/p",
      kitsDir: "/k",
      patternsDir: "/t",
      patterns: [],
      broken: [
        { kind: "pedal", name: "a.pedal.json", path: "/p/a.pedal.json", reason: "x" },
        { kind: "kit", name: "k", path: "/k/k", reason: "y" },
      ],
    };
    expect(brokenOf(library, "kit").map((b) => b.name)).toEqual(["k"]);
    expect(brokenOf(library, "pattern")).toEqual([]);
  });
});
