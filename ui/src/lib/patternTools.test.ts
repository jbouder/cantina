import { describe, expect, it } from "vitest";
import type { PartSection } from "./drumPart";
import type { PatternInfo } from "./engine";
import {
  addVariation,
  barCount,
  clearBar,
  copyBar,
  drawVelocity,
  duplicateBar,
  everyVariation,
  GHOST,
  nudgeVelocity,
  passes,
  pasteBar,
  removeVariation,
  shiftSteps,
  strike,
  withVariation,
} from "./patternTools";

/** Two bars of quarter notes in 4/4: a kick on each bar's 1, a snare on bar 2's 3. */
function beat(): PatternInfo {
  return {
    id: 1,
    name: "Verse",
    stepsPerBeat: 1,
    length: 8,
    swing: 0,
    rows: [
      { pad: "kick", steps: [1, 0, 0, 0, 0.8, 0, 0, 0], marks: [{ step: 4, flam: true }] },
      { pad: "snare", steps: [0, 0, 0, 0, 0, 0, 1, 0] },
    ],
  };
}

const row = (p: PatternInfo, pad: string) => p.rows.find((r) => r.pad === pad)?.steps;

describe("velocities", () => {
  it("ghost and accent a step, adding the hit", () => {
    const p = strike(beat(), "snare", 2, GHOST);
    expect(row(p, "snare")?.[2]).toBe(GHOST);
  });
  it("are nudged without vanishing, and only on a hit", () => {
    expect(row(nudgeVelocity(beat(), "kick", 0, -2), "kick")?.[0]).toBe(0.05);
    expect(nudgeVelocity(beat(), "kick", 1, 0.1)).toEqual(beat());
  });
  it("are drawn on hits alone, keeping their marks", () => {
    const p = drawVelocity(beat(), "kick", 4, 0.4);
    expect(row(p, "kick")?.[4]).toBe(0.4);
    expect(p.rows[0]?.marks).toEqual([{ step: 4, flam: true }]);
    expect(drawVelocity(beat(), "kick", 1, 0.4)).toEqual(beat());
    expect(row(drawVelocity(beat(), "kick", 0, 0), "kick")?.[0]).toBe(0.05);
  });
});

describe("bars", () => {
  it("are counted, copied and pasted, every pad", () => {
    expect(barCount(beat(), 4)).toBe(2);
    const copied = copyBar(beat(), ["kick", "snare", "hat"], 1, 4);
    const p = pasteBar(beat(), copied, 0, 4);
    expect(row(p, "kick")).toEqual([0.8, 0, 0, 0, 0.8, 0, 0, 0]);
    expect(row(p, "snare")).toEqual([0, 0, 1, 0, 0, 0, 1, 0]);
    expect(p.rows.find((r) => r.pad === "kick")?.marks).toEqual([
      { step: 0, flam: true },
      { step: 4, flam: true },
    ]);
  });
  it("are cleared", () => {
    expect(row(clearBar(beat(), 1, 4), "kick")).toEqual([1, 0, 0, 0, 0, 0, 0, 0]);
  });
  it("are duplicated in place, pushing the rest later, within the limit", () => {
    const p = duplicateBar(beat(), 0, 4);
    expect(p?.length).toBe(12);
    expect(row(p as PatternInfo, "kick")).toEqual([1, 0, 0, 0, 1, 0, 0, 0, 0.8, 0, 0, 0]);
    expect(p?.rows[0]?.marks).toEqual([{ step: 8, flam: true }]);
    expect(duplicateBar({ ...beat(), length: 128 }, 0, 4)).toBeNull();
  });
  it("shift round", () => {
    expect(row(shiftSteps(beat(), 1), "kick")).toEqual([0, 1, 0, 0, 0, 0.8, 0, 0]);
    expect(row(shiftSteps(beat(), -1), "snare")).toEqual([0, 0, 0, 0, 0, 1, 0, 0]);
  });
});

describe("variations", () => {
  const section = (): PartSection => ({ name: "Verse", pattern: beat() });
  it("take turns, alternating by default or in a chain", () => {
    expect(passes(section())).toEqual([0]);
    let s = addVariation(addVariation(section(), 0), 0);
    expect(s.variations?.map((v) => v.name)).toEqual(["Verse B", "Verse C"]);
    expect(passes(s)).toEqual([0, 1, 0, 2]);
    s = { ...s, vary: "chain" };
    expect(passes(s)).toEqual([0, 1, 2]);
  });
  it("stop at D, and close up when one goes", () => {
    let s = section();
    for (let i = 0; i < 5; i++) s = addVariation(s, 0);
    expect(s.variations).toHaveLength(3);
    s = withVariation(s, 2, { ...beat(), name: "Fill-ish" });
    s = removeVariation(s, 1);
    expect(s.variations?.map((v) => v.name)).toEqual(["Fill-ish", "Verse D"]);
    expect(removeVariation(removeVariation(s, 1), 1).variations).toBeUndefined();
  });
  it("change alike when the length or feel does", () => {
    const s = everyVariation(addVariation(section(), 0), (p) => ({ ...p, swing: 0.5 }));
    expect([s.pattern.swing, s.variations?.[0]?.swing]).toEqual([0.5, 0.5]);
  });
});
