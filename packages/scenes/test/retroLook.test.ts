import { describe, expect, it } from 'vitest';
import { PALETTES } from '../src/renderers/retro/palettes';
import { quantize } from '../src/retroLook';

const pack = (r: number, g: number, b: number) => (0xff000000 | (b << 16) | (g << 8) | r) >>> 0;

function gradient(w: number, h: number): Uint32Array {
  const px = new Uint32Array(w * h);
  for (let i = 0; i < px.length; i++) {
    const v = Math.round(((i % w) / (w - 1)) * 255);
    px[i] = pack(v, (v * 3) & 255, 255 - v);
  }
  return px;
}

describe('quantize', () => {
  it('limits VGA to 4 levels per channel (64 colors)', () => {
    const px = gradient(64, 8);
    quantize(px, 64, PALETTES.vga);
    for (const c of px) for (const shift of [0, 8, 16]) expect([0, 85, 170, 255]).toContain((c >>> shift) & 255);
  });

  it('maps mono palettes onto the palette ramp only', () => {
    const allowed = new Set(Array.from(PALETTES.green.heat, (c) => c >>> 0));
    const px = gradient(64, 8);
    quantize(px, 64, PALETTES.green);
    for (const c of px) expect(allowed.has(c >>> 0)).toBe(true);
    expect(new Set(px).size).toBeLessThanOrEqual(10);
  });

  it('keeps black black and white bright', () => {
    const px = new Uint32Array([pack(0, 0, 0), pack(255, 255, 255)]);
    quantize(px, 2, PALETTES.amber);
    expect(px[0] >>> 0).toBe(PALETTES.amber.heat[0] >>> 0);
    expect(px[1] >>> 0).toBe(PALETTES.amber.heat[255] >>> 0);
  });
});
