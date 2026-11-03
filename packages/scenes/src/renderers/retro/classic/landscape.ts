import type { AudioFrame } from '../../../frame';
import { lutCss, SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';

const ROWS = 22;
const COLS = 41;
const CENTER = (COLS - 1) / 2;
const HORIZON = 84;
const FOCAL = 92;
const Z_FAR = 14;
const Z_NEAR = 1.3;
const LEVELS = 12;

/**
 * A wireframe 3D spectrum landscape with hidden lines removed, scrolling
 * toward a striped low sun. Classic counterpart of Spectrum grid.
 */
export class WireLandscape extends RetroScene {
  readonly id = 'classic-landscape';
  readonly name = 'Spectrum grid';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.sunset;

  private rows = new Float32Array(ROWS * COLS); // row 0 is the far horizon
  private offset = 0;
  private sunPulse = 0;
  private lines: string[] = [];
  private sky: string[] = [];
  private stars: [number, number][] = [];
  private t = 0;

  protected setup(): void {
    this.stars = Array.from({ length: 60 }, () => [Math.random() * this.W, Math.random() * (HORIZON - 20)] as [number, number]);
    this.onPaletteChange();
  }

  protected onPaletteChange(): void {
    const p = this.palette;
    this.lines = Array.from({ length: LEVELS }, (_, i) => lutCss(p.heat, 110 + (i / (LEVELS - 1)) * 145));
    this.sky = Array.from({ length: 8 }, (_, i) => lutCss(p.heat, 8 + i * 9));
  }

  private newRow(frame: AudioFrame): void {
    const { rows } = this;
    rows.copyWithin(COLS, 0, (ROWS - 1) * COLS);
    const binHz = this.rc.sampleRate / 2 / 1024;
    for (let c = 0; c < COLS; c++) {
      const ax = Math.abs(c - CENTER) / CENTER;
      const side = Math.min(1, Math.max(0, (ax - 0.12) / 0.33));
      const hz = 60 * (8000 / 60) ** ax;
      const bin = Math.min(1023, Math.round(hz / binHz));
      const v = frame.freq[bin] / 255;
      rows[c] = side * (0.12 + v * v * 2.6) * (0.75 + Math.random() * 0.25);
    }
  }

  protected step(frame: AudioFrame): void {
    const { g, W, H } = this;
    this.t += RETRO_STEP;
    this.offset += RETRO_STEP * (2.5 + frame.bass * 7);
    while (this.offset >= 1) {
      this.offset -= 1;
      this.newRow(frame);
    }
    this.sunPulse = Math.max(this.sunPulse * 0.85, frame.beat);

    // Sky bands and stars
    const band = Math.ceil(HORIZON / this.sky.length);
    for (let i = 0; i < this.sky.length; i++) {
      g.fillStyle = this.sky[this.sky.length - 1 - i];
      g.fillRect(0, i * band, W, band);
    }
    g.fillStyle = this.palette.fg;
    for (const [x, y] of this.stars) if (Math.random() > 0.02) g.fillRect(x | 0, y | 0, 1, 1);

    // Low sun with scan stripes through its lower half
    const sr = 34 * (1 + 0.07 * this.sunPulse);
    const scx = W / 2;
    const scy = HORIZON - 14;
    const heat = this.palette.meter;
    for (let y = Math.floor(scy - sr); y <= Math.min(HORIZON, scy + sr); y++) {
      const dy = y - scy;
      const half = Math.sqrt(Math.max(0, sr * sr - dy * dy));
      if (dy > 0) {
        const k = dy / sr;
        if ((y + Math.floor(this.t * 6)) % 6 < 1 + k * 4) continue;
      }
      g.fillStyle = lutCss(heat, 255 - ((dy + sr) / (2 * sr)) * 230);
      g.fillRect(Math.round(scx - half), y, Math.round(half * 2), 1);
    }

    // Ground
    g.fillStyle = this.palette.bg;
    g.fillRect(0, HORIZON, W, H - HORIZON);

    // Terrain quads from far to near: fill to hide what's behind, then stroke.
    const pts = (r: number, c: number): [number, number] => {
      const z = Z_FAR - ((r + this.offset) / (ROWS - 1)) * (Z_FAR - Z_NEAR);
      const h = this.rows[r * COLS + c];
      return [W / 2 + ((c - CENTER) * 0.55 * FOCAL) / z, HORIZON + ((1 - h) * FOCAL * 0.55) / z];
    };
    g.lineWidth = 1;
    for (let r = 1; r < ROWS; r++) {
      for (let c = 0; c < COLS - 1; c++) {
        const a = pts(r - 1, c);
        const b = pts(r - 1, c + 1);
        const d = pts(r, c + 1);
        const e = pts(r, c);
        g.beginPath();
        g.moveTo(a[0], a[1]);
        g.lineTo(b[0], b[1]);
        g.lineTo(d[0], d[1]);
        g.lineTo(e[0], e[1]);
        g.closePath();
        g.fillStyle = this.palette.bg;
        g.fill();
        const h = Math.max(this.rows[r * COLS + c], this.rows[r * COLS + c + 1]);
        g.strokeStyle = this.lines[Math.min(LEVELS - 1, Math.floor((h / 1.6) * LEVELS))];
        g.stroke();
      }
    }
  }
}
