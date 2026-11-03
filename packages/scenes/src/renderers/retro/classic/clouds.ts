import type { AudioFrame } from '../../../frame';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';
import { blend, noiseTexture, scale } from './shared';

const TEX = 256;

/**
 * Two layers of tiled cloud noise drifting past each other, palette-cycled,
 * with twinkling stars. Classic counterpart of Nebula drift.
 */
export class CloudDrift extends RetroScene {
  readonly id = 'classic-clouds';
  readonly name = 'Nebula drift';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.nebula;

  private tex: Uint8Array = new Uint8Array(0);
  private image: ImageData | null = null;
  private px = new Uint32Array(0);
  private stars: Int32Array = new Int32Array(0);
  private drift = 0;
  private cycle = 0;

  protected setup(): void {
    this.tex = noiseTexture(TEX, 5, 7);
    this.image = this.g.createImageData(this.W, this.H);
    this.px = new Uint32Array(this.image.data.buffer);
    this.stars = Int32Array.from({ length: 140 }, () => Math.floor(Math.random() * this.W * this.H));
  }

  protected step(frame: AudioFrame): void {
    const { W, H, tex, px } = this;
    this.drift += RETRO_STEP * (6 + frame.bass * 30);
    this.cycle += RETRO_STEP * (6 + frame.rms * 40);
    const d = this.drift;
    const cyc = this.cycle | 0;
    const cycle = this.palette.cycle;
    const heat = this.palette.heat;
    const glow = 1 + frame.bass * 1.2 + (this.flashAllowed ? frame.beat * 0.5 : 0);
    for (let y = 0; y < H; y++) {
      const v1 = ((y + d * 0.35) | 0) & (TEX - 1);
      const v2 = (((y >> 1) - d * 0.2) | 0) & (TEX - 1);
      for (let x = 0; x < W; x++) {
        const a = tex[v1 * TEX + ((x + d) & (TEX - 1))];
        const b = tex[v2 * TEX + (((x >> 1) + 97 - d * 0.5) & (TEX - 1))];
        const n = (a + b) >> 1;
        const dens = Math.max(0, (n - 95) / 110);
        // Density sets brightness through the heat ramp; a little palette cycling tints it.
        const k = Math.min(1, dens * dens * glow);
        px[y * W + x] = blend(heat[(k * 255) | 0], scale(cycle[(n + cyc) & 255], k), 0.25);
      }
    }
    const twinkle = 0.85 + frame.treble * 2;
    for (const i of this.stars) {
      if (Math.random() < 0.97) px[i] = scale(0xffffffff, Math.min(1, twinkle * (0.5 + ((i * 7) % 5) / 10)));
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
