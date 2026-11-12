// Chord diagrams and the fretboard, as pure functions: a chord shape or
// the notes of a beat become a diagram to draw, and a tab and a playhead
// become the dots to light on a neck. Kept apart from the components so
// tests can check them without a window.

import type { Tab } from "./engine";
import type { ChordShape } from "./exercise";

/** Frets a diagram shows at least. */
export const DIAGRAM_FRETS = 4;

/** A chord as a box diagram draws it. */
export interface Diagram {
  name: string;
  /** A fret per string, lowest string first, counted from the capo: null muted, 0 open. */
  frets: (number | null)[];
  /** The finger on each string, lowest first: 1 the index to 4 the little finger, 0 none. */
  fingers: number[];
  capo: number;
  /** The fret drawn first, counted from the capo; 1 draws the nut. */
  base: number;
  /** How many frets are drawn. */
  span: number;
  /** One finger laid across strings at a fret, by string index, lowest first. */
  barres: { fret: number; from: number; to: number; finger: number }[];
}

/** A diagram for frets and fingers, lowest string first. */
export function diagramOf(
  name: string,
  frets: (number | null)[],
  fingers: number[],
  capo: number,
): Diagram {
  const fretted = frets.filter((f): f is number => f !== null && f > 0);
  const high = Math.max(0, ...fretted);
  const low = fretted.length > 0 ? Math.min(...fretted) : 1;
  const base = high <= DIAGRAM_FRETS ? 1 : low;
  const span = Math.max(DIAGRAM_FRETS, high - base + 1);
  const barres: Diagram["barres"] = [];
  const byFinger = new Map<string, number[]>();
  frets.forEach((fret, i) => {
    const finger = fingers[i] ?? 0;
    if (fret === null || fret === 0 || finger === 0) return;
    const key = `${finger}:${fret}`;
    byFinger.set(key, [...(byFinger.get(key) ?? []), i]);
  });
  for (const [key, strings] of byFinger) {
    if (strings.length < 2) continue;
    const [finger, fret] = key.split(":").map(Number);
    barres.push({
      fret: fret ?? 0,
      from: Math.min(...strings),
      to: Math.max(...strings),
      finger: finger ?? 0,
    });
  }
  return {
    name,
    frets: [...frets],
    fingers: frets.map((_, i) => fingers[i] ?? 0),
    capo,
    base,
    span,
    barres,
  };
}

/** An exercise's chord shape as a diagram. */
export function chordDiagram(shape: ChordShape, capo = 0): Diagram {
  return diagramOf(shape.name, shape.frets, shape.fingers ?? [], capo);
}

const NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** Chord qualities by the intervals above the root, in pitch classes. */
const QUALITIES: [number[], string][] = [
  [[0, 4, 7], ""],
  [[0, 3, 7], "m"],
  [[0, 4, 7, 10], "7"],
  [[0, 3, 7, 10], "m7"],
  [[0, 4, 7, 11], "maj7"],
  [[0, 2, 4, 7, 10], "9"],
  [[0, 2, 7], "sus2"],
  [[0, 5, 7], "sus4"],
  [[0, 3, 6], "dim"],
  [[0, 4, 8], "aug"],
  [[0, 7], "5"],
];

/**
 * A name for a set of MIDI pitches: the root and its quality, the lowest
 * note preferred as the root, with `/bass` when another note is lowest.
 * Empty when the notes make no chord this knows.
 */
export function chordName(pitches: number[]): string {
  if (pitches.length < 2) return "";
  const lowest = Math.min(...pitches);
  const classes = [...new Set(pitches.map((p) => ((p % 12) + 12) % 12))];
  const roots = [
    ((lowest % 12) + 12) % 12,
    ...classes.filter((c) => c !== ((lowest % 12) + 12) % 12),
  ];
  for (const root of roots) {
    const set = classes.map((c) => (c - root + 12) % 12).sort((a, b) => a - b);
    const match = QUALITIES.find(
      ([q]) => q.length === set.length && q.every((v, i) => v === set[i]),
    );
    if (match) {
      const bass = ((lowest % 12) + 12) % 12;
      const slash = bass === root ? "" : `/${NAMES[bass]}`;
      return `${NAMES[root]}${match[1]}${slash}`;
    }
  }
  return "";
}

/** The ticks a note starts on, in order, of the notes that sound a pitch. */
function onsets(tab: Tab): number[] {
  return [...new Set(tab.notes.filter((n) => !n.dead).map((n) => n.start))].sort((a, b) => a - b);
}

/** The onset playing at `tick`: the last at or before it. */
function onsetAt(tab: Tab, tick: number): number | null {
  const at = onsets(tab).filter((t) => t <= tick);
  return at.length > 0 ? (at[at.length - 1] ?? null) : null;
}

/**
 * The chord a tab plays at `tick`: the notes that start together on the
 * onset at or before it, as a diagram, or null for fewer than two strings.
 */
export function beatDiagram(tab: Tab, tick: number): Diagram | null {
  const onset = onsetAt(tab, tick);
  if (onset === null) return null;
  const notes = tab.notes.filter((n) => n.start === onset && !n.dead);
  if (new Set(notes.map((n) => n.string)).size < 2) return null;
  const strings = tab.tuning.length;
  const frets: (number | null)[] = Array.from({ length: strings }, () => null);
  for (const n of notes) {
    const i = strings - n.string;
    if (i >= 0 && i < strings) frets[i] = n.fret;
  }
  const capo = tab.capo ?? 0;
  const pitches = notes.map((n) => (tab.tuning[strings - n.string] ?? 0) + capo + n.fret);
  return diagramOf(chordName(pitches), frets, [], capo);
}

/** A dot on the neck: a string (1 the highest), a fret from the nut, and what it is. */
export interface FretDot {
  string: number;
  /** Counted from the nut, the capo included; 0 is open. */
  fret: number;
  /** Sounding now, coming next, or only part of the phrase's shape. */
  kind: "now" | "next" | "shape";
}

/**
 * What the neck lights for a tab at `tick`: every note of the phrase
 * faintly, as the shape to learn; the notes of the beat sounding; and
 * those of the beat after. Stopped (`tick` null), the first beat is next.
 */
export function fretboardDots(tab: Tab, tick: number | null): FretDot[] {
  const capo = tab.capo ?? 0;
  const times = onsets(tab);
  const now = tick === null ? null : onsetAt(tab, tick);
  const next = times.find((t) => t > (now ?? -1)) ?? null;
  const dots = new Map<string, FretDot>();
  const rank = { shape: 0, next: 1, now: 2 };
  for (const n of tab.notes) {
    if (n.dead) continue;
    const kind = n.start === now ? "now" : n.start === next ? "next" : "shape";
    const key = `${n.string}:${n.fret}`;
    const had = dots.get(key);
    if (!had || rank[kind] > rank[had.kind]) {
      dots.set(key, { string: n.string, fret: capo + n.fret, kind });
    }
  }
  return [...dots.values()].sort((a, b) => a.string - b.string || a.fret - b.fret);
}

/** The last fret the neck draws: twelve at least, enough for the phrase, at most 24. */
export function neckFrets(tab: Tab): number {
  const capo = tab.capo ?? 0;
  const high = Math.max(0, ...tab.notes.map((n) => capo + n.fret));
  return Math.min(24, Math.max(12, high + 1));
}
