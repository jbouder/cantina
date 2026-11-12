/*
 * Which hand frets: the fretboard draws the nut on the left for a
 * right-handed player and on the right for a left-handed one. A setting
 * of the window's, remembered for next time; in a browser `?lefty` turns
 * it on for a screenshot.
 */

import { useSyncExternalStore } from "react";
import { inTauri } from "./engine";

const STORAGE_KEY = "cantina.leftHanded";

const listeners = new Set<() => void>();
let leftHanded = load();

function load(): boolean {
  if (!inTauri && new URLSearchParams(window.location.search).has("lefty")) return true;
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function setLeftHanded(on: boolean) {
  leftHanded = on;
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

/** Left-handed, re-rendering when it changes. */
export function useLeftHanded(): [boolean, (on: boolean) => void] {
  const on = useSyncExternalStore(
    subscribe,
    () => leftHanded,
    () => false,
  );
  return [on, setLeftHanded];
}
