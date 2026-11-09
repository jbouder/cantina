import { describe, expect, it } from "vitest";
import type { RigInfo } from "./engine";
import { chainSummary, filterRigs, rigGenres } from "./rigs";

function rig(name: string, genre: string, instrument: "guitar" | "bass", chain: string[] = []) {
  return {
    name,
    genre,
    description: `${name} tone`,
    instrument,
    stock: true,
    path: `/rigs/${name}.rig.json`,
    chain,
  } satisfies RigInfo;
}

const RIGS: RigInfo[] = [
  rig("Metalcore", "Metal", "guitar"),
  rig("Thrash", "Metal", "guitar"),
  rig("Blues", "Blues", "guitar"),
  rig("Bass Metal", "Metal", "bass"),
];

describe("rigs", () => {
  it("filters by instrument, genre and every word of a search", () => {
    expect(filterRigs(RIGS, "guitar", "", "").map((r) => r.name)).toEqual([
      "Metalcore",
      "Thrash",
      "Blues",
    ]);
    expect(filterRigs(RIGS, "guitar", "Metal", "").map((r) => r.name)).toEqual([
      "Metalcore",
      "Thrash",
    ]);
    expect(filterRigs(RIGS, "bass", "", "metal tone").map((r) => r.name)).toEqual(["Bass Metal"]);
    expect(filterRigs(RIGS, "guitar", "", "jazz")).toEqual([]);
  });

  it("lists an instrument's genres once each, sorted", () => {
    expect(rigGenres(RIGS, "guitar")).toEqual(["Blues", "Metal"]);
    expect(rigGenres(RIGS, "bass")).toEqual(["Metal"]);
  });

  it("sums up a chain in a few words", () => {
    expect(
      chainSummary(
        rig("X", "Rock", "guitar", ["gate", "cab:4x12-modern", "pedal:warm-drive", "au:x"]),
      ),
    ).toBe("gate → 4x12 → warm-drive → plugin");
  });
});
