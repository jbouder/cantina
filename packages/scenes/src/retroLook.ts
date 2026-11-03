import type { AudioFrame } from './frame';
import type { PresetInfo, RenderContext, Renderer } from './Renderer';
import { PALETTES, resolvePalette, type Palette } from './renderers/retro/palettes';

/** About 320 x 200 worth of pixels, whatever the aspect ratio. */
const TARGET_PIXELS = 64000;
const STEP = 1 / 30;

// 4 x 4 ordered-dither thresholds, centred on zero.
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map((v) => (v + 0.5) / 16 - 0.5);
const MONO_LEVELS = 10;

/**
 * Maps packed RGBA pixels (little-endian ABGR) to the palette in place, with
 * ordered dithering: the mono palettes become a monitor-style ramp, VGA
 * becomes 64 colors (4 levels per channel).
 */
export function quantize(px: Uint32Array, width: number, palette: Palette): void {
  const vga = palette.id === 'vga';
  const lut = palette.heat;
  for (let i = 0; i < px.length; i++) {
    const c = px[i];
    const r = (c & 255) / 255;
    const g = ((c >>> 8) & 255) / 255;
    const b = ((c >>> 16) & 255) / 255;
    const d = BAYER[((((i / width) | 0) & 3) << 2) | (i % width & 3)];
    if (vga) {
      const q = (v: number) => Math.max(0, Math.min(3, Math.round(v * 3 + d))) * 85;
      px[i] = 0xff000000 | (q(b) << 16) | (q(g) << 8) | q(r);
    } else {
      const lum = 0.3 * r + 0.59 * g + 0.11 * b;
      const level = Math.max(0, Math.min(MONO_LEVELS - 1, Math.round(lum * (MONO_LEVELS - 1) * 1.15 + d)));
      px[i] = lut[Math.round((level / (MONO_LEVELS - 1)) * 255)];
    }
  }
}

/** 16-bit (5-6-5) color with no dithering, as 2001-era hardware drew it. */
export function to16Bit(px: Uint32Array): void {
  for (let i = 0; i < px.length; i++) px[i] = (px[i] & 0xfff8fcf8) | 0xff000000;
}

export interface RetroLookOptions {
  /** Keep the scene's own colors at 16-bit instead of mapping to a palette. */
  native?: boolean;
  /** Internal resolution in pixels (aspect follows the screen). */
  pixels?: number;
}

/**
 * Wraps a GPU scene so it renders in the Retro look: drawn small on a hidden
 * canvas, reduced to 16-bit color or quantized to the palette at 30 Hz, then
 * scaled up with hard pixels. Scenes with a classic counterpart use that instead.
 */
export class RetroLook implements Renderer {
  readonly family = 'retro';
  private src = document.createElement('canvas');
  private buf = document.createElement('canvas');
  private g: CanvasRenderingContext2D | null = null;
  private out: CanvasRenderingContext2D | null = null;
  private rc: RenderContext | null = null;
  private acc = 0;
  private w = 1;
  private h = 1;
  private dirty = false;

  constructor(
    readonly inner: Renderer,
    private readonly opts: RetroLookOptions = {},
  ) {}

  get id(): string {
    return this.inner.id;
  }
  get name(): string {
    return this.inner.name;
  }
  get presets(): PresetInfo[] | undefined {
    return this.inner.presets;
  }
  get currentPreset(): string | undefined {
    return this.inner.currentPreset;
  }
  get options() {
    return this.inner.options;
  }
  get setPreset() {
    return this.inner.setPreset?.bind(this.inner);
  }

  async init(canvas: HTMLCanvasElement, rc: RenderContext): Promise<void> {
    this.rc = rc;
    this.out = canvas.getContext('2d', { alpha: false });
    this.g = this.buf.getContext('2d', { alpha: false, willReadFrequently: true });
    if (!this.out || !this.g) throw new Error('Canvas 2D is not available.');
    await this.inner.init(this.src, rc);
  }

  resize(width: number, height: number, pixelRatio: number): void {
    if (!this.out) return;
    this.out.canvas.width = Math.max(1, Math.round(width * pixelRatio));
    this.out.canvas.height = Math.max(1, Math.round(height * pixelRatio));
    const k = Math.min(1, Math.sqrt((this.opts.pixels ?? TARGET_PIXELS) / Math.max(1, width * height)));
    this.w = Math.max(16, Math.round(width * k));
    this.h = Math.max(16, Math.round(height * k));
    this.buf.width = this.w;
    this.buf.height = this.h;
    this.inner.resize(this.w, this.h, 1);
    this.acc = STEP; // redraw on the next frame
  }

  render(frame: AudioFrame, dt: number): void {
    const { g, out, rc } = this;
    if (!g || !out || !rc) return;
    this.acc += dt;
    if (this.acc >= STEP) {
      this.inner.render(frame, this.acc);
      this.acc = 0;
      g.drawImage(this.src, 0, 0, this.w, this.h);
      const img = g.getImageData(0, 0, this.w, this.h);
      const px = new Uint32Array(img.data.buffer);
      if (this.opts.native) to16Bit(px);
      else quantize(px, this.w, resolvePalette(rc.palette, PALETTES.green));
      g.putImageData(img, 0, 0);
      this.dirty = true;
    }
    if (this.dirty) {
      out.imageSmoothingEnabled = false;
      out.drawImage(this.buf, 0, 0, out.canvas.width, out.canvas.height);
      this.dirty = false;
    }
  }

  dispose(): void {
    this.inner.dispose();
    this.src.width = this.src.height = 0;
    this.buf.width = this.buf.height = 0;
    this.g = this.out = null;
  }
}
