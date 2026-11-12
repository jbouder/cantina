import { describe, expect, it } from "vitest";
import {
  beatDiagram,
  chordDiagram,
  chordName,
  diagramOf,
  fretboardDots,
  neckFrets,
} from "./diagram";
import type { Tab } from "./engine";

const STANDARD = [40, 45, 50, 55, 59, 64];

function tab(notes: Tab["notes"], extra: Partial<Tab> = {}): Tab {
  return {
    name: "T",
    instrument: "guitar",
    tuning: STANDARD,
    tempo: { bpm: 120, numerator: 4, denominator: 4 },
    notes,
    ...extra,
  };
}

describe("a chord shape's diagram", () => {
  it("draws an open chord from the nut, with open and muted strings", () => {
    const d = chordDiagram({
      name: "C",
      frets: [null, 3, 2, 0, 1, 0],
      fingers: [0, 3, 2, 0, 1, 0],
      start: 0,
      length: 960,
    });
    expect(d.base).toBe(1);
    expect(d.span).toBe(4);
    expect(d.frets).toEqual([null, 3, 2, 0, 1, 0]);
    expect(d.barres).toEqual([]);
  });

  it("starts a shape up the neck at its lowest fret and finds the barre", () => {
    const d = diagramOf("A", [5, 7, 7, 6, 5, 5], [1, 3, 4, 2, 1, 1], 0);
    expect(d.base).toBe(5);
    expect(d.span).toBe(4);
    expect(d.barres).toEqual([{ fret: 5, from: 0, to: 5, finger: 1 }]);
  });

  it("keeps the capo and a stretch wider than four frets", () => {
    const d = diagramOf("Wide", [3, null, 8, null, null, null], [], 2);
    expect(d.capo).toBe(2);
    expect(d.base).toBe(3);
    expect(d.span).toBe(6);
  });
});

describe("naming the notes of a beat", () => {
  it("names triads, sevenths, power chords and slash chords", () => {
    expect(chordName([40, 47, 52, 56, 59, 64])).toBe("E");
    expect(chordName([45, 52, 57, 60, 64])).toBe("Am");
    expect(chordName([40, 47])).toBe("E5");
    expect(chordName([52, 56, 62, 66, 71])).toBe("E9");
    expect(chordName([47, 55, 62, 67])).toBe("G/B");
    expect(chordName([40, 41])).toBe("");
    expect(chordName([40])).toBe("");
  });
});

describe("the chord at a beat of a tab", () => {
  it("takes the notes that start together at or before the tick", () => {
    const t = tab([
      { string: 6, fret: 3, start: 0, length: 480 },
      { string: 5, fret: 5, start: 0, length: 480 },
      { string: 6, fret: 0, start: 480, length: 480 },
    ]);
    const g5 = beatDiagram(t, 100);
    expect(g5?.name).toBe("G5");
    expect(g5?.frets).toEqual([3, 5, null, null, null, null]);
    expect(beatDiagram(t, 600)).toBeNull();
  });

  it("follows a drop tuning and a capo", () => {
    const drop = tab(
      [
        { string: 6, fret: 0, start: 0, length: 480 },
        { string: 5, fret: 0, start: 0, length: 480 },
        { string: 4, fret: 0, start: 0, length: 480 },
      ],
      { tuning: [38, 45, 50, 55, 59, 64], capo: 2 },
    );
    expect(beatDiagram(drop, 0)?.name).toBe("E5");
    expect(beatDiagram(drop, 0)?.capo).toBe(2);
  });
});

describe("the fretboard", () => {
  const run = tab([
    { string: 6, fret: 5, start: 0, length: 480 },
    { string: 6, fret: 8, start: 480, length: 480 },
    { string: 5, fret: 5, start: 960, length: 480 },
    { string: 5, fret: 7, start: 1440, length: 480, dead: true },
  ]);

  it("lights the beat sounding, the one coming and the rest of the shape", () => {
    expect(fretboardDots(run, 500)).toEqual([
      { string: 5, fret: 5, kind: "next" },
      { string: 6, fret: 5, kind: "shape" },
      { string: 6, fret: 8, kind: "now" },
    ]);
  });

  it("shows where to start when stopped, and counts frets from the nut", () => {
    const capoed = { ...run, capo: 3 };
    expect(fretboardDots(capoed, null)).toContainEqual({ string: 6, fret: 8, kind: "next" });
    expect(neckFrets(run)).toBe(12);
    expect(neckFrets(tab([{ string: 1, fret: 20, start: 0, length: 1 }], { capo: 3 }))).toBe(24);
  });
});
