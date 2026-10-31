/*
 * Peak caches by file, built once each in the worker and kept for the
 * page's life. `usePeaks` subscribes a component to one file: it renders
 * again when the cache lands.
 */
import { useSyncExternalStore } from "react";
import { audioSamples } from "./engine";
import type { PeakCache } from "./peaks";
import type { PeaksRequest, PeaksResponse } from "./peaks.worker";

const caches = new Map<string, PeakCache>();
const pending = new Set<string>();
const listeners = new Set<() => void>();

let worker: Worker | null = null;

function getWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./peaks.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (e: MessageEvent<PeaksResponse>) => {
    caches.set(e.data.file, e.data.cache);
    pending.delete(e.data.file);
    for (const l of listeners) l();
  };
  worker.onerror = (e) => console.error("peaks worker", e.message);
  return worker;
}

/** Starts building a file's cache if it is not built or building. */
export function loadPeaks(file: string) {
  if (caches.has(file) || pending.has(file)) return;
  pending.add(file);
  audioSamples(file).then(
    (samples) => {
      const request: PeaksRequest = { file, samples: samples.buffer as ArrayBuffer };
      getWorker().postMessage(request, [request.samples]);
    },
    (e) => {
      pending.delete(file);
      console.error(`loading ${file}:`, e);
    },
  );
}

export function peaksFor(file: string): PeakCache | undefined {
  return caches.get(file);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** A version number that changes whenever any cache lands, to redraw on. */
let version = 0;
listeners.add(() => {
  version++;
});

export function usePeaksVersion(): number {
  return useSyncExternalStore(
    subscribe,
    () => version,
    () => 0,
  );
}
