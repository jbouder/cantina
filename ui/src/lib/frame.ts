/*
 * The binary frame the engine streams at display rate. One format carries
 * the meters now and the analysis stream (spectrum, waveform, beat) in M8.
 * Layout in docs/frames.md; the encoder is src-tauri/src/frame.rs and the
 * two must agree. Little-endian throughout; the 48-byte header is a
 * multiple of four so the body can be viewed as Float32Arrays in place.
 */

export const FRAME_MAGIC = "CNTF";
export const FRAME_VERSION = 1;
export const HEADER_LEN = 48;

const FLAG_PLAYING = 1 << 0;
const FLAG_RECORDING = 1 << 1;
const FLAG_BEAT = 1 << 2;

export interface Beat {
  bpm: number;
  /** 0 at the beat, rising to 1 just before the next. */
  phase: number;
  bar: number;
  beat: number;
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
  spectrum: Float32Array;
  waveform: Float32Array;
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
  const need = HEADER_LEN + 4 * (meterCount * 2 + spectrumBins + waveformSamples);
  if (buffer.byteLength < need) {
    throw new Error(`frame body is short: ${buffer.byteLength} of ${need} bytes`);
  }
  let at = HEADER_LEN;
  const meters = new Float32Array(buffer, at, meterCount * 2);
  at += meterCount * 8;
  const spectrum = new Float32Array(buffer, at, spectrumBins);
  at += spectrumBins * 4;
  const waveform = new Float32Array(buffer, at, waveformSamples);
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
  };
}

/** Writes a frame. The browser stand-in and the tests use it; the app only decodes. */
export function encodeFrame(
  frame: Omit<Frame, "meters" | "spectrum" | "waveform"> & {
    meters: ArrayLike<number>;
    spectrum?: ArrayLike<number>;
    waveform?: ArrayLike<number>;
  },
): ArrayBuffer {
  const spectrum = frame.spectrum ?? [];
  const waveform = frame.waveform ?? [];
  const buffer = new ArrayBuffer(
    HEADER_LEN + 4 * (frame.meters.length + spectrum.length + waveform.length),
  );
  const view = new DataView(buffer);
  for (let i = 0; i < 4; i++) view.setUint8(i, FRAME_MAGIC.charCodeAt(i));
  view.setUint16(4, FRAME_VERSION, true);
  let flags = 0;
  if (frame.playing) flags |= FLAG_PLAYING;
  if (frame.recording) flags |= FLAG_RECORDING;
  if (frame.beat) flags |= FLAG_BEAT;
  view.setUint16(6, flags, true);
  view.setUint32(8, frame.sequence >>> 0, true);
  view.setUint32(12, frame.dropped >>> 0, true);
  view.setFloat64(16, frame.position, true);
  view.setUint16(24, frame.meters.length / 2, true);
  view.setUint16(26, spectrum.length, true);
  view.setUint16(28, waveform.length, true);
  view.setUint16(30, 0, true);
  view.setFloat32(32, frame.beat?.bpm ?? 0, true);
  view.setFloat32(36, frame.beat?.phase ?? 0, true);
  view.setUint32(40, frame.beat?.bar ?? 0, true);
  view.setUint32(44, frame.beat?.beat ?? 0, true);
  let at = HEADER_LEN;
  for (const lane of [frame.meters, spectrum, waveform]) {
    for (let i = 0; i < lane.length; i++) {
      view.setFloat32(at, lane[i] ?? 0, true);
      at += 4;
    }
  }
  return buffer;
}
