import { DEFAULT_SKIN, isSkinId, type SkinId } from "@cantina/skins";
import { useSyncExternalStore } from "react";
import { inTauri } from "../lib/engine";

/*
 * The skins come from @cantina/skins, shared with music-visualizer: the
 * list here, and the palette each one draws with through styles.css. The
 * CSS files in this folder add Cantina's own tokens on top.
 */
export {
  DEFAULT_SKIN,
  isSkinId,
  SKINS,
  type SkinId,
  skinUsesBeat,
} from "@cantina/skins";

const STORAGE_KEY = "cantina.skin";

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

/** The skin on the document now. */
export function currentSkin(): SkinId {
  return current;
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
