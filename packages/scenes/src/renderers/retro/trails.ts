import type { AudioFrame } from '../../frame';
import { PixelBuffer, waveAt } from './fx';
import { PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const POINTS = 220;
const SHAPES = 3;

/** Scripted scope shapes leaving trails that zoom, spin and blur away. Beats change the shape. */
export class TrailsScene extends RetroScene {
  readonly id = 'trails';
  readonly name = 'Scope trails';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = PALETTES.vga;

  private fb!: PixelBuffer;
  private t = 0;
  private shape = 0;
  private dir = 1;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    this.t += RETRO_STEP;
    if (this.beatOnset(frame)) {
      this.dir = -this.dir;
      if (Math.random() < 0.35) this.shape = (this.shape + 1) % SHAPES;
    }

    fb.move(1.035 + frame.bass * 0.05, 0.02 * this.dir);
    fb.blur();
    fb.fade(0.97);

    const t = this.t;
    const lut = this.palette.cycle;
    let px = 0;
    let py = 0;
    for (let i = 0; i < POINTS; i++) {
      const s = i / (POINTS - 1);
      const v = waveAt(frame.wave, s);
      let x: number;
      let y: number;
      if (this.shape === 0) {
        // Spiral
        const r = (0.04 + s * 0.42) * (1 + v * 0.6);
        const a = s * Math.PI * 7 + t * 1.6 * this.dir;
        x = Math.cos(a) * r;
        y = Math.sin(a) * r;
      } else if (this.shape === 1) {
        // Tumbling ring
        const a = s * Math.PI * 2;
        const r = 0.32 + v * 0.18;
        const y3 = Math.sin(a) * r;
        const z = y3 * Math.sin(t * 0.9);
        const k = 1.4 / (1.4 + z);
        x = Math.cos(a) * r * k;
        y = y3 * Math.cos(t * 0.9) * k;
      } else {
        // Rotating sweep
        const bx = s * 1.6 - 0.8;
        const by = v * 0.5 * Math.sin(s * Math.PI);
        const a = t * 0.7 * this.dir;
        x = bx * Math.cos(a) - by * Math.sin(a);
        y = bx * Math.sin(a) + by * Math.cos(a);
      }
      const sx = W / 2 + x * H;
      const sy = H / 2 + y * H;
      if (i > 0) fb.line(px, py, sx, sy, lut[((s * 160 + t * 50) | 0) & 255]);
      px = sx;
      py = sy;
    }
    fb.present(this.g);
  }
}
