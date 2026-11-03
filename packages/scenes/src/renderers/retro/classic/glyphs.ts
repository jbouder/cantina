import type { AudioFrame } from '../../../frame';
import { logBinRanges } from '../bars';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';
import { bandLevels } from './shared';

const CW = 6;
const CH = 9;
const GLYPHS = 64;

/**
 * Bitmap glyph rain at 320 x 200, like the late-90s screensavers. Classic
 * counterpart of Digital rain: bass sets the fall speed, each column follows
 * its slice of the spectrum, treble makes glyphs flicker.
 */
export class GlyphRain extends RetroScene {
  readonly id = 'classic-glyphs';
  readonly name = 'Digital rain';
  protected readonly W = 320;
  protected readonly H = 198;
  protected natural = SCENE_PALETTES.rain;

  private image: ImageData | null = null;
  private px = new Uint32Array(0);
  private bank: Uint8Array[] = [];
  private cols = 0;
  private rows = 0;
  private cells = new Uint8Array(0);
  private pos = new Float32Array(0);
  private speed = new Float32Array(0);
  private trail = new Float32Array(0);
  private ranges: [number, number][] = [];
  private levels = new Float32Array(0);

  protected setup(): void {
    this.image = this.g.createImageData(this.W, this.H);
    this.px = new Uint32Array(this.image.data.buffer);
    this.cols = Math.floor(this.W / CW);
    this.rows = Math.floor(this.H / CH);
    this.cells = Uint8Array.from({ length: this.cols * this.rows }, () => Math.floor(Math.random() * GLYPHS));
    this.pos = Float32Array.from({ length: this.cols }, () => Math.random() * this.rows * 2 - this.rows);
    this.speed = Float32Array.from({ length: this.cols }, () => 0.6 + Math.random() * 0.9);
    this.trail = Float32Array.from({ length: this.cols }, () => 6 + Math.random() * 12);
    this.ranges = logBinRanges(this.cols, 1024, this.rc.sampleRate, 40, 10000);
    this.levels = new Float32Array(this.cols);
    // 5 x 7 glyphs; about half are mirror-symmetric, which reads as more character-like.
    for (let k = 0; k < GLYPHS; k++) {
      const bits = new Uint8Array(35);
      const mirror = Math.random() < 0.5;
      for (let y = 0; y < 7; y++) {
        for (let x = 0; x < 5; x++) {
          const sx = mirror && x > 2 ? 4 - x : x;
          bits[y * 5 + x] = mirror && x > 2 ? bits[y * 5 + sx] : Math.random() < 0.48 ? 1 : 0;
        }
      }
      this.bank.push(bits);
    }
  }

  protected step(frame: AudioFrame): void {
    const { W, px, cols, rows } = this;
    bandLevels(frame.freq, this.ranges, this.levels);
    px.fill(0xff000000 | 0x000400);
    const lut = this.palette.heat;
    const flicker = 0.01 + frame.treble * 0.25;

    for (let c = 0; c < cols; c++) {
      const level = this.levels[c];
      this.pos[c] += RETRO_STEP * this.speed[c] * (6 + frame.bass * 22);
      const trail = this.trail[c] + level * 14;
      if (this.pos[c] - trail > rows) {
        this.pos[c] = -Math.random() * 10;
        this.speed[c] = 0.6 + Math.random() * 0.9;
      }
      for (let r = 0; r < rows; r++) {
        const d = this.pos[c] - r;
        if (d < 0 || d >= trail) continue;
        const cell = r * cols + c;
        if (Math.random() < flicker) this.cells[cell] = Math.floor(Math.random() * GLYPHS);
        const head = d < 1;
        const k = head ? 1 : Math.pow(1 - d / trail, 1.6) * (0.55 + level * 1.2);
        const color = lut[Math.min(255, head ? 255 : 40 + k * 180) | 0];
        const bits = this.bank[this.cells[cell]];
        const ox = c * CW;
        const oy = r * CH + 1;
        for (let y = 0; y < 7; y++) {
          for (let x = 0; x < 5; x++) if (bits[y * 5 + x]) px[(oy + y) * W + ox + x] = color;
        }
      }
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
