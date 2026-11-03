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
  /** 1 on onset, decays to 0 over ~300 ms. */
  beat: number;
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
  };
}
