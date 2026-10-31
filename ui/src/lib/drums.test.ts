import { describe, expect, it } from "vitest";
import {
  currentStep,
  draggedVelocity,
  isBeatStart,
  lengthLabel,
  PPQ,
  patternTicks,
  stepTicks,
  ticksAt,
  velocityAt,
  velocityLabel,
} from "./drums";
import type { PatternInfo } from "./engine";

const tempo = { bpm: 120, numerator: 4, denominator: 4 };
const sixteenths: PatternInfo = {
  id: 1,
  name: "p",
  stepsPerBeat: 4,
  length: 16,
  swing: 0,
  rows: [{ pad: "kick", steps: [1, 0, 0, 0, 0.5, ...new Array(11).fill(0)] }],
};

describe("ticks", () => {
  it("derive from the resolution and the tempo", () => {
    expect(stepTicks(sixteenths)).toBe(240);
    expect(patternTicks(sixteenths)).toBe(16 * 240);
    expect(stepTicks({ stepsPerBeat: 3 })).toBe(320);
    expect(ticksAt(0.5, tempo)).toBe(PPQ);
    expect(ticksAt(2, tempo)).toBe(4 * PPQ);
  });
});

describe("currentStep", () => {
  it("follows a looping pattern and is null when stopped", () => {
    expect(currentStep(0, tempo, sixteenths, true)).toBe(0);
    expect(currentStep(0.125, tempo, sixteenths, true)).toBe(1);
    expect(currentStep(0.5, tempo, sixteenths, true)).toBe(4);
    expect(currentStep(1.99, tempo, sixteenths, true)).toBe(15);
    expect(currentStep(2.0, tempo, sixteenths, true)).toBe(0);
    expect(currentStep(2.5, tempo, sixteenths, true)).toBe(4);
    expect(currentStep(1, tempo, sixteenths, false)).toBeNull();
    expect(currentStep(-1, tempo, sixteenths, true)).toBe(0);
  });
  it("respects a shorter pattern", () => {
    const short = { stepsPerBeat: 4, length: 6 };
    expect(currentStep(0.75, tempo, short, true)).toBe(0);
    expect(currentStep(0.7, tempo, short, true)).toBe(5);
  });
});

describe("velocity", () => {
  it("rises when dragged up and never clears", () => {
    expect(draggedVelocity(0.5, -40)).toBe(1);
    expect(draggedVelocity(0.5, 20)).toBe(0.25);
    expect(draggedVelocity(0.5, 400)).toBe(0.05);
    expect(draggedVelocity(1, 0)).toBe(1);
    expect(velocityLabel(0.754)).toBe("75%");
    expect(velocityAt(sixteenths, "kick", 4)).toBe(0.5);
    expect(velocityAt(sixteenths, "snare", 4)).toBe(0);
  });
});

describe("labels", () => {
  it("say bars, beats or steps", () => {
    expect(lengthLabel(sixteenths, tempo)).toBe("1 bar");
    expect(lengthLabel({ stepsPerBeat: 4, length: 32 }, tempo)).toBe("2 bars");
    expect(lengthLabel({ stepsPerBeat: 4, length: 12 }, tempo)).toBe("3 beats");
    expect(lengthLabel({ stepsPerBeat: 4, length: 4 }, tempo)).toBe("1 beat");
    expect(lengthLabel({ stepsPerBeat: 4, length: 7 }, tempo)).toBe("7 steps");
    expect(lengthLabel({ stepsPerBeat: 2, length: 6 }, { numerator: 3 })).toBe("1 bar");
    expect(isBeatStart(0, sixteenths)).toBe(true);
    expect(isBeatStart(3, sixteenths)).toBe(false);
    expect(isBeatStart(4, sixteenths)).toBe(true);
  });
});
