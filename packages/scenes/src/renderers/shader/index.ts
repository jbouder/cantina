import { ShaderScene, type ShaderSceneConfig } from '../../gl/ShaderScene';
import blackhole from './blackhole.frag?raw';
import blobs from './blobs.frag?raw';
import grid from './grid.frag?raw';
import hyperspace from './hyperspace.frag?raw';
import kaleido from './kaleido.frag?raw';
import nebula from './nebula.frag?raw';
import planet from './planet.frag?raw';
import rain from './rain.frag?raw';
import tunnel from './tunnel.frag?raw';

const CONFIGS: Record<string, ShaderSceneConfig> = {
  tunnel: { id: 'tunnel', name: 'Warp tunnel', frag: tunnel, travelSpeed: (b) => 0.6 + b.bass * 3.5 },
  grid: { id: 'grid', name: 'Spectrum grid', frag: grid, travelSpeed: (b) => 2 + b.bass * 6 },
  kaleido: { id: 'kaleido', name: 'Kaleidoscope bloom', frag: kaleido, feedback: true },
  blobs: { id: 'blobs', name: 'Liquid blobs', frag: blobs },
  nebula: { id: 'nebula', name: 'Nebula drift', frag: nebula, travelSpeed: (b) => 0.25 + b.bass * 1.4 },
  blackhole: { id: 'blackhole', name: 'Black hole', frag: blackhole },
  planet: { id: 'planet', name: 'Ringed planet', frag: planet },
  hyperspace: { id: 'hyperspace', name: 'Hyperspace', frag: hyperspace, travelSpeed: (b) => 0.3 + b.bass * 2.5 },
  rain: { id: 'rain', name: 'Digital rain', frag: rain, travelSpeed: (b) => 0.35 + b.bass * 1.2 },
};

export function createShaderScene(id: string): ShaderScene {
  return new ShaderScene(CONFIGS[id]);
}
