/*
 * The binary frame the engine streams at display rate. One format carries
 * the meters, the pedal readouts and the analysis stream (spectrum,
 * waveform, level, beat) the stage draws from.
 * Layout in docs/frames.md; the encoder is src-tauri/src/frame.rs and the
 * two must agree. Little-endian throughout; the 48-byte header is a
 * multiple of four so the body can be viewed as Float32Arrays in place.
 */

export const FRAME_MAGIC = "CNTF";
export const FRAME_VERSION = 3;
export const HEADER_LEN = 48;

const FLAG_PLAYING = 1 << 0;
const FLAG_RECORDING = 1 << 1;
const FLAG_BEAT = 1 << 2;
const FLAG_ANALYSIS = 1 << 3;
/** The analysis trailer after the readouts: RMS and source. */
const ANALYSIS_LEN = 8;

export interface Beat {
  bpm: number;
  /** 0 at the beat, rising to 1 just before the next. */
  phase: number;
  bar: number;
  beat: number;
}

/** What the spectrum and waveform were taken from, and its level. */
export interface Analysis {
  /** RMS of the analysis window, linear. */
  rms: number;
  /** The meter measuring the same signal: 1 the master, 2 and up a track. */
  source: number;
}

export interface Frame {
  sequence: number;
  /** Frames the engine skipped because the UI had not taken the last one. */
  dropped: number;
  /** Timeline position in seconds. */
  position: number;
  playing: boolean;
  recording: boolean;
  beat: Beat | null;
  /** Peak and RMS pairs, linear: [peak0, rms0, peak1, rms1, ...]. */
  meters: Float32Array;
  /**
   * Log-spaced from 20 Hz to 20 kHz (or Nyquist), each bin its band's
   * magnitude as a sine's amplitude: a full-scale tone reads 1.
   */
  spectrum: Float32Array;
  /** The most recent samples, mono, oldest first, -1 to 1. */
  waveform: Float32Array;
  /**
   * Every pedal readout in the graph, in track and chain order; the
   * status says which pedal owns how many (see readouts.ts).
   */
  readouts: Float32Array;
  /** Null in a frame without the analysis sections. */
  analysis: Analysis | null;
}

/**
 * Reads a frame. The typed arrays are views on `buffer`, not copies, so
 * hold the frame rather than the buffer. Throws on a buffer that is not a
 * frame; a newer version with a larger header is still read, since the
 * header length is fixed and unknown body sections are ignored.
 */
export function decodeFrame(buffer: ArrayBuffer): Frame {
  if (buffer.byteLength < HEADER_LEN) {
    throw new Error(`frame is ${buffer.byteLength} bytes, shorter than its header`);
  }
  const view = new DataView(buffer);
  const magic = String.fromCharCode(
    view.getUint8(0),
    view.getUint8(1),
    view.getUint8(2),
    view.getUint8(3),
  );
  if (magic !== FRAME_MAGIC) {
    throw new Error(`not a frame: starts with ${JSON.stringify(magic)}`);
  }
  const version = view.getUint16(4, true);
  if (version > FRAME_VERSION) {
    throw new Error(`frame version ${version} is newer than this UI understands`);
  }
  const flags = view.getUint16(6, true);
  const meterCount = view.getUint16(24, true);
  const spectrumBins = view.getUint16(26, true);
  const waveformSamples = view.getUint16(28, true);
  // Version 1 left this field reserved as zero.
  const readoutCount = view.getUint16(30, true);
  const hasAnalysis = (flags & FLAG_ANALYSIS) !== 0;
  const need =
    HEADER_LEN +
    4 * (meterCount * 2 + spectrumBins + waveformSamples + readoutCount) +
    (hasAnalysis ? ANALYSIS_LEN : 0);
  if (buffer.byteLength < need) {
    throw new Error(`frame body is short: ${buffer.byteLength} of ${need} bytes`);
  }
  let at = HEADER_LEN;
  const meters = new Float32Array(buffer, at, meterCount * 2);
  at += meterCount * 8;
  const spectrum = new Float32Array(buffer, at, spectrumBins);
  at += spectrumBins * 4;
  const waveform = new Float32Array(buffer, at, waveformSamples);
  at += waveformSamples * 4;
  const readouts = new Float32Array(buffer, at, readoutCount);
  at += readoutCount * 4;
  return {
    sequence: view.getUint32(8, true),
    dropped: view.getUint32(12, true),
    position: view.getFloat64(16, true),
    playing: (flags & FLAG_PLAYING) !== 0,
    recording: (flags & FLAG_RECORDING) !== 0,
    beat:
      (flags & FLAG_BEAT) !== 0
        ? {
            bpm: view.getFloat32(32, true),
            phase: view.getFloat32(36, true),
            bar: view.getUint32(40, true),
            beat: view.getUint32(44, true),
          }
        : null,
    meters,
    spectrum,
    waveform,
    readouts,
    analysis: hasAnalysis
      ? { rms: view.getFloat32(at, true), source: view.getUint32(at + 4, true) }
      : null,
  };
}

/** Writes a frame. The browser stand-in and the tests use it; the app only decodes. */
export function encodeFrame(
  frame: Omit<Frame, "meters" | "spectrum" | "waveform" | "readouts" | "analysis"> & {
    meters: ArrayLike<number>;
    spectrum?: ArrayLike<number>;
    waveform?: ArrayLike<number>;
    readouts?: ArrayLike<number>;
    analysis?: Analysis | null;
  },
): ArrayBuffer {
  const spectrum = frame.spectrum ?? [];
  const waveform = frame.waveform ?? [];
  const readouts = frame.readouts ?? [];
  const analysis = frame.analysis ?? null;
  const buffer = new ArrayBuffer(
    HEADER_LEN +
      4 * (frame.meters.length + spectrum.length + waveform.length + readouts.length) +
      (analysis ? ANALYSIS_LEN : 0),
  );
  const view = new DataView(buffer);
  for (let i = 0; i < 4; i++) view.setUint8(i, FRAME_MAGIC.charCodeAt(i));
  view.setUint16(4, FRAME_VERSION, true);
  let flags = 0;
  if (frame.playing) flags |= FLAG_PLAYING;
  if (frame.recording) flags |= FLAG_RECORDING;
  if (frame.beat) flags |= FLAG_BEAT;
  if (analysis) flags |= FLAG_ANALYSIS;
  view.setUint16(6, flags, true);
  view.setUint32(8, frame.sequence >>> 0, true);
  view.setUint32(12, frame.dropped >>> 0, true);
  view.setFloat64(16, frame.position, true);
  view.setUint16(24, frame.meters.length / 2, true);
  view.setUint16(26, spectrum.length, true);
  view.setUint16(28, waveform.length, true);
  view.setUint16(30, readouts.length, true);
  view.setFloat32(32, frame.beat?.bpm ?? 0, true);
  view.setFloat32(36, frame.beat?.phase ?? 0, true);
  view.setUint32(40, frame.beat?.bar ?? 0, true);
  view.setUint32(44, frame.beat?.beat ?? 0, true);
  let at = HEADER_LEN;
  for (const lane of [frame.meters, spectrum, waveform, readouts]) {
    for (let i = 0; i < lane.length; i++) {
      view.setFloat32(at, lane[i] ?? 0, true);
      at += 4;
    }
  }
  if (analysis) {
    view.setFloat32(at, analysis.rms, true);
    view.setUint32(at + 4, analysis.source, true);
  }
  return buffer;
}

/** Spectrum bins and waveform samples the engine sends; see analysis.rs. */
export const SPECTRUM_BINS = 128;
export const WAVEFORM_SAMPLES = 2048;
export const SPECTRUM_LOW_HZ = 20;
export const SPECTRUM_HIGH_HZ = 20_000;

/** The lower edge of spectrum bin `i` of `bins`, in Hz. */
export function spectrumBinHz(i: number, bins = SPECTRUM_BINS, sampleRate = 48_000): number {
  const high = Math.min(SPECTRUM_HIGH_HZ, sampleRate / 2);
  return SPECTRUM_LOW_HZ * (high / SPECTRUM_LOW_HZ) ** (i / bins);
}

/** What the stage draws from: one per frame, the scenes' one input. */
export interface AnalysisFrame {
  /** Log-spaced magnitudes, see `Frame.spectrum`. */
  spectrum: Float32Array;
  /** Recent samples, mono, oldest first. */
  waveform: Float32Array;
  /** RMS of the analysis window, linear. */
  rms: number;
  /** Bar, beat and phase from the tempo map; null while stopped. */
  beat: Beat | null;
  playing: boolean;
  /** Timeline position in seconds. */
  position: number;
}

/** The analysis part of a frame, or null for one without it. */
export function analysisOf(frame: Frame): AnalysisFrame | null {
  if (!frame.analysis) return null;
  return {
    spectrum: frame.spectrum,
    waveform: frame.waveform,
    rms: frame.analysis.rms,
    beat: frame.beat,
    playing: frame.playing,
    position: frame.position,
  };
}
