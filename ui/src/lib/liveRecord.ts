import { patternTicks, stepTicks, ticksAt } from "./drums";
import type { PatternInfo, Tempo } from "./engine";
import { setCells } from "./gridEdit";

/**
 * Playing the kit live and recording it into a pattern, pure. The grid
 * auditions the pattern from tick 0 while recording, so a moment of the
 * transport is a step of the pattern and a pass of it.
 */

/** Keys that play the kit's pads, in the kit's order: the home row, then the row below. */
export const PAD_KEYS = [
  "a",
  "s",
  "d",
  "f",
  "g",
  "h",
  "j",
  "k",
  "l",
  ";",
  "z",
  "x",
  "c",
  "v",
  "b",
  "n",
];

/** How hard a key plays a pad; with ⇧, an accent at full. */
export const KEY_VELOCITY = 0.8;

/** The pad a key plays, by index into the kit's pads; null for none. */
export function padForKey(key: string): number | null {
  const i = PAD_KEYS.indexOf(key.toLowerCase());
  return i < 0 ? null : i;
}

/** Where in the pattern a moment of the transport falls: which pass, and the nearest step. */
export function placeOf(
  seconds: number,
  tempo: Tempo,
  pattern: Pick<PatternInfo, "stepsPerBeat" | "length">,
): { pass: number; step: number } {
  const ticks = ticksAt(Math.max(0, seconds), tempo);
  const cycle = patternTicks(pattern);
  const pass = Math.floor(ticks / cycle);
  // Quantized to the nearest step; a hit just before the end of a pass
  // belongs to the first step of the next one.
  const nearest = Math.round((ticks - pass * cycle) / stepTicks(pattern));
  if (nearest >= pattern.length) return { pass: pass + 1, step: 0 };
  return { pass, step: nearest };
}

/** The hits of one pass: velocity by "pad:step", the hardest kept. */
export type Take = Map<string, { pad: string; step: number; value: number }>;

/** Adds a hit to a take. */
export function addHit(take: Take, pad: string, step: number, value: number): Take {
  const key = `${pad}:${step}`;
  const was = take.get(key)?.value ?? 0;
  const next = new Map(take);
  next.set(key, { pad, step, value: Math.max(was, value) });
  return next;
}

/**
 * The pattern with a take overdubbed: each recorded hit is kept at the
 * louder of what was there and what was played, nothing is taken away.
 * Null when the take is empty.
 */
export function overdub(pattern: PatternInfo, take: Take): PatternInfo | null {
  if (take.size === 0) return null;
  const cells = [...take.values()].map((h) => {
    const row = pattern.rows.find((r) => r.pad === h.pad);
    return { ...h, value: Math.max(row?.steps[h.step] ?? 0, h.value) };
  });
  return setCells(pattern, cells);
}
