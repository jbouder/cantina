import type { AudioFrame } from '../../frame';
import { SCENE_PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const MAX = 1500;
const GRAVITY = 70; // px/s²

/**
 * A fountain of embers that set the air alight: particles arc up and fall,
 * dropping heat into a fire buffer that rises and cools behind them.
 */
export class FlameFountainScene extends RetroScene {
  readonly id = 'flames';
  readonly name = 'Flame fountain';
  protected readonly W = 200;
  protected readonly H = 150;
  protected natural = SCENE_PALETTES.fire;

  private heat = new Uint8Array(0);
  private image: ImageData | null = null;
  private pixels = new Uint32Array(0);
  private x = new Float32Array(MAX);
  private y = new Float32Array(MAX);
  private vx = new Float32Array(MAX);
  private vy = new Float32Array(MAX);
  private life = new Float32Array(MAX);
  private next = 0;
  private sway = 0;

  protected setup(): void {
    this.heat = new Uint8Array(this.W * this.H);
    this.image = this.g.createImageData(this.W, this.H);
    this.pixels = new Uint32Array(this.image.data.buffer);
  }

  private emit(power: number, spread: number): void {
    const i = this.next;
    this.next = (this.next + 1) % MAX;
    const a = -Math.PI / 2 + this.sway + (Math.random() - 0.5) * spread;
    const speed = 45 + power * 95 + Math.random() * 12;
    this.x[i] = this.W / 2 + (Math.random() - 0.5) * 6;
    this.y[i] = this.H - 6;
    this.vx[i] = Math.cos(a) * speed;
    this.vy[i] = Math.sin(a) * speed;
    this.life[i] = 1;
  }

  protected step(frame: AudioFrame): void {
    const { W, H, heat } = this;
    this.sway = Math.sin(performance.now() / 1700) * 0.12;

    const spread = 0.5 + frame.treble * 2.2;
    const count = Math.floor(6 + frame.bass * 30) * (this.beatOnset(frame) ? 3 : 1);
    for (let n = 0; n < count; n++) this.emit(Math.min(1, frame.bass * 1.2 + frame.mid * 0.4), spread);

    // Embers drop heat as they fly.
    for (let i = 0; i < MAX; i++) {
      if (this.life[i] <= 0) continue;
      this.vy[i] += GRAVITY * RETRO_STEP;
      this.x[i] += this.vx[i] * RETRO_STEP;
      this.y[i] += this.vy[i] * RETRO_STEP;
      this.life[i] -= RETRO_STEP * 0.45;
      const xi = this.x[i] | 0;
      const yi = this.y[i] | 0;
      if (xi < 1 || xi >= W - 1 || yi < 1 || yi >= H - 1) {
        if (yi >= H - 1) this.life[i] = 0;
        continue;
      }
      const v = Math.min(255, 140 + this.life[i] * 115);
      const k = yi * W + xi;
      heat[k] = Math.max(heat[k], v);
      heat[k + 1] = Math.max(heat[k + 1], v * 0.8);
      heat[k + W] = Math.max(heat[k + W], v * 0.8);
    }

    // The fountain's base burns steadily.
    const base = (H - 2) * W;
    for (let x = W / 2 - 10; x < W / 2 + 10; x++) heat[base + x] = Math.min(255, 120 + frame.bass * 200 + Math.random() * 60);

    // Flames rise: each cell takes the average of the cells below it, minus cooling.
    for (let y = 0; y < H - 1; y++) {
      const row = y * W;
      const below = row + W;
      for (let x = 1; x < W - 1; x++) {
        const v = (heat[below + x - 1] + heat[below + x] * 2 + heat[below + x + 1] + heat[row + x] * 2) / 6 - (Math.random() * 6 + 1.5);
        heat[row + x] = v > 0 ? v : 0;
      }
    }

    const lut = this.palette.heat;
    const px = this.pixels;
    for (let i = 0; i < W * H; i++) px[i] = lut[heat[i]];
    this.g.putImageData(this.image!, 0, 0);
  }
}
