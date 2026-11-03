import type { AudioFrame } from '../../frame';
import { logBinRanges } from './bars';
import { PixelBuffer } from './fx';
import { PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const MAX = 4000;
const JETS = 48;
const GRAVITY = 2.4;

/** A 3D fountain of dots; each jet is one frequency band, so louder bands shoot higher. */
export class FountainScene extends RetroScene {
  readonly id = 'fountain';
  readonly name = 'Dot fountain';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = PALETTES.vga;

  private fb!: PixelBuffer;
  private ranges: [number, number][] = [];
  private x = new Float32Array(MAX);
  private y = new Float32Array(MAX);
  private z = new Float32Array(MAX);
  private vx = new Float32Array(MAX);
  private vy = new Float32Array(MAX);
  private vz = new Float32Array(MAX);
  private hue = new Uint8Array(MAX);
  private alive = new Uint8Array(MAX);
  private next = 0;
  private angle = 0;
  private t = 0;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
    this.ranges = logBinRanges(JETS, 1024, this.rc.sampleRate, 40, 12000);
  }

  private emit(j: number, level: number): void {
    const i = this.next;
    this.next = (this.next + 1) % MAX;
    const a = (j / JETS) * Math.PI * 2 + (Math.random() - 0.5) * 0.08;
    const out = 0.32 + Math.random() * 0.08;
    this.x[i] = Math.cos(a) * 0.04;
    this.z[i] = Math.sin(a) * 0.04;
    this.y[i] = -0.55;
    this.vx[i] = Math.cos(a) * out;
    this.vz[i] = Math.sin(a) * out;
    this.vy[i] = 0.5 + level * 2.4 + Math.random() * 0.1;
    this.hue[i] = ((j / JETS) * 255 + this.t * 30) & 255;
    this.alive[i] = 1;
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    this.t += RETRO_STEP;
    this.angle += RETRO_STEP * (0.25 + frame.mid * 0.8);
    const burst = this.beatOnset(frame) ? 3 : 1;

    for (let j = 0; j < JETS; j++) {
      const [a, b] = this.ranges[j];
      let v = 0;
      for (let k = a; k < b; k++) if (frame.freq[k] > v) v = frame.freq[k];
      const level = v / 255;
      if (level > 0.08) for (let n = 0; n < burst; n++) this.emit(j, level);
    }

    fb.fade(0.55);
    const cos = Math.cos(this.angle);
    const sin = Math.sin(this.angle);
    const lut = this.palette.cycle;
    for (let i = 0; i < MAX; i++) {
      if (!this.alive[i]) continue;
      this.vy[i] -= GRAVITY * RETRO_STEP;
      this.x[i] += this.vx[i] * RETRO_STEP;
      this.y[i] += this.vy[i] * RETRO_STEP;
      this.z[i] += this.vz[i] * RETRO_STEP;
      if (this.y[i] < -0.6) {
        this.alive[i] = 0;
        continue;
      }
      const xr = this.x[i] * cos - this.z[i] * sin;
      const zr = this.x[i] * sin + this.z[i] * cos;
      const k = (H * 1.5) / (zr + 2);
      fb.dot(W / 2 + xr * k, H * 0.36 - this.y[i] * k, lut[this.hue[i]], zr < -0.25 ? 2 : 1);
    }
    fb.present(this.g);
  }
}
