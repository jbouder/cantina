import { describe, expect, it } from "vitest";
import type { PatternInfo } from "./engine";
import {
  clearRegion,
  clearRow,
  copyRegion,
  doublePattern,
  duplicateRegion,
  halvePattern,
  inRegion,
  pasteCells,
  regionOf,
  setCells,
  shiftRegion,
  stepMark,
  strokeCells,
  withPatternFeel,
  withRowFeel,
} from "./gridEdit";

const pads = ["kick", "snare", "hh-closed"];

function beat(): PatternInfo {
  return {
    id: 1,
    name: "Beat",
    stepsPerBeat: 4,
    length: 8,
    swing: 0,
    rows: [
      { pad: "kick", steps: [1, 0, 0, 0, 1, 0, 0, 0] },
      { pad: "snare", steps: [0, 0, 1, 0, 0, 0, 1, 0] },
    ],
  };
}

const row = (p: PatternInfo, pad: string) => p.rows.find((r) => r.pad === pad)?.steps;

describe("the grid's gestures", () => {
  it("paints and erases cells, adding a row for a new pad", () => {
    const p = setCells(beat(), [
      { pad: "hh-closed", step: 0, value: 0.8 },
      { pad: "hh-closed", step: 2, value: 0.8 },
      { pad: "kick", step: 4, value: 0 },
      { pad: "kick", step: 99, value: 1 },
    ]);
    expect(row(p, "hh-closed")).toEqual([0.8, 0, 0.8, 0, 0, 0, 0, 0]);
    expect(row(p, "kick")).toEqual([1, 0, 0, 0, 0, 0, 0, 0]);
  });

  it("strokes cross every cell between two points", () => {
    expect(strokeCells({ row: 0, step: 0 }, { row: 0, step: 3 })).toHaveLength(4);
    expect(strokeCells({ row: 0, step: 0 }, { row: 2, step: 2 })).toEqual([
      { row: 0, step: 0 },
      { row: 1, step: 1 },
      { row: 2, step: 2 },
    ]);
  });

  it("copies and pastes a region, dropping what falls off the grid", () => {
    const region = regionOf({ row: 1, step: 3 }, { row: 0, step: 0 });
    expect(region).toEqual({ top: 0, bottom: 1, from: 0, to: 3 });
    expect(inRegion(region, 1, 2)).toBe(true);
    expect(inRegion(region, 2, 2)).toBe(false);
    const cells = copyRegion(beat(), pads, region);
    expect(cells.map((line) => line.map((c) => c.value))).toEqual([
      [1, 0, 0, 0],
      [0, 0, 1, 0],
    ]);
    const p = pasteCells(beat(), pads, cells, 1, 6);
    expect(row(p, "snare")).toEqual([0, 0, 1, 0, 0, 0, 1, 0]);
    expect(row(p, "hh-closed")).toEqual([0, 0, 0, 0, 0, 0, 0, 0]);
  });

  it("clears a region and a row", () => {
    const p = clearRegion(beat(), pads, { top: 0, bottom: 1, from: 0, to: 3 });
    expect(row(p, "kick")).toEqual([0, 0, 0, 0, 1, 0, 0, 0]);
    expect(row(p, "snare")).toEqual([0, 0, 0, 0, 0, 0, 1, 0]);
    expect(row(clearRow(beat(), "snare"), "snare")).toEqual(Array(8).fill(0));
  });

  it("shifts a region round inside itself", () => {
    const right = shiftRegion(beat(), pads, { top: 0, bottom: 0, from: 0, to: 7 }, 1);
    expect(row(right, "kick")).toEqual([0, 1, 0, 0, 0, 1, 0, 0]);
    const left = shiftRegion(beat(), pads, { top: 1, bottom: 1, from: 0, to: 3 }, -3);
    expect(row(left, "snare")).toEqual([0, 0, 0, 1, 0, 0, 1, 0]);
  });

  it("duplicates a region after itself, growing the pattern when it must", () => {
    const p = duplicateRegion(beat(), pads, { top: 0, bottom: 1, from: 0, to: 3 });
    expect(p.length).toBe(8);
    expect(row(p, "snare")).toEqual([0, 0, 1, 0, 0, 0, 1, 0]);
    const grown = duplicateRegion(beat(), pads, { top: 0, bottom: 1, from: 0, to: 7 });
    expect(grown.length).toBe(16);
    expect(row(grown, "kick")).toEqual([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
  });

  it("doubles and halves, within the limits", () => {
    expect(doublePattern(beat())?.length).toBe(16);
    expect(row(doublePattern(beat()) as PatternInfo, "snare")?.slice(8)).toEqual(
      row(beat(), "snare"),
    );
    expect(doublePattern({ ...beat(), length: 128, rows: [] })).toBeNull();
    expect(row(halvePattern(beat()) as PatternInfo, "kick")).toEqual([1, 0, 0, 0]);
    expect(halvePattern({ ...beat(), length: 1, rows: [] })).toBeNull();
  });

  it("steps marks on hits, and marks travel with their cells", () => {
    const flam = stepMark(beat(), [{ pad: "snare", step: 2 }], "flam") as PatternInfo;
    expect(flam.rows.find((r) => r.pad === "snare")?.marks).toEqual([{ step: 2, flam: true }]);
    // Ratchets go 1, 2, 3, 4, 1; chance 100, 75, 50, 25, 100.
    let p = beat();
    const counts: (number | undefined)[] = [];
    for (let i = 0; i < 4; i++) {
      p = stepMark(p, [{ pad: "kick", step: 0 }], "ratchet") as PatternInfo;
      counts.push(p.rows[0]?.marks?.[0]?.ratchet);
    }
    expect(counts).toEqual([2, 3, 4, undefined]);
    expect(stepMark(beat(), [{ pad: "kick", step: 1 }], "chance")).toBeNull();
    // Shifting carries the mark; erasing the hit drops it.
    const moved = shiftRegion(flam, pads, { top: 1, bottom: 1, from: 0, to: 7 }, 1);
    expect(moved.rows.find((r) => r.pad === "snare")?.marks).toEqual([{ step: 3, flam: true }]);
    const erased = setCells(flam, [{ pad: "snare", step: 2, value: 0 }]);
    expect(erased.rows.find((r) => r.pad === "snare")?.marks).toBeUndefined();
    // Doubling copies marks into the second half.
    expect(doublePattern(flam)?.rows.find((r) => r.pad === "snare")?.marks).toEqual([
      { step: 2, flam: true },
      { step: 10, flam: true },
    ]);
  });

  it("sets a pattern's and a row's feel", () => {
    const p = withPatternFeel(beat(), { timing: 0.4, velocity: 0.2 }, 9);
    expect(p.humanize).toEqual({ timing: 0.4, velocity: 0.2 });
    expect(p.seed).toBe(9);
    expect(withPatternFeel(p, null, 0).humanize).toBeUndefined();
    const r = withRowFeel(beat(), "snare", { swing: 0.5 });
    expect(r.rows.find((x) => x.pad === "snare")?.swing).toBe(0.5);
    expect(
      withRowFeel(r, "snare", { swing: null }).rows.find((x) => x.pad === "snare")?.swing,
    ).toBeUndefined();
  });
});
