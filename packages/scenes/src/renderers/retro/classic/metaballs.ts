import type { AudioFrame } from '../../../frame';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';

/**
 * 2D palette metaballs, the classic demo effect. Classic counterpart of Liquid
 * blobs: each ball follows a band, and rms widens how far they merge.
 */
export class Metaballs extends RetroScene {
  readonly id = 'classic-metaballs';
  readonly name = 'Liquid blobs';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.nebula;

  private image: ImageData | null = null;
  private px = new Uint32Array(0);
  private t = 0;
  private levels = new Float32Array(4);

  protected setup(): void {
    this.image = this.g.createImageData(this.W, this.H);
    this.px = new Uint32Array(this.image.data.buffer);
  }

  protected step(frame: AudioFrame): void {
    const { W, H, px } = this;
    this.t += RETRO_STEP;
    const t = this.t;
    const raw = [frame.bass, frame.mid, frame.treble * 2, frame.rms * 2];
    for (let i = 0; i < 4; i++) this.levels[i] += (raw[i] - this.levels[i]) * 0.3;

    const unit = H * 0.32;
    const bx: number[] = [];
    const by: number[] = [];
    const br2: number[] = [];
    for (let i = 0; i < 4; i++) {
      bx.push(W / 2 + Math.sin(t * 0.5 * (1 + i * 0.27) + i * 1.7) * 1.15 * unit);
      by.push(H / 2 + Math.cos(t * 0.43 * (1 + i * 0.19) + i * 2.3) * 0.8 * unit);
      const r = (0.3 + this.levels[i] * 0.65) * unit * (0.8 + frame.rms * 0.8);
      br2.push(r * r);
    }

    const lut = this.palette.heat;
    const edge = 1 - frame.beat * (this.flashAllowed ? 0.15 : 0.05);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        let f = 0;
        for (let i = 0; i < 4; i++) {
          const dx = x - bx[i];
          const dy = y - by[i];
          f += br2[i] / (dx * dx + dy * dy + 1);
        }
        // Outside: a faint glow. Inside: a bright body with a lit rim.
        const idx = f < edge ? f * f * 70 : Math.min(255, 150 + (f - edge) * 50);
        px[y * W + x] = lut[idx | 0];
      }
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
