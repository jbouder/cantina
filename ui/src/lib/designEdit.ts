// Edits to a drum part, as the drum designer makes them: pure functions
// from one part to the next, so the designer's own history can step
// back through them and tests can run them without the engine.

import type { DrumPart, PartSection } from "./drumPart";
import { uniqueName } from "./drumPart";
import type { PatternInfo } from "./engine";

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
