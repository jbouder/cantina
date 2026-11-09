import { describe, expect, it } from "vitest";
import type { ClipInfo, Tab, TabNote } from "./engine";
import {
  barTicks,
  bendLabel,
  cursorAt,
  fretLabel,
  joinLabel,
  layoutTab,
  playedTicks,
  rhythm,
  sampleAt,
  stringNames,
  tickAt,
  tickAtX,
} from "./tabLayout";

const note = (
  string: number,
  fret: number,
  start: number,
  length: number,
  extra = {},
): TabNote => ({
  string,
  fret,
  start,
  length,
  ...extra,
});

const tab = (notes: TabNote[], extra: Partial<Tab> = {}): Tab => ({
  name: "Riff",
  instrument: "guitar",
  tuning: [40, 45, 50, 55, 59, 64],
  tempo: { bpm: 120, numerator: 4, denominator: 4 },
  notes,
  ...extra,
});

const clip: ClipInfo = {
  id: 1,
  name: "Take",
  file: "take.wav",
  start: 96000,
  offset: 48000,
  length: 480000,
};

describe("tab layout", () => {
  it("counts bars in the time signature", () => {
    expect(barTicks({ bpm: 120, numerator: 4, denominator: 4 })).toBe(3840);
    expect(barTicks({ bpm: 120, numerator: 6, denominator: 8 })).toBe(2880);
  });

  it("writes frets, dead notes and harmonics", () => {
    expect(fretLabel(note(1, 3, 0, 1))).toBe("3");
    expect(fretLabel(note(1, 3, 0, 1, { dead: true }))).toBe("x");
    expect(fretLabel(note(1, 12, 0, 1, { harmonic: true }))).toBe("<12>");
  });

  it("writes bends as tab does", () => {
    expect(bendLabel(1)).toBe("½");
    expect(bendLabel(2)).toBe("full");
    expect(bendLabel(3)).toBe("1½");
    expect(bendLabel(4)).toBe("2");
  });

  it("joins legato and slides", () => {
    expect(joinLabel(note(3, 5, 0, 1), note(3, 7, 1, 1, { hammerOn: true }))).toBe("h");
    expect(joinLabel(note(3, 7, 0, 1), note(3, 5, 1, 1, { pullOff: true }))).toBe("p");
    expect(joinLabel(note(3, 5, 0, 1, { slide: true }), note(3, 9, 1, 1))).toBe("/");
    expect(joinLabel(note(3, 9, 0, 1, { slide: true }), note(3, 5, 1, 1))).toBe("\\");
    expect(joinLabel(note(3, 5, 0, 1), undefined)).toBeNull();
  });

  it("reads rhythm values, dotted and triplet", () => {
    expect(rhythm(960)).toMatchObject({ flags: 0, stem: true, hollow: false });
    expect(rhythm(480)).toMatchObject({ flags: 1, dotted: false, triplet: false });
    expect(rhythm(240)).toMatchObject({ flags: 2 });
    expect(rhythm(1920)).toMatchObject({ hollow: true, stem: true });
    expect(rhythm(3840)).toMatchObject({ hollow: true, stem: false });
    expect(rhythm(720)).toMatchObject({ flags: 1, dotted: true });
    expect(rhythm(320)).toMatchObject({ flags: 1, triplet: true });
    expect(rhythm(160)).toMatchObject({ flags: 2, triplet: true });
  });

  it("names strings from the top, the high e in lower case", () => {
    expect(stringNames(tab([]))).toEqual(["e", "B", "G", "D", "A", "E"]);
    expect(stringNames(tab([], { tuning: [38, 45, 50, 55, 59, 64] }))).toEqual([
      "e",
      "B",
      "G",
      "D",
      "A",
      "D",
    ]);
    expect(stringNames(tab([], { instrument: "bass", tuning: [28, 33, 38, 43] }))).toEqual([
      "G",
      "D",
      "A",
      "E",
    ]);
    expect(stringNames(tab([], { capo: 2 }))[5]).toBe("F#");
  });

  it("places notes in their bars and rows", () => {
    const t = tab([note(6, 0, 0, 480), note(1, 3, 3840, 960), note(2, 5, 3840 * 4, 960)]);
    const strip = layoutTab(t, Number.POSITIVE_INFINITY, 200);
    expect(strip.bars).toHaveLength(5);
    expect(strip.rows).toBe(1);
    expect(strip.width).toBe(1000);
    const page = layoutTab(t, 2, 200);
    expect(page.rows).toBe(3);
    expect(page.bars.map((b) => b.row)).toEqual([0, 0, 1, 1, 2]);
    const [first, second] = page.notes;
    if (!first || !second) throw new Error("two notes placed");
    expect(first.line).toBe(5);
    expect(first.x).toBe(14);
    // Bar 2 starts a bar's width along.
    expect(second.x).toBe(214);
    expect(page.beats.map((b) => b.tick)).toEqual([0, 3840, 15360]);
  });

  it("joins only within a row", () => {
    const t = tab([note(3, 5, 0, 480, { slide: true }), note(3, 7, 480, 480)]);
    const placed = layoutTab(t, 4, 200).notes[0];
    if (!placed) throw new Error("a note placed");
    expect(placed.join).toBe("/");
    expect(placed.joinTo).toBeGreaterThan(placed.x);
  });

  it("covers the clip even past the last note", () => {
    expect(layoutTab(tab([note(1, 0, 0, 1)]), 4, 200, 3840 * 3).bars).toHaveLength(3);
  });

  it("puts the cursor and a click on the same ticks", () => {
    const t = tab([note(1, 0, 0, 3840 * 8)]);
    const layout = layoutTab(t, 4, 200);
    for (const tick of [0, 1000, 3840 * 5 + 960, 3840 * 7.5]) {
      const at = cursorAt(layout, t, tick);
      expect(tickAtX(layout, t, at.row, at.x)).toBeCloseTo(tick, 6);
    }
    expect(cursorAt(layout, t, 3840 * 5).row).toBe(1);
  });

  it("turns playhead seconds into ticks and back", () => {
    const t = tab([], { origin: 48000 });
    // Tick 0 is at timeline sample 96000 - 48000 + 48000 = 96000, 2 s.
    expect(tickAt(clip, t, 48000, 2)).toBeCloseTo(0, 6);
    // 120 bpm: a quarter is half a second.
    expect(tickAt(clip, t, 48000, 2.5)).toBeCloseTo(960, 6);
    expect(sampleAt(clip, t, 48000, 960)).toBe(120000);
    // The clip plays from tick 0 for ten seconds: twenty quarters.
    const [from, to] = playedTicks(clip, t, 48000);
    expect(from).toBeCloseTo(0, 6);
    expect(to).toBeCloseTo(960 * 20, 6);
  });
});
