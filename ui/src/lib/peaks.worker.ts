/*
 * Builds peak caches off the main thread. Messages in: a file name and
 * its samples; out: the file name and its cache, with the buffers
 * transferred rather than copied.
 */
import { buildPeaks, type PeakCache, transferables } from "./peaks";

export interface PeaksRequest {
  file: string;
  samples: ArrayBuffer;
}

export interface PeaksResponse {
  file: string;
  cache: PeakCache;
}

// The project compiles against the DOM lib, where `self` is a Window;
// in here it is the worker scope, whose postMessage takes transferables.
const scope = self as unknown as {
  onmessage: ((e: MessageEvent<PeaksRequest>) => void) | null;
  postMessage(message: PeaksResponse, transfer: Transferable[]): void;
};

scope.onmessage = (e) => {
  const { file, samples } = e.data;
  const cache = buildPeaks(new Float32Array(samples));
  scope.postMessage({ file, cache }, transferables(cache));
};
