// A drum part as the drum designer builds it: sections, each with its
// pattern, and an order that arranges them into a song. Mirrors
// `DrumPart` in cantina-project; pure functions only, so tests can run
// them without the engine.

import type { GrooveSection, PatternClipInfo, PatternInfo } from "./engine";

/** Times a step of the order can repeat, and bars apart a fill can come, at most. */
export const MAX_REPEAT = 64;

/** When a section plays a fill. Mirrors `SectionFill` in cantina-project. */
export interface SectionFill {
  /** The section whose first bar is the fill. */
  section: string;
  /** The last bar of every this many is a fill; 0 for none. */
  every: number;
  /** The section's last bar in the order is a fill. */
  atEnd: boolean;
}

/** A section of a part. Mirrors `PartSection` in cantina-project. */
export interface PartSection {
  /** Unique within the part; the order refers to the section by it. */
  name: string;
  /** Intro, verse, chorus…; absent for a section of your own. */
  kind?: GrooveSection | undefined;
  pattern: PatternInfo;
  fill?: SectionFill | undefined;
}

/** One step of the order. Mirrors `OrderStep` in cantina-project. */
export interface OrderStep {
  section: string;
  /** Passes of its pattern, 1 to MAX_REPEAT. */
  times: number;
}

/** A drum part. Mirrors `DrumPart` in cantina-project. */
export interface DrumPart {
  /** The kit, by id. */
  kit: string;
  sections: PartSection[];
  order: OrderStep[];
}

/** Where a step of the order falls, in ticks from the top. */
export interface Span {
  step: number;
  section: number;
  start: number;
  length: number;
}

const PPQ = 960;

/** Ticks in one pass of a pattern. */
export function patternTicks(p: Pick<PatternInfo, "stepsPerBeat" | "length">): number {
  return Math.floor(PPQ / Math.max(1, p.stepsPerBeat)) * Math.max(1, p.length);
}

/** Where each step of the order falls; steps naming no section are left out. */
export function layout(part: DrumPart): Span[] {
  let at = 0;
  const spans: Span[] = [];
  part.order.forEach((o, step) => {
    const section = part.sections.findIndex((s) => s.name === o.section);
    const found = part.sections[section];
    if (!found) return;
    const length = patternTicks(found.pattern) * Math.max(1, o.times);
    spans.push({ step, section, start: at, length });
    at += length;
  });
  return spans;
}

/** Ticks the whole order lasts. */
export function orderTicks(part: DrumPart): number {
  const last = layout(part).at(-1);
  return last ? last.start + last.length : 0;
}

const KINDS: [string, GrooveSection][] = [
  ["intro", "intro"],
  ["verse", "verse"],
  ["chorus", "chorus"],
  ["bridge", "bridge"],
  ["fill", "fill"],
  ["ending", "ending"],
  ["outro", "ending"],
];

/** The kind of section a name says, when it says one. Mirrors `kind_of`. */
export function kindOf(name: string): GrooveSection | undefined {
  const words = name.toLowerCase().split(/[^\p{L}\p{N}]+/u);
  return KINDS.find(([w]) => words.includes(w))?.[1];
}

/** `name`, or `name 2`, `name 3`… if it is taken. Mirrors `unique_name`. */
export function uniqueName(name: string, taken: string[]): string {
  const base = name.trim() || "Section";
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) {
    const candidate = `${base} ${n}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

/** An empty pattern of one bar of sixteenths. */
export function emptyPattern(name: string): PatternInfo {
  return { id: 1, name, stepsPerBeat: 4, length: 16, swing: 0, rows: [] };
}

/**
 * The part a drum track plays now, as the shell makes it when the
 * designer opens. Mirrors `DrumPart::from_track`.
 */
export function partFromTrack(
  kit: string,
  clips: PatternClipInfo[],
  patterns: PatternInfo[],
): DrumPart {
  const sorted = [...clips].sort((a, b) => a.start - b.start);
  const byId = new Map(patterns.map((p) => [p.id, p]));
  let used: number[] = [];
  for (const c of sorted) {
    for (const id of [c.pattern, c.fill?.pattern]) {
      if (id !== undefined && !used.includes(id) && byId.has(id)) used.push(id);
    }
  }
  if (used.length === 0) used = patterns.map((p) => p.id);
  const sections: PartSection[] = [];
  const nameOf = new Map<number, string>();
  for (const id of used) {
    const pattern = byId.get(id);
    if (!pattern) continue;
    const name = uniqueName(
      pattern.name,
      sections.map((s) => s.name),
    );
    nameOf.set(id, name);
    sections.push({ name, kind: kindOf(pattern.name), pattern: structuredClone(pattern) });
  }
  if (sections.length === 0) {
    sections.push({ name: "Verse", kind: "verse", pattern: emptyPattern("Verse") });
  }
  const order: OrderStep[] = [];
  for (const c of sorted) {
    const name = nameOf.get(c.pattern);
    if (!name) continue;
    const ticks = Math.max(1, patternTicks(byId.get(c.pattern) as PatternInfo));
    const times = Math.min(MAX_REPEAT, Math.max(1, Math.ceil(c.length / ticks)));
    const fill = c.fill ? nameOf.get(c.fill.pattern) : undefined;
    const section = sections.find((s) => s.name === name);
    if (fill && fill !== name && section && !section.fill) {
      section.fill = {
        section: fill,
        every: Math.min(MAX_REPEAT, c.fill?.every ?? 0),
        atEnd: c.fill?.atEnd ?? false,
      };
    }
    const last = order.at(-1);
    if (last && last.section === name && last.times + times <= MAX_REPEAT) last.times += times;
    else order.push({ section: name, times });
  }
  if (order.length === 0) {
    for (const s of sections) if (s.kind !== "fill") order.push({ section: s.name, times: 1 });
  }
  return { kit, sections, order };
}
