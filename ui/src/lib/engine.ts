import { invoke } from "@tauri-apps/api/core";

/** What the shell reports about the engine. Mirrors `EngineStatus` in src-tauri. */
export interface EngineStatus {
  running: boolean;
  sampleRate: number;
  bufferFrames: number;
  inputName: string | null;
  outputName: string | null;
  /** Why the last start or stop failed, if it did. */
  error: string | null;
}

/** True inside the Tauri web view, false in a plain browser tab. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/*
 * In a browser (`npm run dev` without the shell) there is no engine, so the
 * UI talks to this stand-in instead. It keeps `vite dev` useful for layout
 * and skin work and is what the PR screenshots are taken from.
 */
const mock: EngineStatus = {
  running: false,
  sampleRate: 48000,
  bufferFrames: 128,
  inputName: "MacBook Pro Microphone",
  outputName: "MacBook Pro Speakers",
  error: null,
};

async function call(command: string): Promise<EngineStatus> {
  if (inTauri) {
    return invoke<EngineStatus>(command);
  }
  if (command === "engine_start") mock.running = true;
  if (command === "engine_stop") mock.running = false;
  return { ...mock };
}

export function engineStatus(): Promise<EngineStatus> {
  return call("engine_status");
}

export function engineStart(): Promise<EngineStatus> {
  return call("engine_start");
}

export function engineStop(): Promise<EngineStatus> {
  return call("engine_stop");
}
