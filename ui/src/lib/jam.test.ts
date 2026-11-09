import { describe, expect, it } from "vitest";
import type { JamMark } from "./engine";
import { addTap, jamPlace, tapTempo } from "./jam";

const SR = 48_000;
const BAR = 3840;

describe("jamPlace", () => {
  // Verse at 120 (a bar is 2 s), chorus from bar 3, 60 bpm from bar 5.
  const marks: JamMark[] = [
    { sample: 0, tick: 0, section: "verse", bpm: 120 },
    { sample: 4 * SR, tick: 2 * BAR, section: "chorus", bpm: 120 },
    { sample: 8 * SR, tick: 4 * BAR, section: "chorus", bpm: 60 },
  ];

  it("counts bars and beats from the start", () => {
    expect(jamPlace(0, marks, SR, BAR, 4)).toMatchObject({ section: "verse", bar: 1, beat: 1 });
    expect(jamPlace(1.6, marks, SR, BAR, 4)).toMatchObject({ bar: 1, beat: 4 });
    expect(jamPlace(2.1, marks, SR, BAR, 4)).toMatchObject({ bar: 2, beat: 1 });
  });

  it("shows what is coming", () => {
    expect(jamPlace(1, marks, SR, BAR, 4)).toMatchObject({
      next: { section: "chorus", bar: 3 },
      nextTempo: { bpm: 60, bar: 5 },
    });
    expect(jamPlace(5, marks, SR, BAR, 4)).toMatchObject({
      section: "chorus",
      bar: 3,
      next: null,
      nextTempo: { bpm: 60, bar: 5 },
    });
  });

  it("follows a tempo change", () => {
    // Bar 5 starts at 8 s; at 60 bpm a beat is a second.
    expect(jamPlace(8, marks, SR, BAR, 4)).toMatchObject({ bar: 5, beat: 1, bpm: 60 });
    expect(jamPlace(11.5, marks, SR, BAR, 4)).toMatchObject({ bar: 5, beat: 4 });
    expect(jamPlace(12, marks, SR, BAR, 4)).toMatchObject({ bar: 6, beat: 1, nextTempo: null });
  });

  it("is nowhere with no marks", () => {
    expect(jamPlace(1, [], SR, BAR, 4)).toBeNull();
  });
});

describe("tap tempo", () => {
  it("averages the last taps", () => {
    let taps: number[] = [];
    for (const t of [0, 500, 1000, 1500]) taps = addTap(taps, t);
    expect(tapTempo(taps)).toBe(120);
    taps = addTap(taps, 2100);
    expect(tapTempo(taps)).toBeCloseTo(114.3, 1);
  });

  it("needs two taps and starts again after a long gap", () => {
    expect(tapTempo(addTap([], 0))).toBeNull();
    const taps = addTap([0, 400], 5000);
    expect(taps).toEqual([5000]);
    expect(tapTempo(taps)).toBeNull();
  });

  it("keeps the tempo to what a jam plays", () => {
    expect(tapTempo([0, 100])).toBe(300);
    expect(tapTempo([0, 1999])).toBe(30.0);
  });
});
