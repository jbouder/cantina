import type { AudioFrame } from '../../frame';
import { PixelBuffer } from './fx';
import { GLYPH_H, GLYPH_W, glyph } from './font5x7';
import { PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';
import { scale } from './classic/shared';

const TEXT =
  '      *** MUSIC VISUALIZER ***      YOU FOUND IT.      CODE, COPPER AND CHIPTUNE AT 320 X 200 AND 30 FRAMES PER SECOND.' +
  '      GREETINGS TO EVERYONE WHO REMEMBERS 256 COLOR PALETTES, SOUNDBLASTERS AND STAYING UP LATE FOR THE NEXT DEMO.' +
  "      THE TERMINAL HAS MORE: TRY 'SNAKE' AND 'BREAKOUT'.      PRESS N TO LEAVE.      ";
const SCROLL_SCALE = 2;
const CELL = (GLYPH_W + 1) * SCROLL_SCALE;
const LOGO = 'MV';

/** An old-school intro: parallax stars, copper bars, a vector-ball logo and a sine scroller. */
export class IntroScene extends RetroScene {
  readonly id = 'intro';
  readonly name = 'Intro';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = PALETTES.vga;
  protected crisp = true;

  private fb!: PixelBuffer;
  private t = 0;
  private scroll = 0;
  private stars: { x: number; y: number; layer: number }[] = [];
  private balls: { x: number; y: number }[] = [];
  private sprites: Uint32Array[] = [];
  private spritePalette = '';

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
    this.stars = Array.from({ length: 90 }, (_, i) => ({ x: Math.random() * this.W, y: Math.random() * 120, layer: (i % 3) + 1 }));
    // Logo points from the bitmap font: one ball per lit pixel.
    for (let li = 0; li < LOGO.length; li++) {
      const bits = glyph(LOGO[li]);
      for (let gy = 0; gy < GLYPH_H; gy++) {
        for (let gx = 0; gx < GLYPH_W; gx++) {
          if (bits[gy * GLYPH_W + gx]) this.balls.push({ x: li * (GLYPH_W + 2) + gx - (LOGO.length * (GLYPH_W + 2) - 2) / 2 + 0.5, y: gy - (GLYPH_H - 1) / 2 });
        }
      }
    }
  }

  /** Shaded ball sprites in a few sizes, lit from the upper left. */
  private buildSprites(): void {
    const lut = this.palette.heat;
    this.sprites = [];
    for (let r = 2; r <= 6; r++) {
      const d = r * 2 + 1;
      const s = new Uint32Array(d * d);
      for (let y = 0; y < d; y++) {
        for (let x = 0; x < d; x++) {
          const dx = (x - r) / r;
          const dy = (y - r) / r;
          const q = dx * dx + dy * dy;
          if (q > 1) continue;
          const light = Math.max(0, 1 - Math.hypot(dx + 0.4, dy + 0.4) * 0.8);
          s[y * d + x] = lut[Math.min(255, (60 + light * 195) | 0)];
        }
      }
      this.sprites.push(s);
    }
    this.spritePalette = this.palette.id;
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    const px = fb.px;
    if (this.spritePalette !== this.palette.id) this.buildSprites();
    this.t += RETRO_STEP;
    const t = this.t;
    fb.clear(0xff000000);

    // Parallax stars
    const speed = 1 + frame.bass * 3;
    for (const s of this.stars) {
      s.x -= s.layer * speed * 0.6;
      if (s.x < 0) s.x += W;
      px[(s.y | 0) * W + (s.x | 0)] = scale(0xffffffff, 0.25 + s.layer * 0.22);
    }

    // Copper bars
    const cycle = this.palette.cycle;
    const half = 6 + frame.bass * 6;
    for (let k = 0; k < 4; k++) {
      const cy = 64 + Math.sin(t * 1.3 + k * 0.85) * 46;
      const color = cycle[((k * 64 + t * 25) | 0) & 255];
      for (let dy = -half; dy <= half; dy++) {
        const y = Math.round(cy + dy);
        if (y < 0 || y >= H) continue;
        const c = scale(color, 1 - Math.abs(dy) / (half + 1));
        px.fill(c, y * W, y * W + W);
      }
    }

    // Vector-ball logo, sorted back to front
    const pulse = 1 + frame.beat * (this.flashAllowed ? 0.15 : 0.05);
    const ay = t * 0.9;
    const ax = Math.sin(t * 0.5) * 0.45;
    const unit = 8.5 * pulse;
    const pts = this.balls.map((b) => {
      const x1 = b.x * Math.cos(ay);
      const z1 = b.x * Math.sin(ay);
      const y2 = b.y * Math.cos(ax) - z1 * Math.sin(ax);
      const z2 = b.y * Math.sin(ax) + z1 * Math.cos(ax);
      const k = 9 / (9 + z2);
      return { sx: W / 2 + x1 * unit * k, sy: 64 + y2 * unit * k, z: z2, k };
    });
    pts.sort((a, b) => b.z - a.z);
    for (const p of pts) {
      const si = Math.max(0, Math.min(this.sprites.length - 1, Math.round((p.k - 0.6) * 6)));
      const sprite = this.sprites[si];
      const r = si + 2;
      const d = r * 2 + 1;
      for (let y = 0; y < d; y++) {
        const yy = (p.sy | 0) - r + y;
        if (yy < 0 || yy >= H) continue;
        for (let x = 0; x < d; x++) {
          const c = sprite[y * d + x];
          const xx = (p.sx | 0) - r + x;
          if (c && xx >= 0 && xx < W) px[yy * W + xx] = c;
        }
      }
    }

    // Sine scroller
    this.scroll += 2 * (1 + frame.bass);
    const base = 158;
    for (let x = 0; x < W; x++) {
      const pos = x + this.scroll;
      const ci = Math.floor(pos / CELL) % TEXT.length;
      const col = Math.floor((pos % CELL) / SCROLL_SCALE);
      if (col >= GLYPH_W) continue;
      const bits = glyph(TEXT[ci]);
      const y0 = Math.round(base + Math.sin(pos * 0.035 + t * 2) * 14);
      const c = cycle[((x * 0.8 + t * 40) | 0) & 255];
      for (let gy = 0; gy < GLYPH_H; gy++) {
        if (!bits[gy * GLYPH_W + col]) continue;
        for (let sy = 0; sy < SCROLL_SCALE; sy++) {
          const y = y0 + gy * SCROLL_SCALE + sy;
          if (y >= 0 && y < H) px[y * W + x] = c;
        }
      }
    }
    fb.present(this.g);
  }
}
