import type { AudioFrame } from '../../../frame';
import { logBinRanges } from '../bars';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';
import { bandLevels, blend, noiseTexture, scale } from './shared';

const R_IN = 1.3;
const R_OUT = 2.3;
const RING_BANDS = 48;
const SQUASH = 0.28;
const TILT = -0.32;
const NOISE = 64;

/**
 * A shaded, banded sphere with dotted-density rings, drawn per pixel at
 * 320 x 200. Classic counterpart of Ringed planet: ring density follows the
 * spectrum, beats brighten the rings, bass lights the atmosphere.
 */
export class ShadedPlanet extends RetroScene {
  readonly id = 'classic-planet';
  readonly name = 'Ringed planet';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.planet;

  private image: ImageData | null = null;
  private px = new Uint32Array(0);
  private noise: Uint8Array = new Uint8Array(0);
  private ranges: [number, number][] = [];
  private levels = new Float32Array(RING_BANDS);
  private stars = new Uint8Array(0);
  private spin = 0;

  protected setup(): void {
    this.image = this.g.createImageData(this.W, this.H);
    this.px = new Uint32Array(this.image.data.buffer);
    this.noise = noiseTexture(NOISE, 3, 3);
    this.ranges = logBinRanges(RING_BANDS, 1024, this.rc.sampleRate, 40, 8000);
    this.stars = Uint8Array.from({ length: this.W * this.H }, () => (Math.random() < 0.006 ? 1 : 0));
  }

  protected step(frame: AudioFrame): void {
    const { W, H, px, noise } = this;
    this.spin += RETRO_STEP * 0.05;
    bandLevels(frame.freq, this.ranges, this.levels);

    const lut = this.palette.heat;
    const R = H * 0.29;
    const cx = W * 0.54;
    const cy = H * 0.5;
    const cosT = Math.cos(TILT);
    const sinT = Math.sin(TILT);
    // Sun from the upper right, slightly toward the viewer.
    const lx = 0.62;
    const ly = -0.42;
    const lz = 0.66;
    const ringBoost = 0.75 + (this.flashAllowed ? frame.beat * 0.6 : frame.beat * 0.15);
    const rim = 0.25 + frame.bass * 0.9;
    const bg = 0xff000000 | 0x0a0605;

    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const dx = x - cx;
        const dy = y - cy;
        // Into the planet's tilted frame
        const tx = (dx * cosT + dy * sinT) / R;
        const ty = (-dx * sinT + dy * cosT) / R;

        // Ring plane: a squashed ellipse in the tilted frame
        const ry = ty / SQUASH;
        const rr = Math.sqrt(tx * tx + ry * ry);
        let ringC = 0;
        let ringA = 0;
        if (rr > R_IN && rr < R_OUT) {
          const u = (rr - R_IN) / (R_OUT - R_IN);
          const bandIdx = Math.min(RING_BANDS - 1, (u * RING_BANDS) | 0);
          const grain = noise[(((u * 60) | 0) & (NOISE - 1)) * NOISE] / 255;
          const gap = u > 0.55 && u < 0.62 ? 0.25 : 1;
          ringA = Math.min(0.95, (0.25 + this.levels[bandIdx] * 1.1) * (0.5 + grain) * gap);
          ringC = scale(lut[(120 + grain * 110) | 0], ringBoost);
        }

        let c = this.stars[y * W + x] ? 0xffc8c8c8 : bg;
        const d2 = tx * tx + ty * ty;
        const onPlanet = d2 < 1;
        if (onPlanet) {
          const nz = Math.sqrt(1 - d2);
          const lat = ty;
          const lon = Math.atan2(tx, nz) / Math.PI + this.spin;
          const n = noise[(((lat * 24 + 64) | 0) & (NOISE - 1)) * NOISE + (((lon * 40) | 0) & (NOISE - 1))] / 255;
          const bands = 0.5 + 0.5 * Math.sin(lat * 13 + n * 4);
          // Lighting uses screen-space normals.
          const sx = dx / R;
          const sy = dy / R;
          const lambert = Math.max(0, sx * lx + sy * ly + nz * lz);
          c = scale(lut[(90 + bands * 140) | 0], 0.08 + lambert);
          c = blend(c, lut[200], Math.pow(1 - nz, 3) * rim * 0.6);
        }
        // Ring in front (lower half of the ellipse) covers the planet; behind it, the planet covers the ring.
        if (ringA > 0 && (!onPlanet || ry > 0)) c = blend(c, ringC, ringA);
        px[y * W + x] = c;
      }
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
