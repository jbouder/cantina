import { FrameAnalyzer } from '../analyzer';
import { BIN_COUNT, FFT_SIZE, type AudioFrame } from '../frame';
import type { FrameSource } from '../Stage';

/**
 * What an engine sends each display frame: the shape of Cantina's
 * `AnalysisFrame`, written out here so the package depends on no app.
 */
export interface EngineAnalysis {
  /** Log-spaced magnitudes from `lowHz` to `highHz`, a full-scale sine reading 1. */
  spectrum: ArrayLike<number>;
  /** Recent samples, mono, oldest first, -1 to 1. */
  waveform: ArrayLike<number>;
  /** RMS of the window, linear. */
  rms: number;
  /** Bar, beat and phase from a tempo map, or null with no transport rolling. */
  beat: { bpm: number; phase: number; bar: number; beat: number } | null;
}

export interface EngineSpectrum {
  sampleRate: number;
  lowHz: number;
  highHz: number;
}

/** The engine's spectrum layout: 20 Hz to 20 kHz at 48 kHz. */
export const ENGINE_SPECTRUM: EngineSpectrum = { sampleRate: 48_000, lowHz: 20, highHz: 20_000 };

/*
 * The scenes were written against a Web Audio AnalyserNode: 1024 linear
 * bins of byte dB from -90 to -10, smoothed by 0.8, and 2048 byte samples.
 * A Blackman window puts a full-scale sine 13.5 dB below the engine's
 * amplitude scale, so the same sound reads the same height either way.
 */
const MIN_DB = -90;
const MAX_DB = -10;
const WINDOW_DB = -13.5;
const SMOOTHING = 0.8;

/**
 * A frame source fed by an engine: push each analysis as it arrives,
 * and the stage reads it as the frame the scenes know.
 */
export class EngineFrames implements FrameSource {
  private readonly analyzer: FrameAnalyzer;
  private readonly smoothed = new Float32Array(BIN_COUNT);
  /**
   * Where each linear bin reads the log spectrum: low down, where a
   * linear bin spans several log bins, the loudest of `first..last`;
   * higher up, where it falls inside one, an interpolation at `at`.
   */
  private readonly first = new Int32Array(BIN_COUNT);
  private readonly last = new Int32Array(BIN_COUNT);
  private readonly at = new Float32Array(BIN_COUNT);
  private latest: EngineAnalysis | null = null;
  private lookupBins = 0;
  private time = 0;

  constructor(private readonly spectrum: EngineSpectrum = ENGINE_SPECTRUM) {
    this.analyzer = new FrameAnalyzer(spectrum.sampleRate);
  }

  /** The newest analysis; the next `readFrame` draws from it. */
  push(analysis: EngineAnalysis): void {
    this.latest = analysis;
  }

  /** The bar, beat and phase of the last analysis pushed, if it had one. */
  get beat(): EngineAnalysis['beat'] {
    return this.latest?.beat ?? null;
  }

  readFrame(): AudioFrame {
    const frame = this.analyzer.frame;
    const a = this.latest;
    this.time = performance.now() / 1000;
    if (!a) return this.analyzer.update(this.time, null);
    this.fillFreq(frame.freq, a.spectrum);
    const wave = frame.wave;
    const n = a.waveform.length;
    for (let i = 0; i < FFT_SIZE; i++) {
      // Stretch a shorter waveform rather than pad it with silence.
      const v = n ? a.waveform[Math.min(n - 1, Math.floor((i * n) / FFT_SIZE))] : 0;
      wave[i] = Math.max(0, Math.min(255, Math.round(128 + 128 * v)));
    }
    return this.analyzer.update(this.time, a.beat);
  }

  private fillFreq(freq: Uint8Array, spectrum: ArrayLike<number>): void {
    const bins = spectrum.length;
    if (bins !== this.lookupBins) this.buildLookup(bins);
    for (let k = 0; k < BIN_COUNT; k++) {
      let m = 0;
      if (bins > 0 && this.first[k] <= this.last[k]) {
        for (let i = this.first[k]; i <= this.last[k]; i++) m = Math.max(m, spectrum[i]);
      } else if (bins > 0 && this.at[k] >= 0) {
        const i = Math.floor(this.at[k]);
        const t = this.at[k] - i;
        m = spectrum[i] * (1 - t) + spectrum[Math.min(bins - 1, i + 1)] * t;
      }
      const s = this.smoothed[k] * SMOOTHING + m * (1 - SMOOTHING);
      this.smoothed[k] = s;
      const db = 20 * Math.log10(Math.max(s, 1e-9)) + WINDOW_DB;
      freq[k] = Math.max(0, Math.min(255, Math.round((255 * (db - MIN_DB)) / (MAX_DB - MIN_DB))));
    }
  }

  private buildLookup(bins: number): void {
    const { sampleRate, lowHz } = this.spectrum;
    const highHz = Math.min(this.spectrum.highHz, sampleRate / 2);
    const span = Math.log(highHz / lowHz);
    // The log bin a frequency falls in, fractionally; below the first, negative.
    const logBin = (hz: number) => (hz <= 0 ? -1 : (bins * Math.log(hz / lowHz)) / span);
    const binHz = sampleRate / FFT_SIZE;
    for (let k = 0; k < BIN_COUNT; k++) {
      const hz = k * binHz;
      const lo = Math.max(0, Math.ceil(logBin(hz - binHz / 2)));
      const hi = Math.min(bins - 1, Math.floor(logBin(hz + binHz / 2)));
      this.first[k] = lo;
      this.last[k] = hi;
      // Each log bin's value stands at the center of its band.
      const at = logBin(hz) - 0.5;
      this.at[k] = hz < lowHz || at > bins - 1 ? -1 : Math.max(0, at);
    }
    this.lookupBins = bins;
  }

  reset(): void {
    this.latest = null;
    this.smoothed.fill(0);
    this.analyzer.reset();
  }
}
