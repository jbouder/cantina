/** Multiplies a packed color's channels by k (0-1+), clamped. */
export function scale(c: number, k: number): number {
  const r = Math.min(255, (c & 255) * k);
  const g = Math.min(255, ((c >>> 8) & 255) * k);
  const b = Math.min(255, ((c >>> 16) & 255) * k);
  return 0xff000000 | (b << 16) | (g << 8) | r;
}

/** Linear blend of two packed colors. */
export function blend(a: number, b: number, t: number): number {
  const u = 1 - t;
  const r = (a & 255) * u + (b & 255) * t;
  const g = ((a >>> 8) & 255) * u + ((b >>> 8) & 255) * t;
  const bl = ((a >>> 16) & 255) * u + ((b >>> 16) & 255) * t;
  return 0xff000000 | (bl << 16) | (g << 8) | r;
}

/** Tileable value noise in 0-255, size x size, summed over a few octaves. */
export function noiseTexture(size: number, octaves = 4, seed = 1): Uint8Array {
  let s = seed;
  const rand = () => ((s = (s * 16807) % 2147483647) / 2147483647);
  const out = new Float32Array(size * size);
  let amp = 0.5;
  let total = 0;
  for (let o = 0, cell = size / 8; o < octaves && cell >= 1; o++, cell /= 2, amp /= 2) {
    const n = size / cell;
    const lattice = Float32Array.from({ length: n * n }, rand);
    for (let y = 0; y < size; y++) {
      const gy = y / cell;
      const y0 = Math.floor(gy) % n;
      const y1 = (y0 + 1) % n;
      let fy = gy - Math.floor(gy);
      fy = fy * fy * (3 - 2 * fy);
      for (let x = 0; x < size; x++) {
        const gx = x / cell;
        const x0 = Math.floor(gx) % n;
        const x1 = (x0 + 1) % n;
        let fx = gx - Math.floor(gx);
        fx = fx * fx * (3 - 2 * fx);
        const top = lattice[y0 * n + x0] * (1 - fx) + lattice[y0 * n + x1] * fx;
        const bot = lattice[y1 * n + x0] * (1 - fx) + lattice[y1 * n + x1] * fx;
        out[y * size + x] += (top * (1 - fy) + bot * fy) * amp;
      }
    }
    total += amp;
  }
  return Uint8Array.from(out, (v) => Math.min(255, (v / total) * 255));
}

/** Max level in each [start, end) bin range, 0-1. */
export function bandLevels(freq: Uint8Array, ranges: [number, number][], out: Float32Array): Float32Array {
  for (let i = 0; i < ranges.length; i++) {
    let v = 0;
    for (let k = ranges[i][0]; k < ranges[i][1]; k++) if (freq[k] > v) v = freq[k];
    out[i] = v / 255;
  }
  return out;
}
