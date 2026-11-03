import type { PaletteChoice, PaletteId } from '../../Renderer';

type Stop = [number, string];

export interface Palette {
  id: string;
  name: string;
  bg: string;
  /** Main trace / foreground color. */
  fg: string;
  /** Faint color for graticules and unlit segments. */
  dim: string;
  /** 256 entries, dark to hot. Packed for ImageData (little-endian ABGR). */
  heat: Uint32Array;
  /** 256 entries that wrap smoothly, for palette cycling. */
  cycle: Uint32Array;
  /** 256 entries low to high, for level meters. */
  meter: Uint32Array;
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Linear gradient through stops into a 256-entry packed LUT. */
export function buildLut(stops: Stop[]): Uint32Array {
  const lut = new Uint32Array(256);
  const rgb = stops.map(([p, c]) => [p, ...hexToRgb(c)] as const);
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let j = 0;
    while (j < rgb.length - 2 && t > rgb[j + 1][0]) j++;
    const [p0, r0, g0, b0] = rgb[j];
    const [p1, r1, g1, b1] = rgb[j + 1];
    const f = p1 === p0 ? 0 : Math.min(1, Math.max(0, (t - p0) / (p1 - p0)));
    const r = Math.round(r0 + (r1 - r0) * f);
    const g = Math.round(g0 + (g1 - g0) * f);
    const b = Math.round(b0 + (b1 - b0) * f);
    lut[i] = (255 << 24) | (b << 16) | (g << 8) | r;
  }
  return lut;
}

export function lutCss(lut: Uint32Array, i: number): string {
  const v = lut[Math.max(0, Math.min(255, Math.round(i)))];
  return `rgb(${v & 255},${(v >> 8) & 255},${(v >> 16) & 255})`;
}

export const PALETTES: Record<PaletteId, Palette> = {
  green: {
    id: 'green',
    name: 'Green phosphor',
    bg: '#010802',
    fg: '#5cff7a',
    dim: '#0c2a12',
    heat: buildLut([[0, '#000000'], [0.35, '#06380f'], [0.7, '#2bff4f'], [1, '#e8ffe8']]),
    cycle: buildLut([[0, '#021405'], [0.25, '#1aa83a'], [0.5, '#c8ffc8'], [0.75, '#1aa83a'], [1, '#021405']]),
    meter: buildLut([[0, '#14c43a'], [0.55, '#9cf53a'], [0.8, '#f5e43a'], [1, '#ff3b2b']]),
  },
  amber: {
    id: 'amber',
    name: 'Amber',
    bg: '#0a0500',
    fg: '#ffb000',
    dim: '#2e1a00',
    heat: buildLut([[0, '#000000'], [0.35, '#4a2200'], [0.7, '#ffb000'], [1, '#fff4d6']]),
    cycle: buildLut([[0, '#1a0a00'], [0.25, '#b86b00'], [0.5, '#ffe7a0'], [0.75, '#b86b00'], [1, '#1a0a00']]),
    meter: buildLut([[0, '#a85a00'], [0.6, '#ffb000'], [1, '#fff0c0']]),
  },
  ice: {
    id: 'ice',
    name: 'Ice blue',
    bg: '#00050c',
    fg: '#7fd8ff',
    dim: '#0a2238',
    heat: buildLut([[0, '#000000'], [0.35, '#06244a'], [0.7, '#4cc3ff'], [1, '#f0ffff']]),
    cycle: buildLut([[0, '#020a1c'], [0.25, '#1b6dd1'], [0.5, '#e8fbff'], [0.75, '#1b6dd1'], [1, '#020a1c']]),
    meter: buildLut([[0, '#1b5fd1'], [0.6, '#4cc3ff'], [1, '#e8fbff']]),
  },
  vga: {
    id: 'vga',
    name: 'VGA rainbow',
    bg: '#000000',
    fg: '#ffff55',
    dim: '#1c1c3a',
    heat: buildLut([[0, '#000000'], [0.3, '#aa0000'], [0.6, '#ff5500'], [0.8, '#ffff55'], [1, '#ffffff']]),
    cycle: buildLut([
      [0, '#ff0000'], [0.17, '#ffff00'], [0.33, '#00ff00'], [0.5, '#00ffff'],
      [0.67, '#0000ff'], [0.83, '#ff00ff'], [1, '#ff0000'],
    ]),
    meter: buildLut([[0, '#00aa00'], [0.5, '#55ff55'], [0.75, '#ffff55'], [1, '#ff5555']]),
  },
};

export const PALETTE_IDS = Object.keys(PALETTES) as PaletteId[];

function makePalette(id: string, name: string, bg: string, fg: string, dim: string, heat: Stop[], cycle: Stop[], meter: Stop[]): Palette {
  return { id, name, bg, fg, dim, heat: buildLut(heat), cycle: buildLut(cycle), meter: buildLut(meter) };
}

/** Each scene's own colors, used when the palette setting is Scene colors. */
export const SCENE_PALETTES = {
  fire: makePalette('fire', 'Fire', '#000000', '#ffd060', '#2a0a00',
    [[0, '#000000'], [0.3, '#8a0a00'], [0.55, '#ff4a00'], [0.8, '#ffc020'], [1, '#ffffff']],
    [[0, '#300000'], [0.25, '#c02000'], [0.5, '#ffd060'], [0.75, '#c02000'], [1, '#300000']],
    [[0, '#a01000'], [0.6, '#ff6a00'], [1, '#ffe080']]),
  ember: makePalette('ember', 'Ember', '#030104', '#ffe6c0', '#2a1206',
    [[0, '#000000'], [0.3, '#5a1404'], [0.6, '#ff6a18'], [0.85, '#ffd8a0'], [1, '#ffffff']],
    [[0, '#1a0602'], [0.25, '#b03a08'], [0.5, '#ffd8a0'], [0.75, '#b03a08'], [1, '#1a0602']],
    [[0, '#7a2008'], [0.6, '#ff7a20'], [1, '#fff0d0']]),
  sunset: makePalette('sunset', 'Sunset', '#0a0214', '#ff4fd8', '#2a0a3a',
    [[0, '#0a0214'], [0.3, '#5a0a6a'], [0.6, '#ff2fa8'], [0.85, '#ffb040'], [1, '#fff6c0']],
    [[0, '#1a0430'], [0.25, '#c01890'], [0.5, '#ffd060'], [0.75, '#20c0ff'], [1, '#1a0430']],
    [[0, '#ff2fa8'], [0.6, '#ff8040'], [1, '#fff060']]),
  nebula: makePalette('nebula', 'Nebula', '#040210', '#e0c0ff', '#1a0c30',
    [[0, '#020108'], [0.3, '#2a0c5a'], [0.55, '#9a2aa8'], [0.8, '#ff7ab0'], [1, '#fff0e0']],
    [[0, '#0a0420'], [0.25, '#4030c0'], [0.5, '#e050b0'], [0.75, '#ff9a50'], [1, '#0a0420']],
    [[0, '#4030c0'], [0.6, '#e050b0'], [1, '#ffd0e8']]),
  planet: makePalette('planet', 'Planet', '#05060c', '#f0e0c0', '#202430',
    [[0, '#000000'], [0.3, '#3a2414'], [0.6, '#b08050'], [0.85, '#e8d0a8'], [1, '#fffaf0']],
    [[0, '#3a2414'], [0.25, '#b08050'], [0.5, '#f0e0c0'], [0.75, '#b08050'], [1, '#3a2414']],
    [[0, '#806040'], [0.6, '#d0b080'], [1, '#fff0d0']]),
  water: makePalette('water', 'Water', '#000814', '#9ae8ff', '#06223a',
    [[0, '#000000'], [0.3, '#062a5a'], [0.6, '#1a90c8'], [0.85, '#9ae8ff'], [1, '#ffffff']],
    [[0, '#02122a'], [0.25, '#0a5a9a'], [0.5, '#9ae8ff'], [0.75, '#20a090'], [1, '#02122a']],
    [[0, '#0a5a9a'], [0.6, '#30c0e0'], [1, '#e0fbff']]),
  tunnel: makePalette('tunnel', 'Tunnel', '#020a0c', '#8affe0', '#0a2a2a',
    [[0, '#000000'], [0.35, '#0a4a4a'], [0.7, '#30e0c0'], [1, '#f0fff0']],
    [[0, '#1aa8a0'], [0.25, '#50f0c8'], [0.45, '#c8e060'], [0.65, '#9050c8'], [0.85, '#3070c0'], [1, '#1aa8a0']],
    [[0, '#1aa8a0'], [0.6, '#c8e060'], [1, '#ffffff']]),
  rain: makePalette('rain', 'Rain', '#000600', '#c8ffc8', '#032a08',
    [[0, '#000000'], [0.35, '#04400c'], [0.7, '#20e048'], [0.92, '#a8ffb8'], [1, '#ffffff']],
    [[0, '#011a04'], [0.25, '#0a8020'], [0.5, '#a8ffb8'], [0.75, '#0a8020'], [1, '#011a04']],
    [[0, '#0a8020'], [0.6, '#20e048'], [1, '#d0ffd8']]),
};

/** The palette a scene draws with: the user's pick, or the scene's own colors for 'auto'. */
export function resolvePalette(choice: PaletteChoice, natural: Palette): Palette {
  return choice === 'auto' ? natural : (PALETTES[choice] ?? natural);
}
