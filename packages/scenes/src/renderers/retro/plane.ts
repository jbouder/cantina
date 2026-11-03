import type { AudioFrame } from '../../frame';
import { logBinRanges } from './bars';
import { PixelBuffer } from './fx';
import { SCENE_PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const COLS = 32;
const ROWS = 28;
const TILT = 0.55;

/** A rotating plane of dots: the front row is the live spectrum and older rows scroll back. */
export class PlaneScene extends RetroScene {
  readonly id = 'plane';
  readonly name = 'Dot plane';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.water;

  private fb!: PixelBuffer;
  private ranges: [number, number][] = [];
  private heights = new Float32Array(COLS * ROWS);
  private angle = 0.6;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
    this.ranges = logBinRanges(COLS, 1024, this.rc.sampleRate);
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H, heights } = this;
    this.angle += RETRO_STEP * (0.12 + frame.rms * 0.4);

    heights.copyWithin(COLS, 0, COLS * (ROWS - 1));
    for (let c = 0; c < COLS; c++) {
      const [a, b] = this.ranges[c];
      let v = 0;
      for (let k = a; k < b; k++) if (frame.freq[k] > v) v = frame.freq[k];
      heights[c] = v / 255;
    }

    fb.clear(0xff000000);
    const cos = Math.cos(this.angle);
    const sin = Math.sin(this.angle);
    const ct = Math.cos(TILT);
    const st = Math.sin(TILT);
    const lut = this.palette.heat;
    for (let r = 0; r < ROWS; r++) {
      for (let c = 0; c < COLS; c++) {
        const h = heights[r * COLS + c];
        const x = (c / (COLS - 1) - 0.5) * 2;
        const z = (r / (ROWS - 1) - 0.5) * 2;
        const y = h * 0.7 - 0.2;
        const xr = x * cos - z * sin;
        const zr = x * sin + z * cos;
        const yv = y * ct - zr * st;
        const zv = y * st + zr * ct + 3;
        const k = (H * 1.6) / zv;
        const fog = 1 - r / (ROWS + 4);
        fb.dot(W / 2 + xr * k, H * 0.55 - yv * k, lut[Math.min(255, (40 + h * 215) * fog) | 0], zv < 2.6 ? 2 : 1);
      }
    }
    fb.present(this.g);
  }
}
