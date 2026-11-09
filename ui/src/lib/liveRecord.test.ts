import { describe, expect, it } from "vitest";
import type { PatternInfo, Tempo } from "./engine";
import { addHit, overdub, padForKey, placeOf, type Take } from "./liveRecord";

const tempo: Tempo = { bpm: 120, numerator: 4, denominator: 4 };
const beat: PatternInfo = {
  id: 1,
  name: "Beat",
  stepsPerBeat: 4,
  length: 16,
  swing: 0,
  rows: [{ pad: "kick", steps: [1, ...Array(15).fill(0)] }],
};

describe("live recording", () => {
  it("maps keys to pads in the kit's order", () => {
    expect(padForKey("a")).toBe(0);
    expect(padForKey("F")).toBe(3);
    expect(padForKey("z")).toBe(10);
    expect(padForKey("q")).toBeNull();
  });

  it("quantizes a moment to the nearest step and counts passes", () => {
    // At 120 bpm a sixteenth is 0.125 s and a bar 2 s.
    expect(placeOf(0, tempo, beat)).toEqual({ pass: 0, step: 0 });
    expect(placeOf(0.13, tempo, beat)).toEqual({ pass: 0, step: 1 });
    expect(placeOf(0.18, tempo, beat)).toEqual({ pass: 0, step: 1 });
    expect(placeOf(0.2, tempo, beat)).toEqual({ pass: 0, step: 2 });
    expect(placeOf(2.5, tempo, beat)).toEqual({ pass: 1, step: 4 });
    // Just before the bar line: the next pass's first step.
    expect(placeOf(1.98, tempo, beat)).toEqual({ pass: 1, step: 0 });
  });

  it("overdubs a take, keeping the louder hit and adding the rest", () => {
    let take: Take = new Map();
    take = addHit(take, "kick", 0, 0.5);
    take = addHit(take, "snare", 4, 0.8);
    take = addHit(take, "snare", 4, 0.6);
    expect(take.size).toBe(2);
    const p = overdub(beat, take) as PatternInfo;
    expect(p.rows.find((r) => r.pad === "kick")?.steps[0]).toBe(1);
    expect(p.rows.find((r) => r.pad === "snare")?.steps[4]).toBe(0.8);
    expect(overdub(beat, new Map())).toBeNull();
  });
});
