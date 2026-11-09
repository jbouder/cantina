import { describe, expect, it } from "vitest";
import type { StockGroove } from "./engine";
import { filterGrooves, meterOf } from "./grooves";

const groove = (
  name: string,
  genre: string,
  section: StockGroove["section"],
  n = 4,
): StockGroove => ({
  name,
  path: `/p/${name}.pattern.json`,
  stepsPerBeat: 4,
  length: 16,
  genre,
  section,
  timeSignature: { numerator: n, denominator: 4 },
  tempo: { min: 90, max: 140 },
  feel: "straight",
});

const all = [
  groove("Rock verse", "Rock", "verse"),
  groove("Rock fill", "Rock", "fill"),
  groove("Jazz waltz", "Jazz", "bridge", 3),
  groove("Funk verse", "Funk", "verse"),
];

describe("the groove filters", () => {
  it("filter by genre, section and time signature together", () => {
    expect(filterGrooves(all, "Rock", "", "", "").map((g) => g.name)).toEqual([
      "Rock verse",
      "Rock fill",
    ]);
    expect(filterGrooves(all, "", "verse", "", "").map((g) => g.name)).toEqual([
      "Rock verse",
      "Funk verse",
    ]);
    expect(filterGrooves(all, "", "", "3/4", "").map((g) => g.name)).toEqual(["Jazz waltz"]);
    expect(filterGrooves(all, "Rock", "verse", "4/4", "")).toHaveLength(1);
  });

  it("search every word across name, genre, section, feel and meter", () => {
    expect(filterGrooves(all, "", "", "", "verse funk").map((g) => g.name)).toEqual(["Funk verse"]);
    expect(filterGrooves(all, "", "", "", "3/4").map((g) => g.name)).toEqual(["Jazz waltz"]);
    expect(filterGrooves(all, "", "", "", "  STRAIGHT ")).toHaveLength(4);
    expect(filterGrooves(all, "", "", "", "polka")).toHaveLength(0);
    expect(meterOf(all[2] as StockGroove)).toBe("3/4");
  });
});
