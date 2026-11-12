import { describe, expect, it } from "vitest";
import type { PatternInfo, Tempo } from "./engine";
import {
  addHit,
  addPartHit,
  heardTicks,
  landing,
  overdub,
  overdubPart,
  type PartTake,
  padForKey,
  placeOf,
  type Take,
} from "./liveRecord";

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

describe("playing a part in", () => {
  const p = { stepsPerBeat: 4, length: 16 };
  const STEP = 240;
  it("lands a hit on the nearest step of the pass it is in", () => {
    expect(landing(3 * STEP + 100, 0, p, [0])).toEqual({ pass: 0, variant: 0, step: 3 });
    expect(landing(3 * STEP + 130, 0, p, [0])).toEqual({ pass: 0, variant: 0, step: 4 });
    // Just before a pass ends: the next pass's first step.
    expect(landing(16 * STEP - 20, 0, p, [0])).toEqual({ pass: 1, variant: 0, step: 0 });
    // From where the section starts playing.
    expect(landing(10_000 + 2 * STEP, 10_000, p, [0]).step).toBe(2);
  });
  it("lands in the variation the pass plays", () => {
    const round = [0, 1, 0, 2];
    expect(landing(16 * STEP + STEP, 0, p, round)).toEqual({ pass: 1, variant: 1, step: 1 });
    expect(landing(3 * 16 * STEP, 0, p, round).variant).toBe(2);
    expect(landing(4 * 16 * STEP, 0, p, round).variant).toBe(0);
  });
  it("overdubs each variation it touched, louder winning, nothing removed", () => {
    const a: PatternInfo = {
      id: 1,
      name: "A",
      stepsPerBeat: 4,
      length: 4,
      swing: 0,
      rows: [{ pad: "kick", steps: [1, 0, 0.5, 0] }],
    };
    const b: PatternInfo = { ...a, name: "B", rows: [] };
    let take: PartTake = new Map();
    take = addPartHit(take, { pass: 0, variant: 0, step: 2 }, "kick", 0.8);
    take = addPartHit(take, { pass: 0, variant: 0, step: 2 }, "kick", 0.3);
    take = addPartHit(take, { pass: 1, variant: 1, step: 1 }, "snare", 0.6);
    const out = overdubPart([a, b], take);
    expect(out.get(0)?.rows[0]?.steps).toEqual([1, 0, 0.8, 0]);
    expect(out.get(1)?.rows.find((r) => r.pad === "snare")?.steps).toEqual([0, 0.6, 0, 0]);
    expect(overdubPart([a], new Map()).size).toBe(0);
  });
  it("takes back the output's delay", () => {
    const tempo = { bpm: 120, numerator: 4, denominator: 4 };
    // At 120 bpm a beat is half a second: 960 ticks.
    expect(heardTicks(1, 0.5, tempo)).toBe(960);
    expect(heardTicks(0.1, 0.5, tempo)).toBe(0);
  });
});
