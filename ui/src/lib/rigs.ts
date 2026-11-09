// The rig picker's list: filtering and grouping the rigs the shell
// reports. Kept apart from the component so tests need no window.

import type { Instrument, RigInfo } from "./engine";

/** The rigs for an instrument, in a genre if one is given, matching a search. */
export function filterRigs(
  rigs: RigInfo[],
  instrument: Instrument,
  genre: string,
  query: string,
): RigInfo[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return rigs.filter((r) => {
    if (r.instrument !== instrument) return false;
    if (genre && r.genre !== genre) return false;
    const text = `${r.name} ${r.genre} ${r.description}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** The genres an instrument's rigs come in, in the order the list shows them. */
export function rigGenres(rigs: RigInfo[], instrument: Instrument): string[] {
  return [...new Set(rigs.filter((r) => r.instrument === instrument).map((r) => r.genre))]
    .filter(Boolean)
    .sort((a, b) => a.localeCompare(b));
}

/** A rig's chain in a few words: the pedals' kinds, cabinets by their impulse. */
export function chainSummary(rig: RigInfo): string {
  return rig.chain
    .map((spec) => {
      if (spec.startsWith("cab:")) return spec.slice(4).replace(/-.*/, "");
      if (spec.startsWith("pedal:")) return spec.slice(6);
      if (spec.startsWith("au:") || spec.startsWith("clap:")) return "plugin";
      return spec;
    })
    .join(" → ");
}
