// The drum designer's pattern tools, pure: ghost notes and accents,
// drawing velocities, whole bars copied, pasted, duplicated and cleared,
// shifting, and a section's A to D variations. Each takes a pattern (or
// a section) and gives back the whole of it as it should be after.

import type { PartSection } from "./drumPart";
import type { PatternInfo } from "./engine";
import { type Cells, cellsOf, MAX_STEPS, withCells } from "./gridEdit";

/** How hard a ghost note is played. */
export const GHOST = 0.3;
/** How hard an accent is played. */
export const ACCENT = 1;
/** The softest a drawn velocity goes; below it a hit would vanish. */
export const MIN_VELOCITY = 0.05;

/** Every pad the pattern has a row for, then `pads` it lacks. */
function allPads(pattern: PatternInfo, pads: string[]): string[] {
  return [...new Set([...pads, ...pattern.rows.map((r) => r.pad)])];
}

/** The pattern with the cell at `pad`, `step` played at `value`: a hit added if there was none. */
export function strike(
  pattern: PatternInfo,
  pad: string,
  step: number,
  value: number,
): PatternInfo {
  if (step < 0 || step >= pattern.length) return pattern;
  const row = cellsOf(pattern, pad);
  const was = row[step];
  row[step] = { value: Math.max(0, Math.min(1, value)), mark: was?.mark ?? null };
  return withCells(pattern, pad, row);
}

/** The hit at `pad`, `step` made `by` harder (or softer), never below the softest. Unchanged without a hit. */
export function nudgeVelocity(
  pattern: PatternInfo,
  pad: string,
  step: number,
  by: number,
): PatternInfo {
  const v = cellsOf(pattern, pad)[step]?.value ?? 0;
  if (v <= 0) return pattern;
  return strike(pattern, pad, step, Math.max(MIN_VELOCITY, Math.round((v + by) * 100) / 100));
}

/**
 * The hit at `pad`, `step` drawn to `value` by the velocity lane: only a
 * hit changes, and it never drops below the softest.
 */
export function drawVelocity(
  pattern: PatternInfo,
  pad: string,
  step: number,
  value: number,
): PatternInfo {
  if ((cellsOf(pattern, pad)[step]?.value ?? 0) <= 0) return pattern;
  return strike(pattern, pad, step, Math.max(MIN_VELOCITY, Math.min(1, value)));
}

/** Steps in a bar of the pattern, at `numerator` beats a bar. */
export function barSteps(pattern: PatternInfo, numerator: number): number {
  return Math.max(1, pattern.stepsPerBeat * Math.max(1, numerator));
}

/** Bars the pattern lasts, a part bar counting as one. */
export function barCount(pattern: PatternInfo, numerator: number): number {
  return Math.max(1, Math.ceil(pattern.length / barSteps(pattern, numerator)));
}

/** Bar `bar`'s cells, every pad, for pasting. */
export function copyBar(
  pattern: PatternInfo,
  pads: string[],
  bar: number,
  numerator: number,
): { pads: string[]; cells: Cells } {
  const per = barSteps(pattern, numerator);
  const all = allPads(pattern, pads);
  return {
    pads: all,
    cells: all.map((pad) => cellsOf(pattern, pad).slice(bar * per, (bar + 1) * per)),
  };
}

/** The pattern with copied cells written over bar `bar`; what runs past the end is dropped. */
export function pasteBar(
  pattern: PatternInfo,
  copied: { pads: string[]; cells: Cells },
  bar: number,
  numerator: number,
): PatternInfo {
  const per = barSteps(pattern, numerator);
  let out = pattern;
  copied.pads.forEach((pad, i) => {
    const row = cellsOf(out, pad);
    (copied.cells[i] ?? []).forEach((c, j) => {
      const at = bar * per + j;
      if (at < row.length) row[at] = { ...c };
    });
    out = withCells(out, pad, row);
  });
  return out;
}

/** The pattern with bar `bar` silent. */
export function clearBar(pattern: PatternInfo, bar: number, numerator: number): PatternInfo {
  const per = barSteps(pattern, numerator);
  let out = pattern;
  for (const r of pattern.rows) {
    const row = cellsOf(out, r.pad).map((c, s) =>
      s >= bar * per && s < (bar + 1) * per ? { value: 0, mark: null } : c,
    );
    out = withCells(out, r.pad, row);
  }
  return out;
}

/**
 * The pattern with bar `bar` played twice: a copy goes in after it and
 * what followed moves a bar later. Null when the pattern would pass the
 * most steps it may have.
 */
export function duplicateBar(
  pattern: PatternInfo,
  bar: number,
  numerator: number,
): PatternInfo | null {
  const per = barSteps(pattern, numerator);
  const length = pattern.length + per;
  if (length > MAX_STEPS) return null;
  let out: PatternInfo = { ...pattern, length };
  for (const r of pattern.rows) {
    const cells = cellsOf(pattern, r.pad);
    const end = Math.min(cells.length, (bar + 1) * per);
    const copy = cells.slice(bar * per, end).map((c) => ({ ...c }));
    out = withCells(out, r.pad, [...cells.slice(0, end), ...copy, ...cells.slice(end)]);
  }
  return out;
}

/** Every row of the pattern turned `by` steps later (earlier when negative), wrapping round. */
export function shiftSteps(pattern: PatternInfo, by: number): PatternInfo {
  const n = pattern.length;
  let out = pattern;
  for (const r of pattern.rows) {
    const cells = cellsOf(pattern, r.pad);
    out = withCells(
      out,
      r.pad,
      cells.map((_, i) => cells[(((i - by) % n) + n) % n] ?? { value: 0, mark: null }),
    );
  }
  return out;
}

/** The letters a section's patterns go by: A, then the variations. */
export const VARIATIONS = ["A", "B", "C", "D"] as const;

/** Which pattern each pass plays, by index, round and round. Mirrors `PartSection::passes`. */
export function passes(section: Pick<PartSection, "variations" | "vary">): number[] {
  const n = Math.min(3, section.variations?.length ?? 0);
  if (n === 0) return [0];
  if (section.vary === "chain") return Array.from({ length: n + 1 }, (_, i) => i);
  return Array.from({ length: n }, (_, i) => [0, i + 1]).flat();
}

/** Pattern `i` of a section: 0 is A. */
export function variationOf(section: PartSection, i: number): PatternInfo {
  return i === 0 ? section.pattern : (section.variations?.[i - 1] ?? section.pattern);
}

/** The section with pattern `i` (0 for A) replaced. */
export function withVariation(section: PartSection, i: number, pattern: PatternInfo): PartSection {
  if (i === 0) return { ...section, pattern };
  const variations = [...(section.variations ?? [])];
  variations[i - 1] = pattern;
  return { ...section, variations };
}

/** The section with a new variation, a copy of pattern `from`. Unchanged at D. */
export function addVariation(section: PartSection, from: number): PartSection {
  const variations = section.variations ?? [];
  if (variations.length >= 3) return section;
  const copy = structuredClone(variationOf(section, from));
  const letter = VARIATIONS[variations.length + 1] ?? "D";
  return {
    ...section,
    variations: [...variations, { ...copy, name: `${section.name} ${letter}` }],
  };
}

/** The section without variation `i` (1 to 3); the later ones move up a letter. */
export function removeVariation(section: PartSection, i: number): PartSection {
  if (i < 1) return section;
  const variations = (section.variations ?? []).filter((_, j) => j !== i - 1);
  const { variations: _, vary: __, ...rest } = section;
  return variations.length > 0
    ? { ...rest, variations, ...(section.vary ? { vary: section.vary } : {}) }
    : rest;
}

/** The section with every pattern, A to D, changed alike: a length or a feel set on all. */
export function everyVariation(
  section: PartSection,
  change: (p: PatternInfo) => PatternInfo,
): PartSection {
  return {
    ...section,
    pattern: change(section.pattern),
    ...(section.variations ? { variations: section.variations.map(change) } : {}),
  };
}
