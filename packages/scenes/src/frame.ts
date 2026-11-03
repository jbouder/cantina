/** Where the music is on a tempo map, from a source that has one. */
export interface Tempo {
  bpm: number;
  /** 0 on the beat, rising to just under 1 before the next. */
  phase: number;
  /** Zero-based. */
  bar: number;
  /** Beat within the bar, zero-based: 0 is the downbeat. */
  beat: number;
}

export interface AudioFrame {
  /** Seconds since the engine started. */
  time: number;
  /** 1024 bins, 0-255. */
  freq: Uint8Array;
  /** 2048 samples, 128 = silence. */
  wave: Uint8Array;
  /** 0-1 */
  rms: number;
  /** 20-250 Hz, 0-1 */
  bass: number;
  /** 250-2000 Hz, 0-1 */
  mid: number;
  /** 2000-16000 Hz, 0-1 */
  treble: number;
  /**
   * 1 on a beat, decays to 0 over ~300 ms. On the tempo map's beats when
   * `tempo` is set, otherwise on bass onsets heard in the audio.
   */
  beat: number;
  /** The same envelope on the first beat of each bar only; 0 without a tempo. */
  downbeat: number;
  /** The exact position, or null with no transport rolling. */
  tempo: Tempo | null;
}

export const FFT_SIZE = 2048;
export const BIN_COUNT = FFT_SIZE / 2;

export function createSilentFrame(): AudioFrame {
  return {
    time: 0,
    freq: new Uint8Array(BIN_COUNT),
    wave: new Uint8Array(FFT_SIZE).fill(128),
    rms: 0,
    bass: 0,
    mid: 0,
    treble: 0,
    beat: 0,
    downbeat: 0,
    tempo: null,
  };
}
