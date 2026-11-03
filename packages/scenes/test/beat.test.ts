import { describe, expect, it } from 'vitest';
import { BEAT_DECAY, BEAT_REFRACTORY, initialBeatState, stepBeat } from '../src/beat';

const FPS = 60;

/** Runs a bass signal (one value per frame) through the detector. */
function run(signal: (t: number, i: number) => number, seconds: number) {
  let state = initialBeatState;
  const onsets: number[] = [];
  const beats: number[] = [];
  for (let i = 0; i < seconds * FPS; i++) {
    const t = i / FPS;
    const r = stepBeat(state, signal(t, i), t);
    state = r.state;
    beats.push(r.beat);
    if (r.onset) onsets.push(t);
  }
  return { onsets, beats };
}

/** A kick every `period` seconds: a 50 ms spike over a quiet floor. */
const kicks = (period: number, floor = 0.15, peak = 0.7) => (t: number) => ((t % period) < 0.05 ? peak : floor);

describe('stepBeat', () => {
  it('never fires on silence', () => {
    expect(run(() => 0, 5).onsets).toEqual([]);
  });

  it('never fires on a steady level', () => {
    expect(run(() => 0.5, 5).onsets).toEqual([]);
  });

  it('fires once per kick at 120 BPM', () => {
    const { onsets } = run(kicks(0.5), 10);
    // The first kick lands during warm-up; every later one is detected.
    expect(onsets.length).toBeGreaterThanOrEqual(18);
    expect(onsets.length).toBeLessThanOrEqual(20);
    for (let i = 1; i < onsets.length; i++) expect(onsets[i] - onsets[i - 1]).toBeCloseTo(0.5, 1);
  });

  it('respects the 250 ms refractory period', () => {
    const { onsets } = run(kicks(0.1, 0.1, 0.8), 5);
    expect(onsets.length).toBeGreaterThan(0);
    for (let i = 1; i < onsets.length; i++) expect(onsets[i] - onsets[i - 1]).toBeGreaterThanOrEqual(BEAT_REFRACTORY - 1e-9);
  });

  it('ignores spikes that stay under the minimum level', () => {
    expect(run(kicks(0.5, 0.01, 0.05), 5).onsets).toEqual([]);
  });

  it('needs the spike to beat 1.4x the recent average', () => {
    // 1.3x bumps over a steady floor should not count.
    expect(run(kicks(0.5, 0.4, 0.52), 5).onsets).toEqual([]);
  });

  it('outputs 1 on onset and decays to 0 over ~300 ms', () => {
    const { onsets, beats } = run(kicks(1), 4);
    const i = Math.round(onsets[0] * FPS);
    expect(beats[i]).toBe(1);
    const half = Math.round((BEAT_DECAY / 2) * FPS);
    expect(beats[i + half]).toBeGreaterThan(0.4);
    expect(beats[i + half]).toBeLessThan(0.6);
    expect(beats[i + Math.ceil(BEAT_DECAY * FPS) + 1]).toBe(0);
  });

  it('is pure: the same input gives the same output', () => {
    const a = run(kicks(0.5), 3);
    const b = run(kicks(0.5), 3);
    expect(a).toEqual(b);
  });
});
