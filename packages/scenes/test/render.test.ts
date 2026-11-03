import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { QUALITY_LEVELS, QualityGovernor, nextLowerQuality } from '../src/quality';
import { SCENES, scenesFor } from '../src/registry';
import { logBinRanges } from '../src/renderers/retro/bars';
import { buildLut } from '../src/renderers/retro/palettes';
import { findTrigger } from '../src/renderers/retro/scope';

describe('registry', () => {
  it('ships 9 shader scenes and 13 canvas scenes, and the hidden intro', () => {
    expect(scenesFor('gpu').map((s) => s.id)).toEqual([
      'tunnel', 'grid', 'kaleido', 'blobs', 'nebula', 'blackhole', 'planet', 'hyperspace', 'rain',
    ]);
    expect(scenesFor('retro').filter((s) => !s.hidden).map((s) => s.id)).toEqual([
      'bars', 'scope', 'fire', 'starfield', 'plasma', 'lissajous',
      'trails', 'ring', 'fountain', 'plane', 'flames', 'ripple', 'spectrogram',
    ]);
  });

  it('gives every shader scene a classic counterpart', () => {
    for (const s of scenesFor('gpu')) expect(s.retro, s.id).toBeTypeOf('function');
  });

  it('gives every scene a thumbnail the harness drew', () => {
    for (const s of SCENES) {
      expect(s.thumbnail, s.id).toMatch(new RegExp(`thumbnails/${s.id}\\.png$`));
      expect(existsSync(new URL(s.thumbnail)), s.id).toBe(true);
    }
  });

  it('has unique ids', () => {
    expect(new Set(SCENES.map((s) => s.id)).size).toBe(SCENES.length);
  });
});

describe('QualityGovernor', () => {
  it('steps down after 2 s of frames slower than 22 ms', () => {
    const g = new QualityGovernor();
    let fired = -1;
    for (let i = 0; i < 400 && fired < 0; i++) if (g.sample(0.03)) fired = i;
    expect(fired * 0.03).toBeGreaterThanOrEqual(2);
    expect(fired * 0.03).toBeLessThan(3);
  });

  it('never steps down at 60 fps', () => {
    const g = new QualityGovernor();
    for (let i = 0; i < 1000; i++) expect(g.sample(1 / 60)).toBe(false);
  });

  it('walks 1 → 0.75 → 0.5 and stops', () => {
    expect(QUALITY_LEVELS).toEqual([1, 0.75, 0.5]);
    expect(nextLowerQuality(1)).toBe(0.75);
    expect(nextLowerQuality(0.75)).toBe(0.5);
    expect(nextLowerQuality(0.5)).toBeNull();
  });
});

describe('spectrum bars', () => {
  it('splits the spectrum into 32 increasing, non-overlapping log ranges', () => {
    const r = logBinRanges(32, 1024, 48000);
    expect(r).toHaveLength(32);
    for (let i = 0; i < r.length; i++) {
      expect(r[i][1]).toBeGreaterThan(r[i][0]);
      if (i) expect(r[i][0]).toBe(r[i - 1][1]);
    }
    // Low bars are narrow, high bars are wide.
    expect(r[0][1] - r[0][0]).toBeLessThan(r[31][1] - r[31][0]);
  });
});

describe('oscilloscope trigger', () => {
  it('locks to the first rising zero crossing', () => {
    const w = new Uint8Array(2048);
    for (let i = 0; i < w.length; i++) w[i] = Math.round(128 + 100 * Math.sin((2 * Math.PI * (i + 37)) / 200));
    const t = findTrigger(w, 1024);
    expect(w[t - 1]).toBeLessThan(128);
    expect(w[t]).toBeGreaterThanOrEqual(128);
  });

  it('falls back to 0 for silence', () => {
    expect(findTrigger(new Uint8Array(2048).fill(128), 1024)).toBe(0);
  });
});

describe('palettes', () => {
  it('builds a 256-entry opaque LUT through the stops', () => {
    const lut = buildLut([[0, '#000000'], [1, '#ff0000']]);
    expect(lut).toHaveLength(256);
    expect(lut[0] >>> 24).toBe(255);
    expect(lut[0] & 255).toBe(0);
    expect(lut[255] & 255).toBe(255);
  });
});
