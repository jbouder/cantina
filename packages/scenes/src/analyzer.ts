import { stepBeat, initialBeatState, type BeatState } from './beat';
import { createSilentFrame, type AudioFrame } from './frame';

export const BANDS = {
  bass: [20, 250],
  mid: [250, 2000],
  treble: [2000, 16000],
} as const;

/** Bin index for a frequency, clamped to the spectrum. */
export function hzToBin(hz: number, sampleRate: number, binCount: number): number {
  const binHz = sampleRate / 2 / binCount;
  return Math.min(binCount - 1, Math.max(0, Math.round(hz / binHz)));
}

/** Mean level of the bins covering [lo, hi] Hz, 0-1. */
export function bandLevel(freq: Uint8Array, lo: number, hi: number, sampleRate: number): number {
  const a = hzToBin(lo, sampleRate, freq.length);
  const b = Math.max(a, hzToBin(hi, sampleRate, freq.length));
  let sum = 0;
  for (let i = a; i <= b; i++) sum += freq[i];
  return sum / ((b - a + 1) * 255);
}

/** RMS of an 8-bit time-domain buffer where 128 is silence, 0-1. */
export function waveRms(wave: Uint8Array): number {
  let sum = 0;
  for (let i = 0; i < wave.length; i++) {
    const v = (wave[i] - 128) / 128;
    sum += v * v;
  }
  return Math.min(1, Math.sqrt(sum / wave.length));
}

/**
 * Turns raw analyser buffers into AudioFrames. The frame object and its arrays
 * are reused every call, so renderers must read them synchronously.
 */
export class FrameAnalyzer {
  readonly frame: AudioFrame = createSilentFrame();
  private beatState: BeatState = initialBeatState;

  constructor(private readonly sampleRate: number) {}

  update(time: number): AudioFrame {
    const f = this.frame;
    f.time = time;
    f.rms = waveRms(f.wave);
    f.bass = bandLevel(f.freq, BANDS.bass[0], BANDS.bass[1], this.sampleRate);
    f.mid = bandLevel(f.freq, BANDS.mid[0], BANDS.mid[1], this.sampleRate);
    f.treble = bandLevel(f.freq, BANDS.treble[0], BANDS.treble[1], this.sampleRate);
    const r = stepBeat(this.beatState, f.bass, time);
    this.beatState = r.state;
    f.beat = r.beat;
    return f;
  }

  reset(): void {
    this.beatState = initialBeatState;
    this.frame.freq.fill(0);
    this.frame.wave.fill(128);
  }
}
