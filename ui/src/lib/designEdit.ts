// Edits to a drum part, as the drum designer makes them: pure functions
// from one part to the next, so the designer's own history can step
// back through them and tests can run them without the engine.

import type { DrumPart, OrderStep, PartSection, SectionFill } from "./drumPart";
import { emptyPattern, MAX_REPEAT, uniqueName } from "./drumPart";
import type { GrooveSection, PatternInfo } from "./engine";
import { MAX_STEPS } from "./gridEdit";

/** The part with section `name`'s pattern replaced. */
export function withPattern(part: DrumPart, name: string, pattern: PatternInfo): DrumPart {
  return {
    ...part,
    sections: part.sections.map((s) => (s.name === name ? { ...s, pattern } : s)),
  };
}

/** The part with section `name` changed by `change`. */
export function withSection(
  part: DrumPart,
  name: string,
  change: (s: PartSection) => PartSection,
): DrumPart {
  return { ...part, sections: part.sections.map((s) => (s.name === name ? change(s) : s)) };
}

/**
 * The part with section `from` called `to`, made unique, and the order
 * and fills that name it following. Unchanged when `to` is blank.
 */
export function renameSection(part: DrumPart, from: string, to: string): DrumPart {
  if (!to.trim() || to.trim() === from) return part;
  const name = uniqueName(
    to,
    part.sections.map((s) => s.name).filter((n) => n !== from),
  );
  return {
    ...part,
    sections: part.sections.map((s) => ({
      ...s,
      name: s.name === from ? name : s.name,
      pattern: s.name === from ? { ...s.pattern, name } : s.pattern,
      fill: s.fill?.section === from ? { ...s.fill, section: name } : s.fill,
    })),
    order: part.order.map((o) => (o.section === from ? { ...o, section: name } : o)),
  };
}

/** The pads a grid shows: the kit's, then any the part's patterns use that the kit lacks. */
export function gridPads(
  kitPads: { id: string; name: string }[],
  pattern: PatternInfo,
): { id: string; name: string }[] {
  const extra = pattern.rows
    .filter((r) => !kitPads.some((p) => p.id === r.pad) && r.steps.some((v) => v > 0))
    .map((r) => ({ id: r.pad, name: r.pad }));
  return [...kitPads, ...extra];
}

/** The labels the kinds go by, in the order the designer offers them. */
export const SECTION_KINDS: { kind: GrooveSection; label: string }[] = [
  { kind: "intro", label: "Intro" },
  { kind: "verse", label: "Verse" },
  { kind: "chorus", label: "Chorus" },
  { kind: "bridge", label: "Bridge" },
  { kind: "fill", label: "Fill" },
  { kind: "ending", label: "Ending" },
];

/**
 * The part with a new section: of a kind, named after it, or of your
 * own, "Section". It starts as an empty pattern as long as the section
 * edited, or a fill's one bar. Returns the part and the new name.
 */
export function addSection(
  part: DrumPart,
  kind: GrooveSection | null,
  like: PatternInfo | null,
  barSteps: number,
): [DrumPart, string] {
  const label = SECTION_KINDS.find((k) => k.kind === kind)?.label ?? "Section";
  const name = uniqueName(
    label,
    part.sections.map((s) => s.name),
  );
  const base = like ?? emptyPattern(name);
  const length = kind === "fill" ? Math.max(1, barSteps) : base.length;
  const pattern: PatternInfo = {
    id: 1,
    name,
    stepsPerBeat: base.stepsPerBeat,
    length: Math.min(MAX_STEPS, length),
    swing: base.swing,
    rows: [],
  };
  const section: PartSection = { name, pattern, ...(kind ? { kind } : {}) };
  return [{ ...part, sections: [...part.sections, section] }, name];
}

/** The part with a copy of section `name` after it, as "Verse 2". Returns the part and the copy's name. */
export function duplicateSection(part: DrumPart, name: string): [DrumPart, string] {
  const i = part.sections.findIndex((s) => s.name === name);
  const source = part.sections[i];
  if (!source) return [part, name];
  const copy = uniqueName(
    name,
    part.sections.map((s) => s.name),
  );
  const sections = [...part.sections];
  sections.splice(i + 1, 0, {
    ...structuredClone(source),
    name: copy,
    pattern: { ...structuredClone(source.pattern), name: copy },
  });
  return [{ ...part, sections }, copy];
}

/**
 * The part without section `name`, its steps in the order and the fills
 * that named it. Unchanged when it is the only section.
 */
export function removeSection(part: DrumPart, name: string): DrumPart {
  if (part.sections.length < 2) return part;
  return {
    ...part,
    sections: part.sections
      .filter((s) => s.name !== name)
      .map((s) => {
        if (s.fill?.section !== name) return s;
        const { fill: _, ...rest } = s;
        return rest;
      }),
    order: part.order.filter((o) => o.section !== name),
  };
}

/** The part with section `name` played `times` at `at` in the order, or at its end. */
export function addStep(part: DrumPart, name: string, times = 1, at?: number): DrumPart {
  const order = [...part.order];
  order.splice(at ?? order.length, 0, { section: name, times: clampTimes(times) });
  return { ...part, order };
}

/** The part with step `i` of the order changed. */
export function setStep(part: DrumPart, i: number, step: Partial<OrderStep>): DrumPart {
  return {
    ...part,
    order: part.order.map((o, j) =>
      j === i ? { ...o, ...step, times: clampTimes(step.times ?? o.times) } : o,
    ),
  };
}

/** The part with step `i` moved `by` places, kept inside the order. */
export function moveStep(part: DrumPart, i: number, by: number): DrumPart {
  const to = Math.max(0, Math.min(part.order.length - 1, i + by));
  const step = part.order[i];
  if (!step || to === i) return part;
  const order = part.order.filter((_, j) => j !== i);
  order.splice(to, 0, step);
  return { ...part, order };
}

/** The part without step `i`. */
export function removeStep(part: DrumPart, i: number): DrumPart {
  return { ...part, order: part.order.filter((_, j) => j !== i) };
}

/** The part with section `name`'s fill set, or taken off with null. */
export function setFill(part: DrumPart, name: string, fill: SectionFill | null): DrumPart {
  return withSection(part, name, (s) => {
    const { fill: _, ...rest } = s;
    return fill && fill.section !== name ? { ...rest, fill } : rest;
  });
}

function clampTimes(times: number): number {
  return Math.max(1, Math.min(MAX_REPEAT, Math.round(times)));
}

/**
 * The part with section `name` playing `pattern` as its A, renamed to
 * the section. Its variations go when they no longer have A's steps.
 */
export function putInSection(part: DrumPart, name: string, pattern: PatternInfo): DrumPart {
  return withSection(part, name, (s) => {
    const fits = (v: PatternInfo) =>
      v.length === pattern.length && v.stepsPerBeat === pattern.stepsPerBeat;
    const variations = (s.variations ?? []).filter(fits);
    const { variations: _, ...rest } = s;
    return {
      ...rest,
      pattern: { ...structuredClone(pattern), id: 1, name },
      ...(variations.length > 0 ? { variations } : {}),
    };
  });
}

/** The order a whole groove starts with, by kind and passes, for those the part has. */
const SONG: [GrooveSection, number][] = [
  ["intro", 1],
  ["verse", 4],
  ["chorus", 4],
  ["verse", 4],
  ["chorus", 4],
  ["bridge", 2],
  ["chorus", 4],
  ["ending", 1],
];

/**
 * The part with a whole groove in it, a pattern per kind of section: each
 * goes into the part's first section of its kind, or a new one named
 * after the kind. An empty order becomes a song of what is there, and a
 * verse or chorus with no fill takes the groove's fill every four bars.
 */
export function putGroove(part: DrumPart, groove: [GrooveSection, PatternInfo][]): DrumPart {
  let out = part;
  for (const [kind, pattern] of groove) {
    const there = out.sections.find((s) => s.kind === kind);
    if (there) {
      out = putInSection(out, there.name, pattern);
    } else {
      const label = SECTION_KINDS.find((k) => k.kind === kind)?.label ?? "Section";
      const name = uniqueName(
        label,
        out.sections.map((s) => s.name),
      );
      out = {
        ...out,
        sections: [
          ...out.sections,
          { name, kind, pattern: { ...structuredClone(pattern), id: 1, name } },
        ],
      };
    }
  }
  const named = (kind: GrooveSection) => out.sections.find((s) => s.kind === kind)?.name;
  if (out.order.length === 0) {
    const order: OrderStep[] = [];
    for (const [kind, times] of SONG) {
      const name = named(kind);
      if (name) order.push({ section: name, times });
    }
    out = { ...out, order };
  }
  const fill = groove.some(([k]) => k === "fill") ? named("fill") : undefined;
  if (fill) {
    for (const kind of ["verse", "chorus"] as const) {
      const name = named(kind);
      const s = out.sections.find((x) => x.name === name);
      if (name && s && !s.fill) out = setFill(out, name, { section: fill, every: 4, atEnd: false });
    }
  }
  return out;
}
