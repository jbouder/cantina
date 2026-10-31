import { useSyncExternalStore } from "react";
import { inTauri } from "../lib/engine";

/** The skins that ship. Each has a CSS file in this folder. */
export const SKINS = [
  { id: "classic", name: "Classic" },
  { id: "midnight", name: "Midnight" },
  { id: "metal", name: "Brushed metal" },
  { id: "hifi", name: "Hi-fi" },
  { id: "neon", name: "Neon" },
  { id: "toxic", name: "Toxic" },
] as const;

export type SkinId = (typeof SKINS)[number]["id"];

export const DEFAULT_SKIN: SkinId = "classic";

const STORAGE_KEY = "cantina.skin";

export function isSkinId(value: unknown): value is SkinId {
  return SKINS.some((s) => s.id === value);
}

/**
 * The skin to show on launch: the one saved last time, or the default. In
 * a browser `?skin=` overrides it so screenshots can pick one per URL.
 */
export function loadSkin(): SkinId {
  if (!inTauri) {
    const fromUrl = new URLSearchParams(window.location.search).get("skin");
    if (isSkinId(fromUrl)) return fromUrl;
  }
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (isSkinId(saved)) return saved;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return DEFAULT_SKIN;
}

const listeners = new Set<() => void>();
let current: SkinId = DEFAULT_SKIN;

/** Puts a skin on the document and remembers it for the next launch. */
export function applySkin(skin: SkinId) {
  current = skin;
  document.documentElement.dataset.skin = skin;
  try {
    window.localStorage.setItem(STORAGE_KEY, skin);
  } catch {
    // Not persisted this time; the skin is still applied.
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The current skin, re-rendering when it changes. */
export function useSkin(): [SkinId, (skin: SkinId) => void] {
  const skin = useSyncExternalStore(
    subscribe,
    () => current,
    () => DEFAULT_SKIN,
  );
  return [skin, applySkin];
}
