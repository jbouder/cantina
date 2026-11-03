import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { SKINS } from "../index";

const TOKENS: Record<string, string> = {
  frame: "frame",
  title: "title-on",
  lcd: "lcd-bg",
  lcdInk: "lcd-fg",
  key: "silver",
  led: "led-on",
};

function tokens(id: string): Record<string, string> {
  const css = readFileSync(new URL(`../tokens/${id}.css`, import.meta.url), "utf8");
  return Object.fromEntries([...css.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2]]));
}

describe("skins", () => {
  const classic = tokens("classic");

  it("has a token file for every skin, and tokens.css imports each", () => {
    const index = readFileSync(new URL("../tokens.css", import.meta.url), "utf8");
    for (const s of SKINS) {
      expect(index).toContain(`./tokens/${s.id}.css`);
      expect(readFileSync(new URL(`../tokens/${s.id}.css`, import.meta.url), "utf8")).toContain(
        s.id === "classic" ? ":root," : `:root[data-skin="${s.id}"]`,
      );
    }
  });

  it("previews each skin in its own colors, falling back to Classic's", () => {
    for (const s of SKINS) {
      const t = { ...classic, ...tokens(s.id) };
      for (const [key, token] of Object.entries(TOKENS)) {
        expect(s.preview[key as keyof typeof s.preview], `${s.id} ${key}`).toBe(t[token]);
      }
    }
  });

  it("defines every shared token in Classic, the fallback", () => {
    for (const s of SKINS) {
      for (const name of Object.keys(tokens(s.id))) expect(classic, `${s.id} --${name}`).toHaveProperty(name);
    }
  });
});
