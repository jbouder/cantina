import { describe, expect, it } from "vitest";
import type { KitFile, KitRecipe } from "./engine";
import { chokeGroups, recipeValues, withKitPad, withRecipePad } from "./kitEdit";

const kit: KitFile = {
  version: 2,
  name: "K",
  pads: [
    { id: "kick", name: "Kick", layers: [{ file: "kick.wav" }] },
    { id: "hh-closed", name: "Closed hat", choke: "hat", layers: [{ file: "hh.wav" }] },
  ],
};

describe("a kit's pads", () => {
  it("change one at a time, defaults left out", () => {
    const next = withKitPad(kit, "kick", { tune: -2, decay: 0.5, gainDb: 3 });
    expect(next.pads[0]).toMatchObject({ tune: -2, decay: 0.5, gainDb: 3 });
    expect(next.pads[1]).toBe(kit.pads[1]);
    const back = withKitPad(next, "kick", { tune: 0, decay: 1, gainDb: 0 });
    expect(back.pads[0]).toEqual(kit.pads[0]);
    expect(withKitPad(kit, "hh-closed", { choke: " " }).pads[1]?.choke).toBeUndefined();
  });
  it("list their choke groups once", () => {
    expect(chokeGroups(withKitPad(kit, "kick", { choke: "hat" }))).toEqual(["hat"]);
  });
});

describe("a recipe's pads", () => {
  const recipe: KitRecipe = {
    version: 1,
    name: "R",
    pads: [{ id: "kick", voice: "kick", pitch: 60 }],
  };
  it("sound as their own numbers, else their voice's", () => {
    expect(recipeValues(recipe.pads[0] as KitRecipe["pads"][number])).toEqual({
      pitch: 60,
      decay: 0.5,
      tone: 0.5,
      noise: 0.1,
    });
  });
  it("change one at a time", () => {
    expect(withRecipePad(recipe, "kick", { tone: 0.9 }).pads[0]).toMatchObject({
      pitch: 60,
      tone: 0.9,
    });
  });
});
