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
  strokeCells,
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
    expect(cells).toEqual([
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
});
