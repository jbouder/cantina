import { describe, expect, it } from 'vitest';
import { FrameAnalyzer, bandLevel, hzToBin, waveRms } from '../src/analyzer';

const SR = 48000;
const BINS = 1024;

/** A spectrum with full level between lo and hi Hz and nothing elsewhere. */
function spectrum(lo: number, hi: number): Uint8Array {
  const f = new Uint8Array(BINS);
  for (let i = hzToBin(lo, SR, BINS); i <= hzToBin(hi, SR, BINS); i++) f[i] = 255;
  return f;
}

/** An 8-bit sine wave with amplitude 0-1. */
function sine(amplitude: number, cycles = 8, n = 2048): Uint8Array {
  const w = new Uint8Array(n);
  for (let i = 0; i < n; i++) w[i] = Math.round(128 + 127 * amplitude * Math.sin((2 * Math.PI * cycles * i) / n));
  return w;
}

describe('hzToBin', () => {
  it('maps frequency to the nearest bin and clamps', () => {
    expect(hzToBin(0, SR, BINS)).toBe(0);
    expect(hzToBin(SR / 4, SR, BINS)).toBe(512);
    expect(hzToBin(SR, SR, BINS)).toBe(BINS - 1);
  });
});

describe('bandLevel', () => {
  it('isolates bass, mid and treble', () => {
    const f = spectrum(20, 250);
    expect(bandLevel(f, 20, 250, SR)).toBeCloseTo(1, 5);
    expect(bandLevel(f, 2000, 16000, SR)).toBe(0);
    const t = spectrum(2000, 16000);
    expect(bandLevel(t, 2000, 16000, SR)).toBeCloseTo(1, 5);
    expect(bandLevel(t, 20, 250, SR)).toBe(0);
  });

  it('is zero for silence', () => {
    expect(bandLevel(new Uint8Array(BINS), 20, 16000, SR)).toBe(0);
  });
});

describe('waveRms', () => {
  it('is 0 for silence', () => {
    expect(waveRms(new Uint8Array(2048).fill(128))).toBe(0);
  });

  it('is amplitude / sqrt(2) for a sine', () => {
    // 8-bit samples peak at 127/128 of full scale.
    const peak = 127 / 128;
    expect(waveRms(sine(1))).toBeCloseTo(peak / Math.SQRT2, 3);
    expect(waveRms(sine(0.5))).toBeCloseTo((0.5 * peak) / Math.SQRT2, 2);
  });
});

describe('FrameAnalyzer', () => {
  it('produces a full AudioFrame from raw buffers', () => {
    const a = new FrameAnalyzer(SR);
    // Stay clear of the 250 Hz and 2 kHz edges, which neighbouring bands share.
    a.frame.freq.set(spectrum(400, 1800));
    a.frame.wave.set(sine(0.8));
    const f = a.update(1.5);
    expect(f.time).toBe(1.5);
    expect(f.freq).toHaveLength(1024);
    expect(f.wave).toHaveLength(2048);
    expect(f.mid).toBeGreaterThan(0.75);
    expect(f.bass).toBe(0);
    expect(f.rms).toBeCloseTo((0.8 * 127) / 128 / Math.SQRT2, 2);
    expect(f.beat).toBe(0);
  });

  it('detects kicks in a synthetic bass signal', () => {
    const a = new FrameAnalyzer(SR);
    const kick = spectrum(40, 200);
    const quiet = new Uint8Array(BINS);
    for (let i = hzToBin(40, SR, BINS); i <= hzToBin(200, SR, BINS); i++) quiet[i] = 40;
    let onsets = 0;
    for (let i = 0; i < 600; i++) {
      const t = i / 60;
      a.frame.freq.set(t % 0.5 < 0.05 ? kick : quiet);
      if (a.update(t).beat === 1) onsets++;
    }
    expect(onsets).toBeGreaterThanOrEqual(18);
  });

  it('reset clears buffers back to silence', () => {
    const a = new FrameAnalyzer(SR);
    a.frame.freq.fill(200);
    a.frame.wave.fill(10);
    a.reset();
    expect(a.update(0)).toMatchObject({ bass: 0, mid: 0, treble: 0, rms: 0, beat: 0 });
  });
});
