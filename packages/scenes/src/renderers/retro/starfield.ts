import type { AudioFrame } from '../../frame';
import { PALETTES, lutCss } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const STARS = 520;
const BUCKETS = 8;

export interface StarfieldOptions {
  /** Hyperspace-style: faster, with long streaks. */
  warp?: boolean;
}

/** 3D stars flying toward the camera with short trails (or long streaks in warp). */
export class StarfieldScene extends RetroScene {
  readonly id = 'starfield';
  readonly name = 'Starfield';
  protected readonly W = 320;
  protected readonly H = 180;
  protected natural = PALETTES.ice;

  private x = new Float32Array(STARS);
  private y = new Float32Array(STARS);
  private z = new Float32Array(STARS);
  private pz = new Float32Array(STARS);
  private colors: string[] = [];
  private bump = 0;
  private speed = 0;
  private readonly warp: boolean;

  constructor(opts: StarfieldOptions = {}) {
    super();
    this.warp = !!opts.warp;
  }

  protected setup(): void {
    for (let i = 0; i < STARS; i++) this.reset(i, Math.random());
    this.onPaletteChange();
  }

  protected onPaletteChange(): void {
    this.colors = Array.from({ length: BUCKETS }, (_, b) => lutCss(this.palette.heat, 90 + (b / (BUCKETS - 1)) * 165));
  }

  private reset(i: number, z: number): void {
    this.x[i] = (Math.random() * 2 - 1) * 1.6;
    this.y[i] = (Math.random() * 2 - 1) * 0.9;
    this.z[i] = this.pz[i] = z;
  }

  protected step(frame: AudioFrame): void {
    this.bump = Math.max(this.bump * 0.85, frame.beat);
    // A surge on each bar's downbeat, twice a beat's.
    if (this.barOnset(frame)) this.bump = 2;
    this.speed = this.warp ? 0.35 + frame.bass * 2.6 + this.bump * 1.4 : 0.12 + frame.bass * 1.1 + this.bump * 0.7;
    for (let i = 0; i < STARS; i++) {
      this.pz[i] = this.z[i];
      this.z[i] -= this.speed * RETRO_STEP;
      // Recycle stars that pass the camera or leave even a wide screen.
      if (this.z[i] <= 0.02 || Math.abs(this.x[i] / this.z[i]) > 2.6 || Math.abs(this.y[i] / this.z[i]) > 1.1) this.reset(i, 1);
    }
  }

  protected draw(): void {
    this.paint(this.g, this.W, this.H, 1, 1);
  }

  /** With the Retro look off: sharp streaks at screen resolution, moving smoothly between steps. */
  protected drawHD(out: CanvasRenderingContext2D): void {
    const h = out.canvas.height;
    this.paint(out, out.canvas.width, h, Math.max(1, h / 300), this.stepFraction);
  }

  private paint(g: CanvasRenderingContext2D, W: number, H: number, lineScale: number, t: number): void {
    const f = H * 0.5;
    g.fillStyle = this.palette.bg;
    g.fillRect(0, 0, W, H);
    g.lineCap = lineScale > 1 ? 'round' : 'butt';
    const trailSteps = this.warp ? 6 : 2;

    const paths: Path2D[] = Array.from({ length: BUCKETS }, () => new Path2D());
    for (let i = 0; i < STARS; i++) {
      // Interpolate between the last two steps so full-resolution motion stays smooth.
      const z = this.pz[i] + (this.z[i] - this.pz[i]) * t;
      if (z <= 0.02 || this.z[i] > this.pz[i]) continue; // just recycled
      const sx = W / 2 + (this.x[i] / z) * f;
      const sy = H / 2 + (this.y[i] / z) * f;
      if (sx < 0 || sx >= W || sy < 0 || sy >= H) continue;
      const tz = Math.min(1, z + this.speed * RETRO_STEP * trailSteps);
      const b = Math.min(BUCKETS - 1, Math.floor((1 - z) * BUCKETS));
      paths[b].moveTo(W / 2 + (this.x[i] / tz) * f, H / 2 + (this.y[i] / tz) * f);
      paths[b].lineTo(sx + 0.5, sy + 0.5);
    }
    for (let b = 0; b < BUCKETS; b++) {
      g.strokeStyle = this.colors[b];
      g.lineWidth = lineScale * (1 + (b / (BUCKETS - 1)) * (lineScale > 1 ? 1.2 : 0));
      g.stroke(paths[b]);
    }
  }
}
