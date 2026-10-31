import { describe, expect, it } from "vitest";
import { decodeFrame, encodeFrame, HEADER_LEN } from "./frame";

describe("frame", () => {
  it("round-trips a meters-only frame", () => {
    const buffer = encodeFrame({
      sequence: 9,
      dropped: 1,
      position: 2.5,
      playing: true,
      recording: false,
      beat: null,
      meters: [0.5, 0.25, 0.75, 0.125],
    });
    expect(buffer.byteLength).toBe(HEADER_LEN + 16);
    const frame = decodeFrame(buffer);
    expect(frame.sequence).toBe(9);
    expect(frame.dropped).toBe(1);
    expect(frame.position).toBe(2.5);
    expect(frame.playing).toBe(true);
    expect(frame.recording).toBe(false);
    expect(frame.beat).toBeNull();
    expect(Array.from(frame.meters)).toEqual([0.5, 0.25, 0.75, 0.125]);
    expect(frame.spectrum.length).toBe(0);
    expect(frame.waveform.length).toBe(0);
  });

  it("carries spectrum, waveform and beat for the analysis stream", () => {
    const frame = decodeFrame(
      encodeFrame({
        sequence: 0,
        dropped: 0,
        position: 0,
        playing: false,
        recording: true,
        beat: { bpm: 120, phase: 0.25, bar: 3, beat: 1 },
        meters: [1, 1],
        spectrum: [0.1, 0.2, 0.3],
        waveform: [-1, 1],
      }),
    );
    expect(frame.recording).toBe(true);
    expect(frame.beat).toEqual({ bpm: 120, phase: 0.25, bar: 3, beat: 1 });
    expect(Array.from(frame.spectrum)).toEqual([
      Math.fround(0.1),
      Math.fround(0.2),
      Math.fround(0.3),
    ]);
    expect(Array.from(frame.waveform)).toEqual([-1, 1]);
  });

  it("matches the Rust encoder's header layout byte for byte", () => {
    // The same frame as frame.rs's header test, so the two stay in step.
    const b = new Uint8Array(
      encodeFrame({
        sequence: 7,
        dropped: 2,
        position: 1.5,
        playing: true,
        recording: false,
        beat: { bpm: 120, phase: 0.25, bar: 3, beat: 1 },
        meters: [0.5, 0.25, 0.75, 0.1],
        spectrum: [1, 2, 3],
        waveform: [-1, 1],
      }),
    );
    expect(String.fromCharCode(...b.subarray(0, 4))).toBe("CNTF");
    expect(b[4]).toBe(1); // version
    expect(b[6]).toBe(0b101); // playing | beat
    expect(b[8]).toBe(7); // sequence
    expect(b[12]).toBe(2); // dropped
    expect(b[24]).toBe(2); // meter count
    expect(b[26]).toBe(3); // spectrum bins
    expect(b[28]).toBe(2); // waveform samples
    expect(b.length).toBe(48 + 4 * 9);
  });

  it("rejects buffers that are not frames", () => {
    expect(() => decodeFrame(new ArrayBuffer(10))).toThrow(/shorter/);
    const junk = encodeFrame({
      sequence: 0,
      dropped: 0,
      position: 0,
      playing: false,
      recording: false,
      beat: null,
      meters: [],
    });
    new Uint8Array(junk)[0] = 0x58;
    expect(() => decodeFrame(junk)).toThrow(/not a frame/);
    const short = encodeFrame({
      sequence: 0,
      dropped: 0,
      position: 0,
      playing: false,
      recording: false,
      beat: null,
      meters: [1, 2, 3, 4],
    }).slice(0, HEADER_LEN + 4);
    expect(() => decodeFrame(short)).toThrow(/short/);
  });
});
