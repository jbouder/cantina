import { describe, expect, it } from "vitest";
import { buildPeaks, columns, levelFor } from "./peaks";

function ramp(n: number) {
  const s = new Float32Array(n);
  for (let i = 0; i < n; i++) s[i] = Math.sin((i / n) * Math.PI * 8) * (i / n);
  return s;
}

describe("buildPeaks", () => {
  it("reduces every bin to its true min and max at every level", () => {
    const samples = ramp(10_000);
    const cache = buildPeaks(samples, [4, 16, 64]);
    expect(cache.frames).toBe(10_000);
    expect(cache.levels.map((l) => l.bin)).toEqual([4, 16, 64]);
    for (const level of cache.levels) {
      expect(level.min.length).toBe(Math.ceil(10_000 / level.bin));
      for (let b = 0; b < level.min.length; b++) {
        const slice = samples.subarray(b * level.bin, (b + 1) * level.bin);
        expect(level.min[b]).toBeCloseTo(Math.min(...slice), 6);
        expect(level.max[b]).toBeCloseTo(Math.max(...slice), 6);
      }
    }
  });

  it("handles an empty file", () => {
    const cache = buildPeaks(new Float32Array(0), [4]);
    expect(cache.frames).toBe(0);
    expect(cache.levels[0]?.min.length).toBe(0);
    expect(columns(cache, 0, 100, 4)).toEqual(new Float32Array(8));
  });
});

describe("levelFor", () => {
  it("picks the coarsest level that still has a bin per pixel", () => {
    const cache = buildPeaks(ramp(100_000), [64, 256, 1024]);
    expect(levelFor(cache, 10).bin).toBe(64);
    expect(levelFor(cache, 64).bin).toBe(64);
    expect(levelFor(cache, 300).bin).toBe(256);
    expect(levelFor(cache, 1e6).bin).toBe(1024);
  });
});

describe("columns", () => {
  it("covers each pixel's span and reads silence past the end", () => {
    const samples = new Float32Array(1000);
    samples[500] = 1;
    samples[501] = -1;
    const cache = buildPeaks(samples, [4, 16]);
    const cols = columns(cache, 0, 1000, 10);
    expect(cols[5 * 2]).toBe(-1);
    expect(cols[5 * 2 + 1]).toBe(1);
    expect(cols[0]).toBe(0);
    const past = columns(cache, 900, 1900, 10);
    expect(past[0]).toBe(0);
    expect(past[19]).toBe(0);
  });
});
