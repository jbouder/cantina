/*
 * The browser stand-in's analysis stream: a bass note and a chord with a
 * kick on every beat while the transport plays, so the stage has
 * something to draw without an engine. Shaped like the engine's output:
 * the same bin count, sample count and scale.
 */

import { type Beat, SPECTRUM_BINS, spectrumBinHz, WAVEFORM_SAMPLES } from "./frame";

const RATE = 48_000;
const TONES = [
  { hz: 82.4, amp: 0.3 },
  { hz: 196, amp: 0.15 },
  { hz: 247, amp: 0.12 },
  { hz: 294, amp: 0.1 },
  { hz: 1568, amp: 0.03 },
];

export interface MockAnalysis {
  spectrum: number[];
  waveform: number[];
  rms: number;
}

/** The beat at `seconds` into a steady tempo map. */
export function mockBeat(seconds: number, bpm: number, beatsPerBar: number): Beat {
  const beats = (seconds * bpm) / 60;
  const whole = Math.floor(beats);
  return {
    bpm,
    phase: beats - whole,
    bar: Math.floor(whole / beatsPerBar),
    beat: whole % beatsPerBar,
  };
}

/** One frame's analysis at time `t`, with the kick from `beat` if any. */
export function mockAnalysis(t: number, beat: Beat | null, level: number): MockAnalysis {
  // A kick: a falling sine that decays over the first fifth of a beat.
  const kick = beat ? Math.max(0, 1 - beat.phase * 5) : 0;
  const waveform = new Array<number>(WAVEFORM_SAMPLES);
  let sum = 0;
  for (let i = 0; i < WAVEFORM_SAMPLES; i++) {
    const s = t + (i - WAVEFORM_SAMPLES) / RATE;
    let v = 0;
    for (const tone of TONES) v += tone.amp * Math.sin(2 * Math.PI * tone.hz * s);
    v = v * level + 0.6 * kick * Math.sin(2 * Math.PI * 55 * s);
    waveform[i] = v;
    sum += v * v;
  }
  const spectrum = new Array<number>(SPECTRUM_BINS);
  for (let b = 0; b < SPECTRUM_BINS; b++) {
    const lo = spectrumBinHz(b);
    const hi = spectrumBinHz(b + 1);
    let m = 0.002 * level * Math.exp(-lo / 6000);
    for (const tone of TONES) if (tone.hz >= lo && tone.hz < hi) m = Math.max(m, tone.amp * level);
    if (55 >= lo && 55 < hi) m = Math.max(m, 0.6 * kick);
    spectrum[b] = m;
  }
  return { spectrum, waveform, rms: Math.sqrt(sum / WAVEFORM_SAMPLES) };
}
