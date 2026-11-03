/*
 * How much the studio may move: the system's reduced-motion preference,
 * and Reduce flashing, a setting of Cantina's own that stills strobing
 * beat effects on the stage and in the skins. Remembered for next time;
 * in a browser `?reduceFlashing` turns it on for a screenshot.
 */

import { useSyncExternalStore } from "react";
import { inTauri } from "./engine";

const STORAGE_KEY = "cantina.reduceFlashing";

const listeners = new Set<() => void>();
let reduceFlashing = load();

function load(): boolean {
  if (!inTauri && new URLSearchParams(window.location.search).has("reduceFlashing")) return true;
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function reducesFlashing(): boolean {
  return reduceFlashing;
}

export function setReduceFlashing(on: boolean) {
  reduceFlashing = on;
  try {
    window.localStorage.setItem(STORAGE_KEY, on ? "1" : "0");
  } catch {
    // Not remembered this time; it still applies.
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Reduce flashing, re-rendering when it changes. */
export function useReduceFlashing(): [boolean, (on: boolean) => void] {
  const on = useSyncExternalStore(
    subscribe,
    () => reduceFlashing,
    () => false,
  );
  return [on, setReduceFlashing];
}

const reducedMotion =
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)")
    : null;

/** The system asks for reduced motion. */
export function prefersReducedMotion(): boolean {
  return reducedMotion?.matches ?? false;
}
