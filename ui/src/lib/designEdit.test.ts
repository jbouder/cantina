import { describe, expect, it } from "vitest";
import {
  addSection,
  addStep,
  duplicateSection,
  gridPads,
  moveStep,
  putGroove,
  putInSection,
  removeSection,
  removeStep,
  renameSection,
  setFill,
  setStep,
  withPattern,
} from "./designEdit";
import { type DrumPart, emptyPattern, fillBars, orderBars } from "./drumPart";
import type { PatternInfo } from "./engine";

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

describe("sections", () => {
  it("are added by kind, or of your own", () => {
    const [next, name] = addSection(part(), "chorus", null, 16);
    expect(name).toBe("Chorus 2");
    expect(next.sections.at(-1)?.kind).toBe("chorus");
    const [own, ownName] = addSection(part(), null, null, 16);
    expect(ownName).toBe("Section");
    expect(own.sections.at(-1)?.kind).toBeUndefined();
  });
  it("take the edited section's length, and a fill one bar", () => {
    const like = { ...emptyPattern("Verse"), length: 32, stepsPerBeat: 4 };
    expect(addSection(part(), "bridge", like, 16)[0].sections.at(-1)?.pattern.length).toBe(32);
    expect(addSection(part(), "fill", like, 16)[0].sections.at(-1)?.pattern.length).toBe(16);
  });
  it("are duplicated after themselves", () => {
    const [next, name] = duplicateSection(part(), "Verse");
    expect(name).toBe("Verse 2");
    expect(next.sections.map((s) => s.name)).toEqual(["Verse", "Verse 2", "Fill", "Chorus"]);
    expect(next.sections[1]?.fill).toEqual(part().sections[0]?.fill);
  });
  it("take their steps and the fills that named them when removed", () => {
    const next = removeSection(part(), "Fill");
    expect(next.sections.map((s) => s.name)).toEqual(["Verse", "Chorus"]);
    expect(next.sections[0]?.fill).toBeUndefined();
    expect(next.order).toEqual([{ section: "Verse", times: 2 }]);
  });
  it("never leave the part empty", () => {
    const one = { ...part(), sections: part().sections.slice(0, 1) };
    expect(removeSection(one, "Verse")).toBe(one);
  });
});

describe("the order", () => {
  it("gains, changes, moves and loses steps", () => {
    let p = addStep(part(), "Chorus", 2);
    expect(p.order.at(-1)).toEqual({ section: "Chorus", times: 2 });
    p = setStep(p, 2, { times: 99 });
    expect(p.order[2]?.times).toBe(64);
    p = moveStep(p, 2, -5);
    expect(p.order.map((o) => o.section)).toEqual(["Chorus", "Verse", "Fill"]);
    p = removeStep(p, 1);
    expect(p.order.map((o) => o.section)).toEqual(["Chorus", "Fill"]);
  });
  it("counts its bars and its fills", () => {
    const p = part();
    // Verse ×2 then Fill ×1, a bar each; the verse fills every 4, so never in 2 bars.
    expect(orderBars(p, 3840)).toBe(3);
    expect(fillBars(p, 3840)).toEqual([]);
    const longer = setStep(p, 0, { times: 8 });
    expect(fillBars(longer, 3840)).toEqual([3, 7]);
    const atEnd = setFill(longer, "Verse", { section: "Fill", every: 0, atEnd: true });
    expect(fillBars(atEnd, 3840)).toEqual([7]);
    expect(fillBars(setFill(longer, "Verse", null), 3840)).toEqual([]);
  });
});

describe("the palette", () => {
  const groovePattern = (name: string, length = 16): PatternInfo => ({
    ...emptyPattern(name),
    length,
    rows: [{ pad: "kick", steps: Array(length).fill(0).fill(1, 0, 1) }],
  });
  it("puts a pattern in a section, renamed to it, dropping variations that no longer fit", () => {
    const p = {
      ...part(),
      sections: part().sections.map((s, i) =>
        i === 0 ? { ...s, variations: [emptyPattern("Verse B")] } : s,
      ),
    };
    const kept = putInSection(p, "Verse", groovePattern("Rock verse"));
    expect(kept.sections[0]?.pattern.name).toBe("Verse");
    expect(kept.sections[0]?.pattern.rows[0]?.pad).toBe("kick");
    expect(kept.sections[0]?.variations).toHaveLength(1);
    const longer = putInSection(p, "Verse", groovePattern("Rock verse", 32));
    expect(longer.sections[0]?.variations).toBeUndefined();
  });
  it("fills every section from a whole groove", () => {
    const empty: DrumPart = {
      kit: "rock",
      sections: [{ name: "Verse", kind: "verse", pattern: emptyPattern("Verse") }],
      order: [],
    };
    const next = putGroove(empty, [
      ["intro", groovePattern("Rock intro")],
      ["verse", groovePattern("Rock verse")],
      ["chorus", groovePattern("Rock chorus")],
      ["fill", groovePattern("Rock fill")],
    ]);
    expect(next.sections.map((s) => s.name)).toEqual(["Verse", "Intro", "Chorus", "Fill"]);
    expect(next.sections[0]?.pattern.rows).toHaveLength(1);
    expect(next.order.map((o) => `${o.section}×${o.times}`)).toEqual([
      "Intro×1",
      "Verse×4",
      "Chorus×4",
      "Verse×4",
      "Chorus×4",
      "Chorus×4",
    ]);
    expect(next.sections[0]?.fill).toEqual({ section: "Fill", every: 4, atEnd: false });
    // An order already there is kept.
    expect(putGroove(part(), [["chorus", groovePattern("x")]]).order).toEqual(part().order);
  });
});
