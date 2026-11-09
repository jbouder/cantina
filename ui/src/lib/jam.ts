import type { GrooveSection, JamMark } from "./engine";

/** Ticks per quarter note, as the engine counts. */
const PPQ = 960;

/** The sections in the order the jam lists them; keys 1 to 6 reach them. */
export const JAM_SECTIONS: GrooveSection[] = [
  "intro",
  "verse",
  "chorus",
  "bridge",
  "fill",
  "ending",
];

export const SECTION_LABELS: Record<GrooveSection, string> = {
  intro: "Intro",
  verse: "Verse",
  chorus: "Chorus",
  bridge: "Bridge",
  fill: "Fill",
  ending: "Ending",
};

/** The twelve tonics the key picker offers. */
export const TONICS = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/** The slowest and fastest a jam plays. */
export const JAM_TEMPO = { min: 30, max: 300 };

/** Where a jam is: the section and tempo playing, the bar and beat, and what is coming. */
export interface JamPlace {
  section: GrooveSection;
  bpm: number;
  /** From 1. */
  bar: number;
  /** From 1. */
  beat: number;
  /** The next section change, and the bar it lands on. */
  next: { section: GrooveSection; bar: number } | null;
  /** The next tempo change, and the bar it lands on. */
  nextTempo: { bpm: number; bar: number } | null;
}

/**
 * Where a jam is `seconds` into its timeline, from its marks: the last
 * mark at or before the position gives the section and tempo, and the
 * ticks since it the bar and beat.
 */
export function jamPlace(
  seconds: number,
  marks: JamMark[],
  sampleRate: number,
  barTicks: number,
  beatsPerBar: number,
): JamPlace | null {
  const first = marks[0];
  if (!first) return null;
  const sample = Math.max(0, seconds * sampleRate);
  let at = first;
  for (const m of marks) {
    if (m.sample <= sample) at = m;
    else break;
  }
  const tick = at.tick + ((sample - at.sample) / sampleRate) * (at.bpm / 60) * PPQ;
  const barOf = (t: number) => Math.floor(t / barTicks) + 1;
  const bar = barOf(tick);
  const beatTicks = barTicks / Math.max(1, beatsPerBar);
  const beat = Math.floor((tick - (bar - 1) * barTicks) / beatTicks) + 1;
  const later = marks.filter((m) => m.sample > sample);
  const section = later.find((m) => m.section !== at.section);
  const tempo = later.find((m) => m.bpm !== at.bpm);
  return {
    section: at.section,
    bpm: at.bpm,
    bar,
    beat: Math.min(beat, beatsPerBar),
    next: section ? { section: section.section, bar: barOf(section.tick) } : null,
    nextTempo: tempo ? { bpm: tempo.bpm, bar: barOf(tempo.tick) } : null,
  };
}

/** A gap between taps longer than this starts a new count, in ms. */
const TAP_RESET_MS = 2000;
/** Taps averaged, the latest. */
const TAPS = 5;

/**
 * The taps that count after a tap at `now`: the recent ones, a long gap
 * starting again.
 */
export function addTap(taps: number[], now: number): number[] {
  const last = taps.at(-1);
  const kept = last !== undefined && now - last <= TAP_RESET_MS ? taps : [];
  return [...kept, now].slice(-TAPS);
}

/** The tempo the taps give, to a tenth of a bpm, or null before the second tap. */
export function tapTempo(taps: number[]): number | null {
  const first = taps[0];
  const last = taps.at(-1);
  if (taps.length < 2 || first === undefined || last === undefined || last <= first) return null;
  const bpm = (60_000 * (taps.length - 1)) / (last - first);
  return Math.round(clampTempo(bpm) * 10) / 10;
}

export function clampTempo(bpm: number): number {
  return Math.min(JAM_TEMPO.max, Math.max(JAM_TEMPO.min, bpm));
}
