import type { ClipInfo, Tab, TabNote, Tempo } from "./engine";

/** Ticks per quarter note, as everywhere else. */
export const PPQ = 960;

/** Ticks in one bar of `tempo`'s time signature. */
export function barTicks(tempo: Tempo): number {
  return ((PPQ * 4) / Math.max(1, tempo.denominator)) * Math.max(1, tempo.numerator);
}

/** Seconds per tick at `tempo`. */
export function secondsPerTick(tempo: Tempo): number {
  return 60 / (tempo.bpm * PPQ);
}

/** The timeline sample where the tab's tick 0 sounds. */
export function tabZeroSample(clip: ClipInfo, tab: Tab): number {
  return clip.start - clip.offset + (tab.origin ?? 0);
}

/** The tab's tick at a timeline position in seconds. */
export function tickAt(clip: ClipInfo, tab: Tab, sampleRate: number, seconds: number): number {
  const fromZero = seconds - tabZeroSample(clip, tab) / sampleRate;
  return fromZero / secondsPerTick(tab.tempo);
}

/** The timeline sample a tick of the tab sounds at. */
export function sampleAt(clip: ClipInfo, tab: Tab, sampleRate: number, tick: number): number {
  return tabZeroSample(clip, tab) + tick * secondsPerTick(tab.tempo) * sampleRate;
}

/** The ticks of the tab the clip plays: notes outside are trimmed away. */
export function playedTicks(clip: ClipInfo, tab: Tab, sampleRate: number): [number, number] {
  const zero = tabZeroSample(clip, tab);
  const perTick = secondsPerTick(tab.tempo) * sampleRate;
  return [(clip.start - zero) / perTick, (clip.start + clip.length - zero) / perTick];
}

/** What is written for a note on the staff: its fret, `x` or `<12>`. */
export function fretLabel(note: TabNote): string {
  if (note.dead) return "x";
  if (note.harmonic) return `<${note.fret}>`;
  return String(note.fret);
}

/** A bend's amount as tab writes it: ½, full, 1½, 2. */
export function bendLabel(semitones: number): string {
  const steps = semitones / 2;
  if (steps === 1) return "full";
  const whole = Math.floor(steps);
  const half = steps - whole >= 0.25 ? "½" : "";
  return `${whole > 0 ? whole : ""}${half}` || "¼";
}

/**
 * The technique marked between a note and the next one on its string:
 * `h` hammer-on, `p` pull-off, `/` or `\` a slide up or down. Read from
 * the second note for hammer-ons and pull-offs, the first for slides.
 */
export function joinLabel(note: TabNote, next: TabNote | undefined): string | null {
  if (!next) return null;
  if (next.hammerOn) return "h";
  if (next.pullOff) return "p";
  if (note.slide) return next.fret >= note.fret ? "/" : "\\";
  return null;
}

/** A note value as rhythm notation draws it. */
export interface Rhythm {
  /** Flags (or beams): 0 for a quarter or longer, 1 for an eighth, 2 a sixteenth, 3 a thirty-second. */
  flags: number;
  /** A stem is drawn: anything shorter than a whole note. */
  stem: boolean;
  /** Drawn hollow, a half note or a whole. */
  hollow: boolean;
  dotted: boolean;
  /** Part of a triplet: marked 3. */
  triplet: boolean;
}

/** The written value nearest a length in ticks. */
export function rhythm(ticks: number): Rhythm {
  const values: [number, Omit<Rhythm, "dotted" | "triplet">][] = [
    [PPQ * 4, { flags: 0, stem: false, hollow: true }],
    [PPQ * 2, { flags: 0, stem: true, hollow: true }],
    [PPQ, { flags: 0, stem: true, hollow: false }],
    [PPQ / 2, { flags: 1, stem: true, hollow: false }],
    [PPQ / 4, { flags: 2, stem: true, hollow: false }],
    [PPQ / 8, { flags: 3, stem: true, hollow: false }],
  ];
  let best: Rhythm = { flags: 3, stem: true, hollow: false, dotted: false, triplet: false };
  let error = Number.POSITIVE_INFINITY;
  for (const [length, shape] of values) {
    for (const [scale, extra] of [
      [1, { dotted: false, triplet: false }],
      [1.5, { dotted: true, triplet: false }],
      [2 / 3, { dotted: false, triplet: true }],
    ] as const) {
      const e = Math.abs(ticks - length * scale);
      if (e < error) {
        error = e;
        best = { ...shape, ...extra };
      }
    }
  }
  return best;
}

/** One note placed on the page. */
export interface PlacedNote {
  note: TabNote;
  /** Index into the tab's notes. */
  index: number;
  x: number;
  /** The string's line, 0 for the top (string 1). */
  line: number;
  label: string;
  /** Written between this note and the next on its string. */
  join: string | null;
  /** x of the next note on the string, for the join. */
  joinTo: number | null;
}

/** A column of notes that start together, for the rhythm under the staff. */
export interface PlacedBeat {
  tick: number;
  x: number;
  rhythm: Rhythm;
}

/** One bar on the page. */
export interface PlacedBar {
  /** Counting from 1. */
  number: number;
  startTick: number;
  x: number;
  width: number;
  row: number;
}

/** The whole tab laid out in rows of bars. */
export interface TabLayout {
  bars: PlacedBar[];
  notes: PlacedNote[];
  beats: PlacedBeat[];
  rows: number;
  /** Width of a row. */
  width: number;
  barWidth: number;
  strings: number;
}

/** Room before a bar's first beat and after its last, in pixels. */
const BAR_PAD = 14;

/**
 * Lays the tab out: `barsPerRow` bars of `barWidth` pixels a row, enough
 * bars for every note and at least `minTicks` ticks. Notes sit at their
 * tick's x in their bar, on their string's line.
 */
export function layoutTab(tab: Tab, barsPerRow: number, barWidth: number, minTicks = 0): TabLayout {
  const per = barTicks(tab.tempo);
  const end = Math.max(minTicks, ...tab.notes.map((n) => n.start + n.length), 1);
  const count = Math.max(1, Math.ceil(end / per));
  const perRow = Math.max(1, Math.min(barsPerRow, count));
  const inner = barWidth - 2 * BAR_PAD;
  const xAt = (tick: number) => {
    const bar = Math.min(count - 1, Math.floor(tick / per));
    const col = bar % perRow;
    return col * barWidth + BAR_PAD + ((tick - bar * per) / per) * inner;
  };
  const rowOf = (tick: number) => Math.floor(Math.min(count - 1, Math.floor(tick / per)) / perRow);

  const bars: PlacedBar[] = Array.from({ length: count }, (_, i) => ({
    number: i + 1,
    startTick: i * per,
    x: (i % perRow) * barWidth,
    width: barWidth,
    row: Math.floor(i / perRow),
  }));

  const order = tab.notes
    .map((note, index) => ({ note, index }))
    .sort((a, b) => a.note.start - b.note.start || a.note.string - b.note.string);
  const notes: PlacedNote[] = order.map(({ note, index }) => {
    const next = order.find(
      (o) => o.note.string === note.string && o.note.start > note.start,
    )?.note;
    const join = joinLabel(note, next);
    const sameRow = next !== undefined && rowOf(next.start) === rowOf(note.start);
    return {
      note,
      index,
      x: xAt(note.start),
      line: note.string - 1,
      label: fretLabel(note),
      join,
      joinTo: join && next && sameRow ? xAt(next.start) : null,
    };
  });

  const starts = [...new Set(tab.notes.map((n) => n.start))].sort((a, b) => a - b);
  const beats: PlacedBeat[] = starts.map((tick, i) => {
    const longest = Math.max(...tab.notes.filter((n) => n.start === tick).map((n) => n.length));
    // A note's written value runs to the next beat, or its own end.
    const next = starts[i + 1];
    const gap = next !== undefined ? next - tick : longest;
    return { tick, x: xAt(tick), rhythm: rhythm(Math.min(gap, longest) || gap) };
  });

  return {
    bars,
    notes,
    beats,
    rows: Math.ceil(count / perRow),
    width: perRow * barWidth,
    barWidth,
    strings: tab.tuning.length,
  };
}

/** The row and x of a tick on the layout, for the cursor. */
export function cursorAt(layout: TabLayout, tab: Tab, tick: number): { row: number; x: number } {
  const per = barTicks(tab.tempo);
  const count = layout.bars.length;
  const clamped = Math.max(0, Math.min(tick, count * per));
  const bar = Math.min(count - 1, Math.floor(clamped / per));
  const placed = layout.bars[bar] ?? { row: 0, x: 0 };
  const inner = layout.barWidth - 2 * BAR_PAD;
  return { row: placed.row, x: placed.x + BAR_PAD + ((clamped - bar * per) / per) * inner };
}

/** The tick under an x on a row, for click to seek. */
export function tickAtX(layout: TabLayout, tab: Tab, row: number, x: number): number {
  const per = barTicks(tab.tempo);
  const perRow = layout.width / layout.barWidth;
  const col = Math.max(0, Math.min(perRow - 1, Math.floor(x / layout.barWidth)));
  const bar = Math.min(layout.bars.length - 1, row * perRow + col);
  const inner = layout.barWidth - 2 * BAR_PAD;
  const within = (x - col * layout.barWidth - BAR_PAD) / inner;
  return bar * per + Math.max(0, Math.min(1, within)) * per;
}

/** The names of the open strings, top line first: e, B, G, D, A, E. */
export function stringNames(tab: Tab): string[] {
  const names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  const lowest = Math.min(...tab.tuning);
  return [...tab.tuning].reverse().map((midi, i) => {
    const name = names[(((midi + (tab.capo ?? 0)) % 12) + 12) % 12] ?? "?";
    // The top string of a guitar in E is written lower case, as tab does.
    return i === 0 && tab.tuning.length >= 6 && midi - lowest >= 24 ? name.toLowerCase() : name;
  });
}
