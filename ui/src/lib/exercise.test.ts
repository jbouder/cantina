import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { type Exercise, exerciseTab, ladder, loopPlace, loopTicks, stepTempo } from "./exercise";

const STOCK = fileURLToPath(new URL("../../../exercises/", import.meta.url));

function stock(name: string): Exercise {
  return JSON.parse(readFileSync(`${STOCK}${name}.exercise.json`, "utf8"));
}

describe("an exercise's tab", () => {
  it("strikes a strummed chord on every string it plays, ringing to the next stroke", () => {
    // D-D-D-DU in eighths: strokes on eighths 0, 2, 4, 6 and 7 of each bar.
    const e = stock("e-to-a");
    const tab = exerciseTab(e);
    const lowE = tab.notes.filter((n) => n.string === 6).map((n) => n.start);
    expect(lowE).toEqual([0, 960, 1920, 2880, 3360, 3840, 4800, 5760, 6720, 7200]);
    const first = tab.notes.find((n) => n.string === 6);
    expect(first?.length).toBe(960);
    // The A leaves the low E out: five strings a stroke.
    const a = tab.notes.filter((n) => n.start === 7680);
    expect(a.map((n) => n.string).sort()).toEqual([1, 2, 3, 4, 5]);
    expect(tab.notes.find((n) => n.start === 3360)?.velocity).toBe(0.8);
  });

  it("keeps a phrase of notes as it is, at the start tempo", () => {
    const e = stock("pentatonic-box-1");
    const tab = exerciseTab(e);
    expect(tab.notes).toEqual(e.phrase.kind === "notes" ? e.phrase.notes : []);
    expect(tab.tuning).toEqual([40, 45, 50, 55, 59, 64]);
    expect(tab.tempo.bpm).toBe(e.tempo.start);
    expect(loopTicks(e)).toBe(3 * 3840);
  });

  it("reads every stock exercise", () => {
    for (const file of readdirSync(STOCK)) {
      const e: Exercise = JSON.parse(readFileSync(`${STOCK}${file}`, "utf8"));
      const tab = exerciseTab(e);
      expect(tab.notes.length, file).toBeGreaterThan(0);
      expect(Math.max(...tab.notes.map((n) => n.start + n.length)), file).toBeLessThanOrEqual(
        loopTicks(e),
      );
    }
  });
});

describe("the tempo ladder", () => {
  const e = stock("open-e");

  it("climbs from the start to the goal by the step", () => {
    expect(ladder(e)).toEqual([60, 65, 70, 75, 80, 85, 90, 95, 100]);
    expect(ladder({ ...e, tempo: { start: 70, goal: 110, step: 15 } })).toEqual([70, 85, 100, 110]);
  });

  it("steps a rung at a time and stays on it", () => {
    expect(stepTempo(e, 60, 1)).toBe(65);
    expect(stepTempo(e, 62, 1)).toBe(65);
    expect(stepTempo(e, 62, -1)).toBe(60);
    expect(stepTempo(e, 100, 1)).toBe(100);
    expect(stepTempo(e, 60, -1)).toBe(60);
  });
});

describe("the loop", () => {
  it("counts passes from 1 and wraps the tick", () => {
    expect(loopPlace(0, 3840)).toEqual({ pass: 1, tick: 0 });
    expect(loopPlace(3840 * 2 + 10, 3840)).toEqual({ pass: 3, tick: 10 });
  });
});
