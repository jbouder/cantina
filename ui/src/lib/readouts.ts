/*
 * Pedal readouts: the values a pedal measures for its display, streamed
 * in every frame (see frame.ts). The frame carries them flat, in track
 * and chain order; the status says which pedal owns how many, so the
 * two are joined here. A tuner's pitch is turned into a note here too.
 */

import type { TrackStatus } from "./engine";

/** Each pedal's readout values by pedal id, from one frame. */
export function readoutsByPedal(
  tracks: TrackStatus[],
  values: ArrayLike<number>,
): Map<number, number[]> {
  const out = new Map<number, number[]>();
  let at = 0;
  for (const track of tracks) {
    for (const pedal of track.chain) {
      const n = pedal.readouts.length;
      if (n === 0) continue;
      out.set(
        pedal.id,
        Array.from({ length: n }, (_, i) => values[at + i] ?? 0),
      );
      at += n;
    }
  }
  // A frame from before or after a chain edit does not line up; show
  // nothing rather than another pedal's values.
  if (at !== values.length) return new Map();
  return out;
}

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"] as const;

export interface Note {
  /** The nearest note's name, with a sharp where needed. */
  name: string;
  /** Scientific pitch octave: A4 is 440 Hz. */
  octave: number;
  /** Cents from the nearest note, -50 to 50; positive is sharp. */
  cents: number;
}

/** The nearest note to a frequency against a reference for A4. */
export function noteFor(hz: number, reference = 440): Note | null {
  if (!(hz > 0) || !(reference > 0)) return null;
  const semitones = 12 * Math.log2(hz / reference) + 69;
  const nearest = Math.round(semitones);
  return {
    name: NAMES[((nearest % 12) + 12) % 12] ?? "C",
    octave: Math.floor(nearest / 12) - 1,
    cents: (semitones - nearest) * 100,
  };
}
