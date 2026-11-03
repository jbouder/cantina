/*
 * What the stage shows, shared by every place that draws it: the stage
 * sheet, performance mode and the stage window on a second display. The
 * scene is remembered for next time and passed to the stage window, so
 * switching scenes on the laptop switches the TV.
 */

import { DEFAULT_SCENE, getScene, SCENES } from "@cantina/scenes";
import { useSyncExternalStore } from "react";

// As engine.ts decides it, without loading the engine's browser stand-in,
// so the stage window and the tests stay light.
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const STORAGE_KEY = "cantina.scene";
/** The event and window label the stage window answers to. */
export const STAGE_WINDOW = "stage";
const SCENE_EVENT = "stage://scene";

/** The scenes a person can pick, in order: all but those reachable by id only. */
export const LISTED_SCENES = SCENES.filter((s) => !s.hidden);

const listeners = new Set<() => void>();
let scene = load();

function load(): string {
  if (typeof window === "undefined") return DEFAULT_SCENE.gpu;
  if (!inTauri) {
    const fromUrl = new URLSearchParams(window.location.search).get("scene");
    if (fromUrl && getScene(fromUrl)) return fromUrl;
  }
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved && getScene(saved)) return saved;
  } catch {
    // The default will do.
  }
  return DEFAULT_SCENE.gpu;
}

export function currentScene(): string {
  return scene;
}

/** Shows a scene everywhere the stage is drawn. */
export function setScene(id: string, { broadcast = true } = {}) {
  if (!getScene(id) || id === scene) return;
  scene = id;
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, id);
  } catch {
    // Not remembered this time.
  }
  for (const listener of listeners) listener();
  if (broadcast && inTauri) {
    import("@tauri-apps/api/event").then(({ emitTo }) =>
      emitTo(STAGE_WINDOW, SCENE_EVENT, id).catch(() => {
        // No stage window open.
      }),
    );
  }
}

/** The scene `step` places along the list from the current one, wrapping. */
export function stepScene(step: number) {
  const at = LISTED_SCENES.findIndex((s) => s.id === scene);
  const next = LISTED_SCENES[(at + step + LISTED_SCENES.length) % LISTED_SCENES.length];
  if (next) setScene(next.id);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The scene on the stage, re-rendering when it changes. */
export function useScene(): string {
  return useSyncExternalStore(
    subscribe,
    () => scene,
    () => DEFAULT_SCENE.gpu,
  );
}

/** In the stage window: follows the scene the main window picks. */
export async function followScene(): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>(SCENE_EVENT, (e) => setScene(e.payload, { broadcast: false }));
}

/** True when this page is the stage window rather than the studio. */
export function isStageWindow(): boolean {
  return new URLSearchParams(window.location.search).get("view") === STAGE_WINDOW;
}

/**
 * Opens the stage full screen on the display the main window is not on.
 * Returns false, opening nothing, when there is only one display or no
 * shell to open windows.
 */
export async function openStageWindow(): Promise<boolean> {
  if (!inTauri) return false;
  const { availableMonitors, currentMonitor } = await import("@tauri-apps/api/window");
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const existing = await WebviewWindow.getByLabel(STAGE_WINDOW);
  if (existing) {
    await existing.setFocus();
    return true;
  }
  const here = await currentMonitor();
  const other = (await availableMonitors()).find(
    (m) => !here || m.position.x !== here.position.x || m.position.y !== here.position.y,
  );
  if (!other) return false;
  const at = other.position.toLogical(other.scaleFactor);
  const size = other.size.toLogical(other.scaleFactor);
  new WebviewWindow(STAGE_WINDOW, {
    url: `index.html?view=${STAGE_WINDOW}`,
    title: "Cantina stage",
    x: at.x,
    y: at.y,
    width: size.width,
    height: size.height,
    decorations: false,
    fullscreen: true,
    focus: false,
  });
  return true;
}

export async function closeStageWindow() {
  if (!inTauri) return;
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  await (await WebviewWindow.getByLabel(STAGE_WINDOW))?.close();
}

/** Whether the stage window is open, for the key that toggles it. */
export async function stageWindowOpen(): Promise<boolean> {
  if (!inTauri) return false;
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  return (await WebviewWindow.getByLabel(STAGE_WINDOW)) !== null;
}
