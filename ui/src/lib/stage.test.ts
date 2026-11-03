import { describe, expect, it } from "vitest";
import { currentScene, LISTED_SCENES, setScene, stepScene } from "./stage";

describe("stage scene", () => {
  it("steps through the listed scenes and wraps at both ends", () => {
    const first = LISTED_SCENES[0]?.id ?? "";
    const last = LISTED_SCENES[LISTED_SCENES.length - 1]?.id ?? "";
    setScene(first);
    stepScene(-1);
    expect(currentScene()).toBe(last);
    stepScene(1);
    expect(currentScene()).toBe(first);
    stepScene(1);
    expect(currentScene()).toBe(LISTED_SCENES[1]?.id);
  });

  it("lists every scene but the hidden intro and ignores unknown ids", () => {
    expect(LISTED_SCENES).toHaveLength(22);
    expect(LISTED_SCENES.some((s) => s.id === "intro")).toBe(false);
    const before = currentScene();
    setScene("no-such-scene");
    expect(currentScene()).toBe(before);
  });
});
