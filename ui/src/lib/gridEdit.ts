import type { PatternInfo } from "./engine";

/**
 * The drum grid's editing gestures, pure: each takes a pattern and gives
 * back the whole pattern as it should be after, which the grid sends as
 * one `ReplacePattern` command, so every gesture is one step of undo.
 * Rows are addressed by the order the grid shows the kit's pads.
 */

/** The most steps a pattern may have. */
export const MAX_STEPS = 128;

/** A rectangle of cells: rows `top..=bottom` of the grid, steps `from..=to`. */
export interface Region {
  top: number;
  bottom: number;
  from: number;
  to: number;
}

/** Copied cells: velocities by row, then step. */
export type Cells = number[][];

/** The region from one corner to the other, in either order. */
export function regionOf(
  a: { row: number; step: number },
  b: { row: number; step: number },
): Region {
  return {
    top: Math.min(a.row, b.row),
    bottom: Math.max(a.row, b.row),
    from: Math.min(a.step, b.step),
    to: Math.max(a.step, b.step),
  };
}

/** True when the cell is inside the region. */
export function inRegion(region: Region | null, row: number, step: number): boolean {
  return (
    region !== null &&
    row >= region.top &&
    row <= region.bottom &&
    step >= region.from &&
    step <= region.to
  );
}

/** The velocities of a pad, `length` long, zeros where the pattern has no row. */
function stepsOf(pattern: PatternInfo, pad: string, length = pattern.length): number[] {
  const row = pattern.rows.find((r) => r.pad === pad);
  return Array.from({ length }, (_, s) => row?.steps[s] ?? 0);
}

/** The pattern with `pad`'s steps replaced; a row is added when needed. */
function withRow(pattern: PatternInfo, pad: string, steps: number[]): PatternInfo {
  const has = pattern.rows.some((r) => r.pad === pad);
  const rows = has
    ? pattern.rows.map((r) => (r.pad === pad ? { pad, steps } : r))
    : [...pattern.rows, { pad, steps }];
  return { ...pattern, rows };
}

/** Sets cells to values, adding rows for pads that had none. */
export function setCells(
  pattern: PatternInfo,
  cells: { pad: string; step: number; value: number }[],
): PatternInfo {
  let out = pattern;
  for (const { pad, step, value } of cells) {
    if (step < 0 || step >= pattern.length) continue;
    const steps = stepsOf(out, pad);
    steps[step] = Math.max(0, Math.min(1, value));
    out = withRow(out, pad, steps);
  }
  return out;
}

/** The cells in a region, for copying. */
export function copyRegion(pattern: PatternInfo, pads: string[], region: Region): Cells {
  const out: Cells = [];
  for (let r = region.top; r <= region.bottom; r++) {
    const pad = pads[r];
    const steps = pad ? stepsOf(pattern, pad) : [];
    out.push(
      Array.from({ length: region.to - region.from + 1 }, (_, i) => steps[region.from + i] ?? 0),
    );
  }
  return out;
}

/** Writes copied cells with their top-left at `row`, `step`; what falls off the grid is dropped. */
export function pasteCells(
  pattern: PatternInfo,
  pads: string[],
  cells: Cells,
  row: number,
  step: number,
): PatternInfo {
  const writes: { pad: string; step: number; value: number }[] = [];
  cells.forEach((line, r) => {
    const pad = pads[row + r];
    if (!pad) return;
    line.forEach((value, i) => {
      if (step + i < pattern.length) writes.push({ pad, step: step + i, value });
    });
  });
  return setCells(pattern, writes);
}

/** Clears every cell in a region. */
export function clearRegion(pattern: PatternInfo, pads: string[], region: Region): PatternInfo {
  const empty = copyRegion(pattern, pads, region).map((line) => line.map(() => 0));
  return pasteCells(pattern, pads, empty, region.top, region.from);
}

/** Clears a pad's whole row. */
export function clearRow(pattern: PatternInfo, pad: string): PatternInfo {
  return withRow(
    pattern,
    pad,
    stepsOf(pattern, pad).map(() => 0),
  );
}

/**
 * Moves the cells of a region `by` steps, wrapping round inside it, so a
 * hit pushed off one end comes back at the other.
 */
export function shiftRegion(
  pattern: PatternInfo,
  pads: string[],
  region: Region,
  by: number,
): PatternInfo {
  const cells = copyRegion(pattern, pads, region);
  const width = region.to - region.from + 1;
  const turned = cells.map((line) =>
    line.map((_, i) => line[(((i - by) % width) + width) % width] ?? 0),
  );
  return pasteCells(pattern, pads, turned, region.top, region.from);
}

/**
 * Copies a region to just after itself. When it would run off the end,
 * the pattern grows to hold it, up to the most steps a pattern may have.
 */
export function duplicateRegion(pattern: PatternInfo, pads: string[], region: Region): PatternInfo {
  const width = region.to - region.from + 1;
  const need = region.to + 1 + width;
  const grown = need > pattern.length ? resize(pattern, Math.min(MAX_STEPS, need)) : pattern;
  return pasteCells(grown, pads, copyRegion(pattern, pads, region), region.top, region.to + 1);
}

/** The pattern at a new length: cut off, or padded with silence. */
function resize(pattern: PatternInfo, length: number): PatternInfo {
  return {
    ...pattern,
    length,
    rows: pattern.rows.map((r) => ({ pad: r.pad, steps: stepsOf(pattern, r.pad, length) })),
  };
}

/** Plays the pattern twice in a row: twice as long, the same hits again. Null when it would pass the limit. */
export function doublePattern(pattern: PatternInfo): PatternInfo | null {
  if (pattern.length * 2 > MAX_STEPS) return null;
  return {
    ...pattern,
    length: pattern.length * 2,
    rows: pattern.rows.map((r) => ({ pad: r.pad, steps: [...r.steps, ...r.steps] })),
  };
}

/** Keeps the first half. Null for a pattern of one step. */
export function halvePattern(pattern: PatternInfo): PatternInfo | null {
  if (pattern.length < 2) return null;
  return resize(pattern, Math.floor(pattern.length / 2));
}

/** The cells a paint stroke from one cell to another crosses, in a straight line. */
export function strokeCells(
  a: { row: number; step: number },
  b: { row: number; step: number },
): { row: number; step: number }[] {
  const n = Math.max(Math.abs(b.row - a.row), Math.abs(b.step - a.step));
  const out: { row: number; step: number }[] = [];
  for (let i = 0; i <= n; i++) {
    const t = n === 0 ? 0 : i / n;
    out.push({
      row: Math.round(a.row + (b.row - a.row) * t),
      step: Math.round(a.step + (b.step - a.step) * t),
    });
  }
  return out;
}
