import { describe, expect, it } from "vitest";
import { kindOf, layout, orderTicks, partFromTrack, uniqueName } from "./drumPart";
import type { PatternInfo } from "./engine";

const BAR = 3840;

function pattern(id: number, name: string, length = 16): PatternInfo {
  return { id, name, stepsPerBeat: 4, length, swing: 0, rows: [] };
}

describe("a part from a track", () => {
  it("takes a section per pattern and an order from the clips", () => {
    const part = partFromTrack(
      "rock",
      [
        { id: 1, pattern: 1, start: 0, offset: 0, length: 2 * BAR },
        { id: 2, pattern: 1, start: 2 * BAR, offset: 0, length: 2 * BAR },
        {
          id: 3,
          pattern: 2,
          start: 4 * BAR,
          offset: 0,
          length: 4 * BAR,
          fill: { pattern: 3, every: 4, atEnd: false },
        },
      ],
      [pattern(1, "Verse"), pattern(2, "Chorus", 32), pattern(3, "Fill")],
    );
    expect(part.sections.map((s) => s.name)).toEqual(["Verse", "Chorus", "Fill"]);
    expect(part.order).toEqual([
      { section: "Verse", times: 4 },
      { section: "Chorus", times: 2 },
    ]);
    expect(part.sections[1]?.fill).toEqual({ section: "Fill", every: 4, atEnd: false });
    expect(orderTicks(part)).toBe(8 * BAR);
  });

  it("falls back to the project's patterns, then an empty verse", () => {
    expect(partFromTrack("rock", [], []).sections.map((s) => s.name)).toEqual(["Verse"]);
    const part = partFromTrack("rock", [], [pattern(1, "Beat"), pattern(2, "Beat")]);
    expect(part.sections.map((s) => s.name)).toEqual(["Beat", "Beat 2"]);
    expect(part.order).toHaveLength(2);
  });
});

describe("the layout", () => {
  it("puts each step after the last", () => {
    const part = partFromTrack("rock", [], [pattern(1, "Verse"), pattern(2, "Chorus", 32)]);
    part.order = [
      { section: "Verse", times: 2 },
      { section: "Chorus", times: 1 },
      { section: "Gone", times: 1 },
    ];
    expect(layout(part)).toEqual([
      { step: 0, section: 0, start: 0, length: 2 * BAR },
      { step: 1, section: 1, start: 2 * BAR, length: 2 * BAR },
    ]);
  });
});

describe("names", () => {
  it("say their kind", () => {
    expect(kindOf("Rock — Fill")).toBe("fill");
    expect(kindOf("Outro")).toBe("ending");
    expect(kindOf("Breakdown")).toBeUndefined();
  });
  it("are made unique", () => {
    expect(uniqueName("Verse", ["Verse", "Verse 2"])).toBe("Verse 3");
    expect(uniqueName(" ", [])).toBe("Section");
  });
});
