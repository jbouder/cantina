import { describe, expect, it } from 'vitest';
import { EngineFrames } from '../src/sources/engine';

const BINS = 128;

/** An engine spectrum with one tone at `hz` of amplitude `amp`. */
function tone(hz: number, amp: number): Float32Array {
  const spectrum = new Float32Array(BINS);
  const at = Math.floor((BINS * Math.log(hz / 20)) / Math.log(20_000 / 20));
  spectrum[at] = amp;
  return spectrum;
}

function settle(source: EngineFrames, spectrum: Float32Array, waveform = new Float32Array(2048)) {
  let frame = source.readFrame();
  for (let i = 0; i < 60; i++) {
    source.push({ spectrum, waveform, rms: 0.5, beat: null });
    frame = source.readFrame();
  }
  return frame;
}

describe('engine frames', () => {
  it('puts a tone in the linear bins around its frequency, at analyser height', () => {
    const frame = settle(new EngineFrames(), tone(1000, 1));
    // Bin k is k * 48000 / 2048 Hz, so 1 kHz is bin 43.
    const peak = frame.freq.indexOf(Math.max(...frame.freq));
    expect(Math.abs(peak - 43)).toBeLessThanOrEqual(3);
    // A full-scale tone sits near the top of the analyser's range, not clipped flat.
    expect(frame.freq[peak]).toBeGreaterThan(200);
    expect(frame.freq[400]).toBe(0);
    expect(frame.mid).toBeGreaterThan(frame.treble);
  });

  it('is quieter for a quieter tone', () => {
    const loud = settle(new EngineFrames(), tone(200, 1)).freq.slice();
    const soft = settle(new EngineFrames(), tone(200, 0.01)).freq;
    expect(Math.max(...soft)).toBeLessThan(Math.max(...loud));
  });

  it('turns the waveform into bytes around 128', () => {
    const waveform = new Float32Array(2048).map((_, i) => (i % 2 ? 0.5 : -0.5));
    const frame = settle(new EngineFrames(), new Float32Array(BINS), waveform);
    expect(frame.wave[0]).toBe(64);
    expect(frame.wave[1]).toBe(192);
    expect(frame.rms).toBeCloseTo(0.5, 1);
  });

  it('stretches a short waveform and reads silence before any push', () => {
    const source = new EngineFrames();
    expect(source.readFrame().wave.every((v) => v === 128)).toBe(true);
    const frame = settle(source, new Float32Array(BINS), new Float32Array([-1, 1]));
    expect(frame.wave[0]).toBe(0);
    expect(frame.wave[2047]).toBe(255);
    expect(source.beat).toBeNull();
  });
});
