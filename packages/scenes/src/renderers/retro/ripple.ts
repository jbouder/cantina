import type { AudioFrame } from '../../frame';
import { SCENE_PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

/** Water ripples over a palette pattern. Beats drop stones, treble adds rain, the waveform stirs the surface. */
export class RippleScene extends RetroScene {
  readonly id = 'ripple';
  readonly name = 'Ripple';
  protected readonly W = 200;
  protected readonly H = 150;
  protected natural = SCENE_PALETTES.water;

  private cur = new Int16Array(0);
  private prev = new Int16Array(0);
  private tex = new Uint8Array(0);
  private image: ImageData | null = null;
  private pixels = new Uint32Array(0);
  private offset = 0;

  protected setup(): void {
    const { W, H } = this;
    this.cur = new Int16Array(W * H);
    this.prev = new Int16Array(W * H);
    this.tex = new Uint8Array(W * H);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const dx = x - W / 2;
        const dy = y - H / 2;
        const d = Math.sqrt(dx * dx + dy * dy);
        // The angle term spans exactly 256 steps, so it wraps without a seam.
        this.tex[y * W + x] = (d * 3 + (Math.atan2(dy, dx) / Math.PI) * 128) & 255;
      }
    }
    this.image = this.g.createImageData(W, H);
    this.pixels = new Uint32Array(this.image.data.buffer);
  }

  private drop(x: number, y: number, r: number, strength: number): void {
    const { W, H } = this;
    for (let dy = -r; dy <= r; dy++) {
      for (let dx = -r; dx <= r; dx++) {
        const xx = x + dx;
        const yy = y + dy;
        if (xx < 1 || yy < 1 || xx >= W - 1 || yy >= H - 1 || dx * dx + dy * dy > r * r) continue;
        this.cur[yy * W + xx] = Math.max(-2000, Math.min(2000, this.cur[yy * W + xx] + strength));
      }
    }
  }

  protected step(frame: AudioFrame): void {
    const { W, H } = this;
    this.offset = (this.offset + RETRO_STEP * (8 + frame.rms * 120)) % 256;

    if (this.beatOnset(frame)) {
      this.drop(8 + ((Math.random() * (W - 16)) | 0), 8 + ((Math.random() * (H - 16)) | 0), 5, 600 + frame.bass * 1200);
    }
    if (Math.random() < frame.treble * 3) {
      this.drop(2 + ((Math.random() * (W - 4)) | 0), 2 + ((Math.random() * (H - 4)) | 0), 1, 160);
    }
    // The waveform drags across the middle of the pool.
    for (let x = 2; x < W - 2; x += 2) {
      const v = (frame.wave[(x / W) * 2048 | 0] - 128) / 128;
      const y = (H / 2 + v * H * 0.35) | 0;
      if (y > 1 && y < H - 1) this.cur[y * W + x] += (v * 90) | 0;
    }

    // Classic two-buffer water: new = (sum of neighbours) / 2 - previous, damped.
    const { cur, prev } = this;
    for (let y = 1; y < H - 1; y++) {
      for (let x = 1, i = y * W + 1; x < W - 1; x++, i++) {
        let n = ((cur[i - 1] + cur[i + 1] + cur[i - W] + cur[i + W]) >> 1) - prev[i];
        n -= n >> 5;
        prev[i] = n;
      }
    }
    [this.cur, this.prev] = [this.prev, this.cur];

    // Refract the pattern through the surface slope and shade by it.
    const h = this.cur;
    const lut = this.palette.cycle;
    const off = this.offset | 0;
    const px = this.pixels;
    for (let y = 1; y < H - 1; y++) {
      for (let x = 1, i = y * W + 1; x < W - 1; x++, i++) {
        const dx = h[i - 1] - h[i + 1];
        const dy = h[i - W] - h[i + W];
        const sx = Math.min(W - 1, Math.max(0, x + (dx >> 3)));
        const sy = Math.min(H - 1, Math.max(0, y + (dy >> 3)));
        px[i] = lut[(this.tex[sy * W + sx] + off + (dx >> 2)) & 255];
      }
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
