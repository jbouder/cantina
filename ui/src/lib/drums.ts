/*
 * Step sequencer arithmetic: where the playhead is in a looping pattern,
 * what a drag does to a velocity, and labels. Pure, so a test can reach
 * it; the grid component draws and handles the pointer.
 */
import type { PatternInfo, Tempo } from "./engine";

/** Ticks per quarter note, as in the engine's tempo map and the project. */
export const PPQ = 960;

/** The velocity a fresh hit gets. */
export const DEFAULT_VELOCITY = 1;
/** Pixels of vertical drag that take a velocity from 0 to 1. */
export const DRAG_PX = 80;

export function stepTicks(pattern: Pick<PatternInfo, "stepsPerBeat">): number {
  return PPQ / Math.max(1, pattern.stepsPerBeat);
}

export function patternTicks(pattern: Pick<PatternInfo, "stepsPerBeat" | "length">): number {
  return stepTicks(pattern) * Math.max(1, pattern.length);
}

/** Ticks at a position in seconds under a constant tempo. */
export function ticksAt(seconds: number, tempo: Tempo): number {
  return (seconds * tempo.bpm * PPQ) / 60;
}

/**
 * The step under the playhead of a pattern looping from the start, or
 * null when the transport is not rolling. Swing is ignored: the column
 * lights on the straight grid.
 */
export function currentStep(
  seconds: number,
  tempo: Tempo,
  pattern: Pick<PatternInfo, "stepsPerBeat" | "length">,
  playing: boolean,
): number | null {
  if (!playing) return null;
  const ticks = ticksAt(Math.max(0, seconds), tempo) % patternTicks(pattern);
  return Math.min(pattern.length - 1, Math.floor(ticks / stepTicks(pattern)));
}

/** The velocity a drag of `dy` pixels (down is positive) makes from `start`. */
export function draggedVelocity(start: number, dy: number): number {
  const v = start - dy / DRAG_PX;
  return Math.min(1, Math.max(0.05, Math.round(v * 100) / 100));
}

/** A velocity as a percentage for readouts. */
export function velocityLabel(velocity: number): string {
  return `${Math.round(velocity * 100)}%`;
}

/** True for a step that starts a beat, for the grid's shading. */
export function isBeatStart(step: number, pattern: Pick<PatternInfo, "stepsPerBeat">): boolean {
  return step % Math.max(1, pattern.stepsPerBeat) === 0;
}

/** `1 bar`, `2 bars`, `6 beats`, `3 steps`: a pattern's length in musical terms. */
export function lengthLabel(
  pattern: Pick<PatternInfo, "stepsPerBeat" | "length">,
  tempo: Pick<Tempo, "numerator">,
): string {
  const perBeat = Math.max(1, pattern.stepsPerBeat);
  const perBar = perBeat * Math.max(1, tempo.numerator);
  if (pattern.length % perBar === 0) {
    const bars = pattern.length / perBar;
    return `${bars} bar${bars === 1 ? "" : "s"}`;
  }
  if (pattern.length % perBeat === 0) {
    const beats = pattern.length / perBeat;
    return `${beats} beat${beats === 1 ? "" : "s"}`;
  }
  return `${pattern.length} step${pattern.length === 1 ? "" : "s"}`;
}

/** The velocity at a cell, 0 when the pad has no row. */
export function velocityAt(pattern: PatternInfo, pad: string, step: number): number {
  return pattern.rows.find((r) => r.pad === pad)?.steps[step] ?? 0;
}
