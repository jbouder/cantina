import { describe, expect, it } from "vitest";
import { dropIndex, rowDropIndex } from "./reorder";

// Three 100 px cards with 10 px gaps: centers at 50, 160 and 270.
const row = [
  { left: 0, right: 100 },
  { left: 110, right: 210 },
  { left: 220, right: 320 },
];

describe("dropIndex", () => {
  it("picks the card under the pointer", () => {
    expect(dropIndex(30, row)).toBe(0);
    expect(dropIndex(150, row)).toBe(1);
    expect(dropIndex(300, row)).toBe(2);
  });

  it("picks the nearer card in a gap", () => {
    expect(dropIndex(104, row)).toBe(0);
    expect(dropIndex(106, row)).toBe(1);
  });

  it("clamps past either end", () => {
    expect(dropIndex(-500, row)).toBe(0);
    expect(dropIndex(5000, row)).toBe(2);
  });

  it("has nowhere to land in an empty row", () => {
    expect(dropIndex(10, [])).toBe(-1);
  });
});

describe("a dragged row", () => {
  it("lands on the nearest row, past the middle of the next", () => {
    expect(rowDropIndex(1, 0, 60, 4)).toBe(1);
    expect(rowDropIndex(1, 29, 60, 4)).toBe(1);
    expect(rowDropIndex(1, 31, 60, 4)).toBe(2);
    expect(rowDropIndex(1, -31, 60, 4)).toBe(0);
  });

  it("stays in the list", () => {
    expect(rowDropIndex(0, -500, 60, 4)).toBe(0);
    expect(rowDropIndex(3, 500, 60, 4)).toBe(3);
    expect(rowDropIndex(0, 0, 60, 0)).toBe(-1);
  });
});
