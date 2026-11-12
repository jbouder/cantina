import { describe, expect, it } from "vitest";
import { gridPads, renameSection, withPattern } from "./designEdit";
import { type DrumPart, emptyPattern } from "./drumPart";

function part(): DrumPart {
  return {
    kit: "rock",
    sections: [
      {
        name: "Verse",
        kind: "verse",
        pattern: emptyPattern("Verse"),
        fill: { section: "Fill", every: 4, atEnd: false },
      },
      { name: "Fill", kind: "fill", pattern: emptyPattern("Fill") },
      { name: "Chorus", kind: "chorus", pattern: emptyPattern("Chorus") },
    ],
    order: [
      { section: "Verse", times: 2 },
      { section: "Fill", times: 1 },
    ],
  };
}

describe("renaming a section", () => {
  it("follows it through the order and the fills", () => {
    const next = renameSection(part(), "Fill", "Turnaround");
    expect(next.sections.map((s) => s.name)).toEqual(["Verse", "Turnaround", "Chorus"]);
    expect(next.sections[0]?.fill?.section).toBe("Turnaround");
    expect(next.order[1]?.section).toBe("Turnaround");
    expect(next.sections[1]?.pattern.name).toBe("Turnaround");
  });
  it("keeps names unique and ignores a blank one", () => {
    expect(renameSection(part(), "Fill", "Chorus").sections[1]?.name).toBe("Chorus 2");
    expect(renameSection(part(), "Fill", "  ")).toEqual(part());
  });
});

describe("a section's pattern", () => {
  it("is replaced alone", () => {
    const p = { ...emptyPattern("Chorus"), length: 32 };
    const next = withPattern(part(), "Chorus", p);
    expect(next.sections[2]?.pattern.length).toBe(32);
    expect(next.sections[0]).toEqual(part().sections[0]);
  });
});

describe("the grid's pads", () => {
  it("are the kit's, then pads the pattern plays that the kit lacks", () => {
    const p = {
      ...emptyPattern("Verse"),
      rows: [
        { pad: "kick", steps: [1] },
        { pad: "cowbell", steps: [1] },
        { pad: "tom", steps: [0] },
      ],
    };
    expect(gridPads([{ id: "kick", name: "Kick" }], p).map((x) => x.id)).toEqual([
      "kick",
      "cowbell",
    ]);
  });
});
