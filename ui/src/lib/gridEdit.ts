import type { Humanize, PatternInfo, PatternRow, StepMark } from "./engine";

/**
 * The drum grid's editing gestures, pure: each takes a pattern and gives
 * back the whole pattern as it should be after, which the grid sends as
 * one `ReplacePattern` command, so every gesture is one step of undo.
 * Rows are addressed by the order the grid shows the kit's pads. A step's
 * mark (chance, ratchet, flam) travels with its velocity, and goes when
 * the hit is cleared.
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

/** A cell as copied: its velocity and its mark, if any (with `step` relative). */
export interface Cell {
  value: number;
  mark: Omit<StepMark, "step"> | null;
}

/** Copied cells: by row, then step. */
export type Cells = Cell[][];

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

/** True for a mark that changes nothing. */
function plain(m: Omit<StepMark, "step">): boolean {
  return (m.chance ?? 1) === 1 && (m.ratchet ?? 1) === 1 && !m.flam;
}

/** A pad's row as cells, `length` long. */
function cellsOf(pattern: PatternInfo, pad: string, length = pattern.length): Cell[] {
  const row = pattern.rows.find((r) => r.pad === pad);
  return Array.from({ length }, (_, s) => {
    const m = row?.marks?.find((x) => x.step === s);
    if (!m) return { value: row?.steps[s] ?? 0, mark: null };
    const { step: _step, ...mark } = m;
    return { value: row?.steps[s] ?? 0, mark: plain(mark) ? null : mark };
  });
}

/** The pattern with `pad`'s cells replaced, keeping the row's own feel; a row is added when needed. */
function withCells(pattern: PatternInfo, pad: string, cells: Cell[]): PatternInfo {
  const old = pattern.rows.find((r) => r.pad === pad);
  const marks: StepMark[] = [];
  cells.forEach((c, step) => {
    if (c.value > 0 && c.mark && !plain(c.mark)) marks.push({ step, ...c.mark });
  });
  const row: PatternRow = { ...(old ?? { pad, steps: [] }), pad, steps: cells.map((c) => c.value) };
  if (marks.length > 0) row.marks = marks;
  else delete row.marks;
  const rows = old ? pattern.rows.map((r) => (r.pad === pad ? row : r)) : [...pattern.rows, row];
  return { ...pattern, rows };
}

/** Sets cells to values, adding rows for pads that had none. An erased hit loses its mark. */
export function setCells(
  pattern: PatternInfo,
  cells: { pad: string; step: number; value: number }[],
): PatternInfo {
  let out = pattern;
  for (const { pad, step, value } of cells) {
    if (step < 0 || step >= pattern.length) continue;
    const row = cellsOf(out, pad);
    const v = Math.max(0, Math.min(1, value));
    row[step] = { value: v, mark: v > 0 ? (row[step]?.mark ?? null) : null };
    out = withCells(out, pad, row);
  }
  return out;
}

/** The cells in a region, for copying. */
export function copyRegion(pattern: PatternInfo, pads: string[], region: Region): Cells {
  const out: Cells = [];
  for (let r = region.top; r <= region.bottom; r++) {
    const pad = pads[r];
    const row = pad ? cellsOf(pattern, pad) : [];
    out.push(
      Array.from(
        { length: region.to - region.from + 1 },
        (_, i) => row[region.from + i] ?? { value: 0, mark: null },
      ),
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
  let out = pattern;
  cells.forEach((line, r) => {
    const pad = pads[row + r];
    if (!pad) return;
    const target = cellsOf(out, pad);
    line.forEach((cell, i) => {
      if (step + i < pattern.length) target[step + i] = { ...cell };
    });
    out = withCells(out, pad, target);
  });
  return out;
}

/** Clears every cell in a region. */
export function clearRegion(pattern: PatternInfo, pads: string[], region: Region): PatternInfo {
  const empty = copyRegion(pattern, pads, region).map((line) =>
    line.map(() => ({ value: 0, mark: null })),
  );
  return pasteCells(pattern, pads, empty, region.top, region.from);
}

/** Clears a pad's whole row. */
export function clearRow(pattern: PatternInfo, pad: string): PatternInfo {
  return withCells(
    pattern,
    pad,
    cellsOf(pattern, pad).map(() => ({ value: 0, mark: null })),
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
    line.map((_, i) => line[(((i - by) % width) + width) % width] ?? { value: 0, mark: null }),
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
  let out: PatternInfo = { ...pattern, length };
  for (const r of pattern.rows) out = withCells(out, r.pad, cellsOf(pattern, r.pad, length));
  return out;
}

/** Plays the pattern twice in a row: twice as long, the same hits again. Null when it would pass the limit. */
export function doublePattern(pattern: PatternInfo): PatternInfo | null {
  if (pattern.length * 2 > MAX_STEPS) return null;
  let out: PatternInfo = { ...pattern, length: pattern.length * 2 };
  for (const r of pattern.rows) {
    const cells = cellsOf(pattern, r.pad);
    out = withCells(out, r.pad, [...cells, ...cells.map((c) => ({ ...c }))]);
  }
  return out;
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

// ----- feel -----

/** The chances a step steps through: always, then less and less. */
export const CHANCES = [1, 0.75, 0.5, 0.25];

/** What a key changes on a step's mark. */
export type MarkKind = "chance" | "ratchet" | "flam";

/**
 * Steps a mark on the hits at `cells`: chance through 100, 75, 50 and 25%,
 * a ratchet through 1 to 4 hits, a flam on and off. Every cell takes the
 * first cell's next value. Empty cells are left alone. Null when none of
 * the cells has a hit.
 */
export function stepMark(
  pattern: PatternInfo,
  cells: { pad: string; step: number }[],
  kind: MarkKind,
): PatternInfo | null {
  const hits = cells.filter((c) => (cellsOf(pattern, c.pad)[c.step]?.value ?? 0) > 0);
  const first = hits[0];
  if (!first) return null;
  const was = cellsOf(pattern, first.pad)[first.step]?.mark ?? {};
  const next =
    kind === "chance"
      ? { chance: CHANCES[(CHANCES.indexOf(was.chance ?? 1) + 1) % CHANCES.length] ?? 1 }
      : kind === "ratchet"
        ? { ratchet: ((was.ratchet ?? 1) % 4) + 1 }
        : { flam: !was.flam };
  let out = pattern;
  for (const c of hits) {
    const row = cellsOf(out, c.pad);
    const cell = row[c.step];
    if (!cell) continue;
    row[c.step] = { value: cell.value, mark: { ...(cell.mark ?? {}), ...next } };
    out = withCells(out, c.pad, row);
  }
  return out;
}

/** The pattern with its own humanize (null for on the grid) and seed. */
export function withPatternFeel(
  pattern: PatternInfo,
  humanize: Humanize | null,
  seed: number = pattern.seed ?? 0,
): PatternInfo {
  const out: PatternInfo = { ...pattern, seed };
  if (humanize && (humanize.timing > 0 || humanize.velocity > 0)) out.humanize = humanize;
  else delete out.humanize;
  if (!seed) delete out.seed;
  return out;
}

/** A row with its own swing and humanize, or (null) the pattern's. */
export function withRowFeel(
  pattern: PatternInfo,
  pad: string,
  feel: { swing?: number | null; humanize?: Humanize | null },
): PatternInfo {
  const has = pattern.rows.find((r) => r.pad === pad);
  const row: PatternRow = has ? { ...has } : { pad, steps: Array(pattern.length).fill(0) };
  if (feel.swing !== undefined) {
    if (feel.swing === null) delete row.swing;
    else row.swing = feel.swing;
  }
  if (feel.humanize !== undefined) {
    if (feel.humanize === null) delete row.humanize;
    else row.humanize = feel.humanize;
  }
  const rows = has ? pattern.rows.map((r) => (r.pad === pad ? row : r)) : [...pattern.rows, row];
  return { ...pattern, rows };
}
