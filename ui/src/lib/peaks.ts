/*
 * Waveform peak caches. A file's samples are reduced once, off the main
 * thread (see peaks.worker.ts), into min and max per bin at several bin
 * sizes. Drawing a clip then reads the level whose bin is just under the
 * zoom's samples per pixel, so a column costs a handful of reads whatever
 * the zoom, and a long file at any zoom never touches its samples again.
 */

/** Bin sizes, in samples, from fine to coarse. */
export const LEVELS = [64, 256, 1024, 4096, 16384] as const;

export interface PeakLevel {
  /** Samples per bin. */
  bin: number;
  min: Float32Array;
  max: Float32Array;
}

export interface PeakCache {
  /** Frames in the file. */
  frames: number;
  /** From fine to coarse. */
  levels: PeakLevel[];
}

/** Reduces samples to every level. Pure, so it runs in a worker or a test. */
export function buildPeaks(samples: Float32Array, levels: readonly number[] = LEVELS): PeakCache {
  const frames = samples.length;
  const out: PeakLevel[] = [];
  // Each level is built from the previous one, so the cost is dominated
  // by the finest level.
  let source: PeakLevel | null = null;
  for (const bin of levels) {
    const count = Math.ceil(frames / bin);
    const min = new Float32Array(count);
    const max = new Float32Array(count);
    if (source === null) {
      for (let b = 0; b < count; b++) {
        let lo = Number.POSITIVE_INFINITY;
        let hi = Number.NEGATIVE_INFINITY;
        const end = Math.min(frames, (b + 1) * bin);
        for (let i = b * bin; i < end; i++) {
          const s = samples[i] ?? 0;
          if (s < lo) lo = s;
          if (s > hi) hi = s;
        }
        min[b] = lo;
        max[b] = hi;
      }
    } else {
      const ratio = bin / source.bin;
      for (let b = 0; b < count; b++) {
        let lo = Number.POSITIVE_INFINITY;
        let hi = Number.NEGATIVE_INFINITY;
        const end = Math.min(source.min.length, (b + 1) * ratio);
        for (let i = b * ratio; i < end; i++) {
          const l = source.min[i] ?? 0;
          const h = source.max[i] ?? 0;
          if (l < lo) lo = l;
          if (h > hi) hi = h;
        }
        min[b] = lo;
        max[b] = hi;
      }
    }
    const level = { bin, min, max };
    out.push(level);
    source = level;
  }
  return { frames, levels: out };
}

/** The coarsest level whose bin is no larger than `samplesPerPixel`. */
export function levelFor(cache: PeakCache, samplesPerPixel: number): PeakLevel {
  let chosen = cache.levels[0];
  if (!chosen) throw new Error("a peak cache needs at least one level");
  for (const level of cache.levels) {
    if (level.bin <= samplesPerPixel) chosen = level;
  }
  return chosen;
}

/**
 * Min and max per pixel column for frames `from` to `to` of the file
 * spread over `columns` pixels, interleaved `[min0, max0, min1, max1, …]`.
 * Columns past the end of the file read as silence.
 */
export function columns(cache: PeakCache, from: number, to: number, columns: number): Float32Array {
  const out = new Float32Array(columns * 2);
  if (columns <= 0 || to <= from) return out;
  const samplesPerPixel = (to - from) / columns;
  const level = levelFor(cache, samplesPerPixel);
  for (let c = 0; c < columns; c++) {
    const a = from + c * samplesPerPixel;
    const b = a + samplesPerPixel;
    const first = Math.floor(a / level.bin);
    const last = Math.max(first, Math.ceil(b / level.bin) - 1);
    let lo = 0;
    let hi = 0;
    let any = false;
    for (let i = first; i <= last && i < level.min.length; i++) {
      const l = level.min[i] ?? 0;
      const h = level.max[i] ?? 0;
      if (!any) {
        lo = l;
        hi = h;
        any = true;
      } else {
        if (l < lo) lo = l;
        if (h > hi) hi = h;
      }
    }
    out[c * 2] = lo;
    out[c * 2 + 1] = hi;
  }
  return out;
}

/** The buffers a cache is built on, for transferring it out of a worker. */
export function transferables(cache: PeakCache): ArrayBuffer[] {
  return cache.levels.flatMap((l) => [l.min.buffer as ArrayBuffer, l.max.buffer as ArrayBuffer]);
}
