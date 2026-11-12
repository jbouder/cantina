// Exercises as the practice view reads them: the file's types, the tab
// its phrase reads as, and where in the loop a playhead is. Kept apart
// from engine.ts so tests can import it without a window. The tab mirrors
// `Exercise::tab` in cantina-project, which the shell sends; the stand-in
// engine works it out here.

import type { Instrument, JamKey, Tab, TabNote, Teaches } from "./engine";

/** Ticks in a quarter note. */
export const PPQ = 960;

/** A chord shape held for a while. Mirrors `ChordShape` in cantina-project. */
export interface ChordShape {
  name: string;
  /** A fret per string, lowest string first; null for a string not played. */
  frets: (number | null)[];
  /** The finger on each string, 1 the index to 4 the little finger, 0 none. */
  fingers?: number[];
  start: number;
  length: number;
}

/** A strumming pattern. Mirrors `Strum` in cantina-project. */
export interface Strum {
  /** `D` down, `U` up, `x` a muted scratch, `-` a miss; spaces ignored. */
  pattern: string;
  stepsPerBeat: number;
}

/** What to play. Mirrors `Phrase` in cantina-project. */
export type Phrase =
  | { kind: "notes"; notes: TabNote[] }
  | { kind: "chords"; chords: ChordShape[]; strum?: Strum | null };

/** Something to practice. Mirrors `Exercise` in cantina-project. */
export interface Exercise {
  version: number;
  name: string;
  instrument: Instrument;
  teaches: Teaches;
  style: string;
  instructions: string;
  key: JamKey;
  timeSignature?: { numerator: number; denominator: number };
  tempo: { start: number; goal: number; step: number };
  countIn?: number;
  tuning?: number[] | null;
  capo?: number;
  bars?: number | null;
  phrase: Phrase;
  rig?: string | null;
  drums?: { kit: string; groove: string; levelDb?: number } | null;
}

/** What it teaches, as the picker and the docs name it. */
export const TEACHES_LABELS: Record<Teaches, string> = {
  chord: "Chords",
  change: "Changes",
  scale: "Scales",
  technique: "Technique",
  riff: "Riffs",
  rhythm: "Rhythm",
};

const STANDARD: Record<Instrument, number[]> = {
  guitar: [40, 45, 50, 55, 59, 64],
  bass: [28, 33, 38, 43],
};

/** The open strings, lowest first: the file's tuning, or standard. */
export function exerciseTuning(e: Exercise): number[] {
  return e.tuning ?? STANDARD[e.instrument];
}

function meter(e: Exercise) {
  return e.timeSignature ?? { numerator: 4, denominator: 4 };
}

/** Ticks in a beat of the exercise's meter. */
export function beatTicks(e: Exercise): number {
  return (PPQ * 4) / Math.max(1, meter(e).denominator);
}

/** Ticks in a bar of the exercise's meter. */
export function barTicks(e: Exercise): number {
  return beatTicks(e) * Math.max(1, meter(e).numerator);
}

/** The tick just after the phrase's last note or chord. */
export function phraseEnd(e: Exercise): number {
  const p = e.phrase;
  const ends =
    p.kind === "notes"
      ? p.notes.map((n) => n.start + n.length)
      : p.chords.map((c) => c.start + c.length);
  return Math.max(0, ...ends);
}

/** Ticks in one pass of the loop: `bars` bars, or every bar the phrase reaches. */
export function loopTicks(e: Exercise): number {
  const bar = barTicks(e);
  if (e.bars) return Math.max(1, e.bars) * bar;
  return Math.max(1, Math.ceil(phraseEnd(e) / bar)) * bar;
}

/** When a chord is struck while it lasts, how long each strike rings, and which stroke. */
function strikes(e: Exercise, chord: ChordShape, strum: Strum | null | undefined) {
  const end = chord.start + chord.length;
  const strokes = [...(strum?.pattern ?? "")].filter((c) => c.trim() !== "");
  const step = strum ? Math.floor(beatTicks(e) / Math.max(1, strum.stepsPerBeat)) : 0;
  if (!strum || strokes.length === 0 || step === 0) {
    return [{ at: chord.start, length: chord.length, stroke: "D" }];
  }
  const hits: { at: number; stroke: string }[] = [];
  for (let at = chord.start, i = 0; at < end; at += step, i++) {
    const c = strokes[i % strokes.length] ?? "-";
    if (c === "D" || c === "U" || c === "x") hits.push({ at, stroke: c });
  }
  return hits.map((h, k) => ({
    at: h.at,
    length: (hits[k + 1]?.at ?? end) - h.at,
    stroke: h.stroke,
  }));
}

/**
 * The phrase as a tab at the start tempo: notes as they are, and each
 * chord strike as a note on every string it plays, all on the strike's
 * tick. As `Exercise::tab` in cantina-project.
 */
export function exerciseTab(e: Exercise): Tab {
  const tuning = exerciseTuning(e);
  const m = meter(e);
  let notes: TabNote[];
  if (e.phrase.kind === "notes") {
    notes = e.phrase.notes.map((n) => ({ ...n }));
  } else {
    const strum = e.phrase.strum;
    notes = [];
    for (const chord of e.phrase.chords) {
      for (const s of strikes(e, chord, strum)) {
        chord.frets.forEach((fret, i) => {
          if (fret === null || fret === undefined) return;
          const note: TabNote = {
            string: tuning.length - i,
            fret,
            start: s.at,
            length: s.length,
          };
          if (s.stroke === "U") note.velocity = 0.8;
          if (s.stroke === "x") {
            note.velocity = 0.7;
            note.dead = true;
          }
          notes.push(note);
        });
      }
    }
  }
  return {
    name: e.name,
    instrument: e.instrument,
    tuning,
    capo: e.capo ?? 0,
    tempo: { bpm: e.tempo.start, numerator: m.numerator, denominator: m.denominator },
    notes,
  };
}

/** The ladder's rungs from the start to the goal, the goal always last. */
export function ladder(e: Exercise): number[] {
  const { start, goal, step } = e.tempo;
  const out: number[] = [];
  if (!(step > 0)) return [start];
  for (let t = start; t < goal - 1e-9; t += step) out.push(Math.round(t * 10) / 10);
  out.push(goal);
  return out;
}

/** The tempo a rung up or down the ladder from `tempo`, kept inside it. */
export function stepTempo(e: Exercise, tempo: number, by: 1 | -1): number {
  const rungs = ladder(e);
  if (by > 0) return rungs.find((t) => t > tempo + 1e-6) ?? rungs.at(-1) ?? tempo;
  return [...rungs].reverse().find((t) => t < tempo - 1e-6) ?? rungs[0] ?? tempo;
}

/** Which pass of the loop a tick is in, from 1, and the tick inside it. */
export function loopPlace(tick: number, loop: number): { pass: number; tick: number } {
  const period = Math.max(1, loop);
  const t = Math.max(0, tick);
  return { pass: Math.floor(t / period) + 1, tick: t % period };
}
