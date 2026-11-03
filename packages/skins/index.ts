/*
 * The six player skins Cantina and music-visualizer share. Each has a
 * palette in tokens/<id>.css, imported all together from tokens.css; an
 * app's own component-level tokens go on top in its own stylesheets.
 */

/** A few of a skin's colors, for drawing a swatch of it in a picker. */
export interface SkinPreview {
  frame: string;
  title: string;
  lcd: string;
  lcdInk: string;
  key: string;
  led: string;
}

export interface SkinInfo {
  id: string;
  name: string;
  /** The skin's CSS reads `--beat`, so an app animates it while it is on. */
  beat: boolean;
  /** Read from the skin's tokens: --frame, --title-on, --lcd-bg, --lcd-fg, --silver, --led-on. */
  preview: SkinPreview;
}

export const SKINS = [
  {
    id: "classic",
    name: "Classic",
    beat: false,
    preview: {
      frame: "#3a3a4e",
      title: "#c9b25e",
      lcd: "#000",
      lcdInk: "#00e600",
      key: "#c4c4ce",
      led: "#3cff3c",
    },
  },
  {
    id: "midnight",
    name: "Midnight",
    beat: false,
    preview: {
      frame: "#1f2a4d",
      title: "#6fd8ff",
      lcd: "#020610",
      lcdInk: "#4fe0ff",
      key: "#9fb3e0",
      led: "#4fe0ff",
    },
  },
  {
    id: "metal",
    name: "Brushed metal",
    beat: false,
    preview: {
      frame: "#c9cdd3",
      title: "#7d838b",
      lcd: "#050b14",
      lcdInk: "#6cb6ff",
      key: "#dfe2e6",
      led: "#4aa8ff",
    },
  },
  {
    id: "hifi",
    name: "Hi-fi",
    beat: false,
    preview: {
      frame: "#6b3f22",
      title: "#e6c27a",
      lcd: "#010d0c",
      lcdInk: "#4ff0d0",
      key: "#d8cfbd",
      led: "#ff9a3c",
    },
  },
  {
    id: "neon",
    name: "Neon",
    beat: true,
    preview: {
      frame: "#0c0614",
      title: "#ff4fd8",
      lcd: "#04020a",
      lcdInk: "#3cf0ff",
      key: "#0c0614",
      led: "#ff4fd8",
    },
  },
  {
    id: "toxic",
    name: "Toxic",
    beat: false,
    preview: {
      frame: "#8fd400",
      title: "#1a2c00",
      lcd: "#0a0a00",
      lcdInk: "#fff23a",
      key: "#c8f040",
      led: "#fff23a",
    },
  },
] as const satisfies readonly SkinInfo[];

export type SkinId = (typeof SKINS)[number]["id"];

export const DEFAULT_SKIN: SkinId = "classic";

export function isSkinId(value: unknown): value is SkinId {
  return SKINS.some((s) => s.id === value);
}

/** True when the skin's CSS reads `--beat`. */
export function skinUsesBeat(skin: SkinId): boolean {
  return SKINS.find((s) => s.id === skin)?.beat ?? false;
}
