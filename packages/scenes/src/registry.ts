import type { Family, OptionSchema, Renderer } from './Renderer';

export interface SceneEntry {
  id: string;
  name: string;
  family: Family;
  /** Options known up front, so settings can show them before the scene loads. */
  options?: OptionSchema;
  /** Creates a fresh instance; heavy code is split out with dynamic import(). */
  load: () => Promise<Renderer>;
  /** GPU scenes: a classic-era Canvas 2D version used for the Retro look. */
  retro?: () => Promise<Renderer>;
  /** GPU scenes without a counterpart: keep their own colors (16-bit) in the Retro look. */
  retroNative?: boolean;
  /** Left out of lists, Next and Random; reachable by id only. */
  hidden?: boolean;
  /** A 160 by 90 still, drawn by the scene harness from recorded frames. */
  thumbnail: string;
}

/** The harness writes one per scene: `node scripts/scene-harness.mjs --write`. */
function thumbnail(id: string): string {
  return new URL(`../thumbnails/${id}.png`, import.meta.url).href;
}

function shader(id: string, name: string, retro: () => Promise<Renderer>): Omit<SceneEntry, 'thumbnail'> {
  return { id, name, family: 'gpu', load: async () => (await import('./renderers/shader')).createShaderScene(id), retro };
}

const ENTRIES: Omit<SceneEntry, 'thumbnail'>[] = [
  shader('tunnel', 'Warp tunnel', async () => new (await import('./renderers/retro/classic/tunnel')).ClassicTunnel()),
  shader('grid', 'Spectrum grid', async () => new (await import('./renderers/retro/classic/landscape')).WireLandscape()),
  shader('kaleido', 'Kaleidoscope bloom', async () => new (await import('./renderers/retro/classic/mirror')).MirrorFeedback()),
  shader('blobs', 'Liquid blobs', async () => new (await import('./renderers/retro/classic/metaballs')).Metaballs()),
  shader('nebula', 'Nebula drift', async () => new (await import('./renderers/retro/classic/clouds')).CloudDrift()),
  shader('blackhole', 'Black hole', async () => new (await import('./renderers/retro/classic/disk')).ParticleDisk()),
  shader('planet', 'Ringed planet', async () => new (await import('./renderers/retro/classic/sphere')).ShadedPlanet()),
  shader('hyperspace', 'Hyperspace', async () => new (await import('./renderers/retro/starfield')).StarfieldScene({ warp: true })),
  shader('rain', 'Digital rain', async () => new (await import('./renderers/retro/classic/glyphs')).GlyphRain()),
  {
    id: 'bars',
    name: 'Spectrum bars',
    family: 'retro',
    options: {
      style: {
        label: 'Coloring',
        default: 'normal',
        choices: [
          { value: 'normal', label: 'Normal' },
          { value: 'fire', label: 'Fire' },
          { value: 'line', label: 'Line' },
        ],
      },
    },
    load: async () => new (await import('./renderers/retro/bars')).BarsScene(),
  },
  {
    id: 'scope',
    name: 'Oscilloscope',
    family: 'retro',
    options: {
      mode: {
        label: 'Trace',
        default: 'line',
        choices: [
          { value: 'line', label: 'Line' },
          { value: 'dots', label: 'Dots' },
          { value: 'solid', label: 'Solid' },
        ],
      },
    },
    load: async () => new (await import('./renderers/retro/scope')).ScopeScene(),
  },
  { id: 'fire', name: 'Fire', family: 'retro', load: async () => new (await import('./renderers/retro/fire')).FireScene() },
  { id: 'starfield', name: 'Starfield', family: 'retro', load: async () => new (await import('./renderers/retro/starfield')).StarfieldScene() },
  { id: 'plasma', name: 'Palette plasma', family: 'retro', load: async () => new (await import('./renderers/retro/plasma')).PlasmaScene() },
  { id: 'lissajous', name: 'Lissajous XY', family: 'retro', load: async () => new (await import('./renderers/retro/lissajous')).LissajousScene() },
  { id: 'trails', name: 'Scope trails', family: 'retro', load: async () => new (await import('./renderers/retro/trails')).TrailsScene() },
  { id: 'ring', name: 'Ring scope', family: 'retro', load: async () => new (await import('./renderers/retro/ring')).RingScene() },
  { id: 'fountain', name: 'Dot fountain', family: 'retro', load: async () => new (await import('./renderers/retro/fountain')).FountainScene() },
  { id: 'plane', name: 'Dot plane', family: 'retro', load: async () => new (await import('./renderers/retro/plane')).PlaneScene() },
  { id: 'flames', name: 'Flame fountain', family: 'retro', load: async () => new (await import('./renderers/retro/flames')).FlameFountainScene() },
  { id: 'ripple', name: 'Ripple', family: 'retro', load: async () => new (await import('./renderers/retro/ripple')).RippleScene() },
  { id: 'intro', name: 'Intro', family: 'retro', hidden: true, load: async () => new (await import('./renderers/retro/intro')).IntroScene() },
  { id: 'spectrogram', name: 'Spectrogram', family: 'retro', load: async () => new (await import('./renderers/retro/spectrogram')).SpectrogramScene() },
];

export const SCENES: SceneEntry[] = ENTRIES.map((e) => ({ ...e, thumbnail: thumbnail(e.id) }));

export const DEFAULT_SCENE: Record<Family, string> = { gpu: 'tunnel', retro: 'bars' };

export function getScene(id: string): SceneEntry | undefined {
  return SCENES.find((s) => s.id === id);
}

export function scenesFor(family: Family): SceneEntry[] {
  return SCENES.filter((s) => s.family === family);
}
