import { useSyncExternalStore } from "react";

/**
 * A pattern dragged from the drum grid toward the timeline.
 *
 * The drag uses pointer events, not HTML drag and drop: on macOS the
 * shell's file-drop handler claims every drop, so the webview never
 * fires `drop` (#159). The grid owns the pointer; the timeline registers
 * itself as the place a pattern can land.
 */
export type PatternDrag = {
  id: number;
  name: string;
  /** The pointer, in client coordinates. */
  x: number;
  y: number;
};

let current: PatternDrag | null = null;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Starts, moves or (with null) ends the drag. */
export function setPatternDrag(drag: PatternDrag | null) {
  current = drag;
  for (const l of listeners) l();
}

/** The pattern being dragged, if any; re-renders as it moves. */
export function usePatternDrag(): PatternDrag | null {
  return useSyncExternalStore(subscribe, () => current);
}

type Target = { el: Element; drop: (pattern: number, x: number, y: number) => void };
let target: Target | null = null;

/**
 * Makes `el` the place patterns land: `drop` gets the pattern and the
 * pointer in client coordinates. Returns a function that unregisters it.
 */
export function setPatternDropTarget(
  el: Element,
  drop: (pattern: number, x: number, y: number) => void,
): () => void {
  const mine: Target = { el, drop };
  target = mine;
  return () => {
    if (target === mine) target = null;
  };
}

/**
 * Drops a pattern at a point. True when the point is over the target,
 * which then places it; false when it lands anywhere else.
 */
export function dropPatternAt(pattern: number, x: number, y: number): boolean {
  if (!target) return false;
  const hit = document.elementFromPoint(x, y);
  if (!hit || !target.el.contains(hit)) return false;
  target.drop(pattern, x, y);
  return true;
}
