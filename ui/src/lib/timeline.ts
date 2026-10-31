/*
 * Timeline geometry: samples to pixels, the grid from the tempo, snapping,
 * and the clip edits as pure functions. The canvas component draws and
 * handles the pointer; everything it decides is decided here, where a
 * test can reach it.
 */
import type { ClipInfo, Tempo } from "./engine";

/** What the timeline shows: the sample at its left edge and the zoom. */
export interface View {
  /** Timeline sample at the left edge of the lanes. */
  scroll: number;
  /** Samples per CSS pixel. */
  spp: number;
}

export const MIN_SPP = 8;
export const MAX_SPP = 1 << 16;

/** Pixels from the left edge for a timeline sample. */
export function xOf(sample: number, view: View): number {
  return (sample - view.scroll) / view.spp;
}

/** The timeline sample under a pixel, never negative. */
export function sampleAt(x: number, view: View): number {
  return Math.max(0, view.scroll + x * view.spp);
}

/** Zooms by `factor` keeping the sample under pixel `x` where it is. */
export function zoomAround(view: View, factor: number, x: number): View {
  const spp = Math.min(MAX_SPP, Math.max(MIN_SPP, view.spp * factor));
  const anchor = sampleAt(x, view);
  return { spp, scroll: Math.max(0, anchor - x * spp) };
}

/** A zoom that fits `samples` into `width` pixels with a little room. */
export function fitView(samples: number, width: number): View {
  const spp = Math.min(MAX_SPP, Math.max(MIN_SPP, (samples * 1.05) / Math.max(1, width)));
  return { scroll: 0, spp };
}

/** The grid the tempo gives at a zoom: where the lines go and how far apart. */
export interface Grid {
  /** Samples per beat. */
  beat: number;
  /** Samples per bar. */
  bar: number;
  /** Samples between grid lines, and what edits snap to. */
  division: number;
  /** Beats per bar, for labels. */
  beatsPerBar: number;
}

/** The fewest pixels between grid lines before the grid gets coarser. */
const MIN_LINE_PX = 18;

export function gridFor(tempo: Tempo, sampleRate: number, spp: number): Grid {
  const beat = (sampleRate * 60) / tempo.bpm;
  const beatsPerBar = tempo.numerator;
  const bar = beat * beatsPerBar;
  // Candidate divisions from a sixteenth up to many bars.
  const candidates = [beat / 4, beat / 2, beat, bar, bar * 2, bar * 4, bar * 8, bar * 16, bar * 32];
  let division = candidates[candidates.length - 1] ?? bar;
  for (const c of candidates) {
    if (c / spp >= MIN_LINE_PX) {
      division = c;
      break;
    }
  }
  return { beat, bar, division, beatsPerBar };
}

/** The nearest grid line to `sample`, or `sample` itself when snapping is off. */
export function snap(sample: number, division: number, enabled: boolean): number {
  if (!enabled || division <= 0) return Math.max(0, Math.round(sample));
  return Math.max(0, Math.round(sample / division) * division);
}

/** `1.2.3`: bar, beat and sixteenth, one-based, for the ruler and readouts. */
export function musicalLabel(sample: number, grid: Grid): string {
  const bar = Math.floor(sample / grid.bar);
  const inBar = sample - bar * grid.bar;
  const beat = Math.floor(inBar / grid.beat);
  const inBeat = inBar - beat * grid.beat;
  const sixteenth = Math.floor(inBeat / (grid.beat / 4));
  return `${bar + 1}.${beat + 1}.${sixteenth + 1}`;
}

/** Fixed layout, in CSS pixels. */
export const RULER_HEIGHT = 28;
export const TRACK_HEIGHT = 72;
/** How close to a clip's edge a pointer counts as a trim handle. */
export const EDGE_PX = 7;

export type Hit =
  | { kind: "ruler" }
  | { kind: "empty"; track: number }
  | { kind: "clip" | "left" | "right"; track: number; clip: ClipInfo };

/** What is under a pointer at `x`, `y` in the canvas. `tracks` holds each track's clips. */
export function hitTest(tracks: ClipInfo[][], x: number, y: number, view: View): Hit | null {
  if (y < 0) return null;
  if (y < RULER_HEIGHT) return { kind: "ruler" };
  const track = Math.floor((y - RULER_HEIGHT) / TRACK_HEIGHT);
  const clips = tracks[track];
  if (!clips) return null;
  for (const clip of clips) {
    const left = xOf(clip.start, view);
    const right = xOf(clip.start + clip.length, view);
    if (x < left || x > right) continue;
    if (x - left <= EDGE_PX && right - left > EDGE_PX * 3) return { kind: "left", track, clip };
    if (right - x <= EDGE_PX && right - left > EDGE_PX * 3) return { kind: "right", track, clip };
    return { kind: "clip", track, clip };
  }
  return { kind: "empty", track };
}

/** A clip's geometry after an edit: what the command will carry. */
export interface Geometry {
  start: number;
  offset: number;
  length: number;
}

/** Moves a clip by `delta` samples, snapping its start. */
export function moved(clip: ClipInfo, delta: number, division: number, snapOn: boolean): Geometry {
  return {
    start: snap(clip.start + delta, division, snapOn),
    offset: clip.offset,
    length: clip.length,
  };
}

/**
 * Trims the left edge to `at`, keeping the audio where it is: the offset
 * moves with the start. The edge cannot go before the file's first frame
 * or past the clip's last.
 */
export function trimmedLeft(
  clip: ClipInfo,
  at: number,
  division: number,
  snapOn: boolean,
): Geometry {
  const end = clip.start + clip.length;
  const earliest = clip.start - clip.offset;
  let start = snap(at, division, snapOn);
  start = Math.min(Math.max(start, earliest), end - 1);
  const delta = start - clip.start;
  return { start, offset: clip.offset + delta, length: clip.length - delta };
}

/**
 * Trims the right edge to `at`. The clip cannot grow past the end of its
 * file, which is `fileFrames` when known.
 */
export function trimmedRight(
  clip: ClipInfo,
  at: number,
  division: number,
  snapOn: boolean,
  fileFrames?: number,
): Geometry {
  let end = snap(at, division, snapOn);
  const latest =
    fileFrames === undefined ? clip.start + clip.length : clip.start + (fileFrames - clip.offset);
  end = Math.max(clip.start + 1, Math.min(end, latest));
  return { start: clip.start, offset: clip.offset, length: end - clip.start };
}

/** Whether a timeline sample is strictly inside a clip, where a split is allowed. */
export function canSplit(clip: ClipInfo, at: number): boolean {
  return at > clip.start && at < clip.start + clip.length;
}

/** The scroll that keeps `sample` on screen, paging rather than creeping. */
export function follow(view: View, sample: number, width: number): number {
  const span = width * view.spp;
  if (sample < view.scroll || sample >= view.scroll + span) {
    return Math.max(0, sample - span * 0.1);
  }
  return view.scroll;
}
