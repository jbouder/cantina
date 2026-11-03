import { describe, expect, it } from 'vitest';
import { FrameAnalyzer } from '../src/analyzer';
import { BEAT_DECAY } from '../src/beat';
import type { AudioFrame } from '../src/frame';
import { EngineFrames } from '../src/sources/engine';
import { RetroScene } from '../src/renderers/retro/RetroScene';

const tempo = (bar: number, beat: number, phase: number) => ({ bpm: 120, phase, bar, beat });

describe('beat-locked frames', () => {
  it('follows the tempo map: 1 on the beat, decaying over the onset time', () => {
    const a = new FrameAnalyzer(48_000);
    expect(a.update(0, tempo(0, 0, 0))).toMatchObject({ beat: 1, downbeat: 1 });
    // At 120 BPM a beat is 0.5 s, so phase 0.3 is 0.15 s after it.
    const f = a.update(0.15, tempo(0, 1, 0.3));
    expect(f.beat).toBeCloseTo(1 - 0.15 / BEAT_DECAY);
    expect(f.downbeat).toBe(0);
    expect(f.tempo).toEqual(tempo(0, 1, 0.3));
    expect(a.update(0.4, tempo(0, 1, 0.8)).beat).toBe(0);
  });

  it('pulses on the beat with no onset in the audio at all', () => {
    const a = new FrameAnalyzer(48_000);
    // Silence: onset detection would never fire.
    for (let i = 0; i < 20; i++) a.update(i / 60, tempo(1, 2, 0.5));
    expect(a.update(1, tempo(1, 3, 0)).beat).toBe(1);
  });

  it('falls back to onsets when the beat is null', () => {
    const a = new FrameAnalyzer(48_000);
    // A steady quiet bass, then a jump: an onset.
    for (let i = 0; i < 20; i++) {
      a.frame.freq.fill(0).fill(40, 0, 12);
      a.update(i / 60, null);
    }
    a.frame.freq.fill(0).fill(255, 0, 12);
    const f = a.update(20 / 60, null);
    expect(f.beat).toBe(1);
    expect(f.downbeat).toBe(0);
    expect(f.tempo).toBeNull();
  });

  it('carries the engine beat through EngineFrames', () => {
    const source = new EngineFrames();
    source.push({ spectrum: new Float32Array(128), waveform: new Float32Array(2048), rms: 0, beat: tempo(4, 0, 0) });
    expect(source.readFrame()).toMatchObject({ beat: 1, downbeat: 1, tempo: tempo(4, 0, 0) });
    source.push({ spectrum: new Float32Array(128), waveform: new Float32Array(2048), rms: 0, beat: null });
    expect(source.readFrame().tempo).toBeNull();
  });
});

class Probe extends RetroScene {
  readonly id = 'probe';
  readonly name = 'Probe';
  protected readonly W = 1;
  protected readonly H = 1;
  protected step(): void {}
  beats(frame: AudioFrame) {
    return this.beatOnset(frame);
  }
  bars(frame: AudioFrame) {
    return this.barOnset(frame);
  }
}

describe('beat and bar onsets in scenes', () => {
  const at = (bar: number, beat: number, phase: number, envelope = 0): AudioFrame =>
    ({ beat: envelope, downbeat: 0, tempo: tempo(bar, beat, phase) }) as unknown as AudioFrame;

  it('fires once per beat and once per bar on the grid', () => {
    const p = new Probe();
    const steps = [at(0, 0, 0), at(0, 0, 0.5), at(0, 1, 0), at(0, 1, 0.5), at(0, 3, 0.9), at(1, 0, 0), at(1, 0, 0.2)];
    expect(steps.map((f) => p.beats(f))).toEqual([true, false, true, false, true, true, false]);
    const q = new Probe();
    expect(steps.map((f) => q.bars(f))).toEqual([true, false, false, false, false, true, false]);
  });

  it('never finds a bar without a tempo, and finds beats from the envelope', () => {
    const p = new Probe();
    const loose = (envelope: number) => ({ beat: envelope, downbeat: 0, tempo: null }) as unknown as AudioFrame;
    expect([loose(0), loose(1), loose(0.8), loose(0.1), loose(1)].map((f) => p.beats(f))).toEqual([false, true, false, false, true]);
    expect(p.bars(loose(1))).toBe(false);
  });
});
