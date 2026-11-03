import type { AudioFrame } from '../../frame';
import { logBinRanges } from './bars';
import { PixelBuffer, waveAt } from './fx';
import { PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

const SPOKES = 48;
const RING_POINTS = 160;

/** A circular scope and spectrum spokes around two counter-rotating stars, streaming outward. */
export class RingScene extends RetroScene {
  readonly id = 'ring';
  readonly name = 'Ring scope';
  protected readonly W = 240;
  protected readonly H = 240;
  protected natural = PALETTES.vga;

  private fb!: PixelBuffer;
  private ranges: [number, number][] = [];
  private t = 0;
  private dir = 1;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
    this.ranges = logBinRanges(SPOKES, 1024, this.rc.sampleRate, 40, 12000);
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    this.t += RETRO_STEP;
    if (this.beatOnset(frame)) this.dir = -this.dir;

    fb.move(1.025, 0.015 * this.dir);
    fb.blur();
    fb.fade(0.9);

    const cx = W / 2;
    const cy = H / 2;
    const p = this.palette;
    const t = this.t;

    // Circular scope
    let px = 0;
    let py = 0;
    for (let i = 0; i <= RING_POINTS; i++) {
      const s = (i % RING_POINTS) / RING_POINTS;
      const a = s * Math.PI * 2;
      const r = H * (0.2 + waveAt(frame.wave, s) * 0.07);
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i > 0) fb.line(px, py, x, y, p.cycle[((s * 255 + t * 60) | 0) & 255]);
      px = x;
      py = y;
    }

    // Spectrum spokes
    for (let j = 0; j < SPOKES; j++) {
      const [a0, b0] = this.ranges[j];
      let v = 0;
      for (let k = a0; k < b0; k++) if (frame.freq[k] > v) v = frame.freq[k];
      const level = v / 255;
      const a = (j / SPOKES) * Math.PI * 2 - t * 0.3;
      const r0 = H * 0.29;
      const r1 = r0 + level * H * 0.17;
      fb.line(cx + Math.cos(a) * r0, cy + Math.sin(a) * r0, cx + Math.cos(a) * r1, cy + Math.sin(a) * r1, p.meter[(level * 255) | 0]);
    }

    // Two five-point stars, spinning opposite ways
    const size = H * (0.05 + frame.bass * 0.1);
    this.star(cx, cy, size, t * 1.3, p.heat[230]);
    this.star(cx, cy, size * 0.6, -t * 1.9, p.heat[180]);
    fb.present(this.g);
  }

  private star(cx: number, cy: number, r: number, rot: number, c: number): void {
    for (let i = 0; i < 5; i++) {
      const a0 = rot + (i * 4 * Math.PI) / 5;
      const a1 = rot + ((i + 1) * 4 * Math.PI) / 5;
      this.fb.line(cx + Math.cos(a0) * r, cy + Math.sin(a0) * r, cx + Math.cos(a1) * r, cy + Math.sin(a1) * r, c);
    }
  }
}
