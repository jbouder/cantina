import { describe, expect, it } from "vitest";
import type { PedalInfo, TrackStatus } from "./engine";
import { noteFor, readoutsByPedal } from "./readouts";

function pedal(id: number, readouts: string[]): PedalInfo {
  return {
    id,
    spec: "x",
    name: "X",
    params: [],
    hasEditor: false,
    editorOpen: false,
    latencyFrames: 0,
    readouts,
  };
}

function track(chain: PedalInfo[]): TrackStatus {
  return {
    id: 1,
    name: "T",
    armed: false,
    recording: false,
    kit: null,
    chain,
    clips: [],
    patternClips: [],
    strip: { gainDb: 0, pan: 0, mute: false, solo: false },
    latencyFrames: 0,
  };
}

describe("readoutsByPedal", () => {
  it("hands each pedal its own values in chain order across tracks", () => {
    const tracks = [
      track([pedal(1, ["Pitch", "Clarity"]), pedal(2, [])]),
      track([pedal(3, ["Reduction"])]),
    ];
    const map = readoutsByPedal(tracks, [440, 0.9, -6]);
    expect(map.get(1)).toEqual([440, 0.9]);
    expect(map.has(2)).toBe(false);
    expect(map.get(3)).toEqual([-6]);
  });

  it("shows nothing when the frame does not line up with the status", () => {
    const tracks = [track([pedal(1, ["Pitch", "Clarity"])])];
    expect(readoutsByPedal(tracks, [440]).size).toBe(0);
    expect(readoutsByPedal(tracks, [440, 0.9, 1]).size).toBe(0);
  });
});

describe("noteFor", () => {
  it("names notes against the reference", () => {
    expect(noteFor(440)).toEqual({ name: "A", octave: 4, cents: 0 });
    expect(noteFor(261.63)?.name).toBe("C");
    expect(noteFor(261.63)?.octave).toBe(4);
    expect(noteFor(41.2)?.name).toBe("E");
    expect(noteFor(41.2)?.octave).toBe(1);
    expect(noteFor(466.16)?.name).toBe("A♯");
  });

  it("reports cents and follows the reference", () => {
    const sharp = noteFor(442);
    expect(sharp?.name).toBe("A");
    expect(sharp?.cents).toBeCloseTo(7.85, 1);
    expect(noteFor(442, 442)?.cents).toBeCloseTo(0, 5);
    expect(noteFor(427.5)?.cents).toBeCloseTo(-49.8, 0);
    expect(noteFor(0)).toBeNull();
  });
});
