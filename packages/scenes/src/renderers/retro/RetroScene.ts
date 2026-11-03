import type { AudioFrame } from '../../frame';
import type { RenderContext, Renderer } from '../../Renderer';
import { PALETTES, resolvePalette, type Palette } from './palettes';

export const RETRO_STEP = 1 / 30;
const MAX_STEPS_PER_FRAME = 4;

/**
 * Base for Canvas 2D scenes. Each draws into a low-resolution buffer that is
 * scaled up without smoothing and letterboxed into the visible canvas.
 * step() runs at a fixed 30 Hz for era-appropriate motion; draw() runs every
 * animation frame.
 */
export abstract class RetroScene implements Renderer {
  readonly family = 'retro';
  abstract readonly id: string;
  abstract readonly name: string;
  protected abstract readonly W: number;
  protected abstract readonly H: number;

  protected g!: CanvasRenderingContext2D;
  protected rc!: RenderContext;
  private buf: HTMLCanvasElement | null = null;
  private out: CanvasRenderingContext2D | null = null;
  private acc = 0;
  private rect = { x: 0, y: 0, w: 1, h: 1 };
  private lastPalette = '';
  private lastBeat = 0;
  /** The last beat and bar seen on the tempo map, as bar * 1024 + beat. */
  private lastTempoBeat = -1;
  private lastBar = -1;

  init(canvas: HTMLCanvasElement, rc: RenderContext): void {
    this.rc = rc;
    this.out = canvas.getContext('2d', { alpha: false });
    if (!this.out) throw new Error('Canvas 2D is not available.');
    this.buf = document.createElement('canvas');
    this.buf.width = this.W;
    this.buf.height = this.H;
    this.g = this.buf.getContext('2d', { alpha: false, willReadFrequently: false })!;
    this.g.fillStyle = this.palette.bg;
    this.g.fillRect(0, 0, this.W, this.H);
    this.lastPalette = rc.palette;
    this.setup();
  }

  resize(width: number, height: number, pixelRatio: number): void {
    if (!this.out) return;
    const c = this.out.canvas;
    c.width = Math.max(1, Math.round(width * pixelRatio));
    c.height = Math.max(1, Math.round(height * pixelRatio));
    let s = Math.min(c.width / this.W, c.height / this.H);
    if (s >= 2) s = Math.floor(s);
    const w = Math.round(this.W * s);
    const h = Math.round(this.H * s);
    this.rect = { x: Math.floor((c.width - w) / 2), y: Math.floor((c.height - h) / 2), w, h };
    this.out.imageSmoothingEnabled = false;
  }

  render(frame: AudioFrame, dt: number): void {
    if (!this.out || !this.buf) return;
    if (this.rc.palette !== this.lastPalette) {
      this.lastPalette = this.rc.palette;
      this.onPaletteChange();
    }
    this.acc += dt;
    let n = 0;
    while (this.acc >= RETRO_STEP && n < MAX_STEPS_PER_FRAME) {
      this.step(frame);
      this.acc -= RETRO_STEP;
      n++;
    }
    if (this.acc >= RETRO_STEP) this.acc = 0;

    const o = this.out;
    if (!this.rc.retroLook && this.drawHD) {
      this.drawHD(o, frame);
      return;
    }
    this.draw(frame);

    const { x, y, w, h } = this.rect;
    o.fillStyle = '#000';
    o.fillRect(0, 0, o.canvas.width, o.canvas.height);
    if (this.rc.retroLook || this.crisp) {
      o.imageSmoothingEnabled = false;
      o.drawImage(this.buf, x, y, w, h);
      return;
    }
    // Smooth look: filtered upscale plus a soft additive glow where supported.
    o.imageSmoothingEnabled = true;
    o.imageSmoothingQuality = 'high';
    o.drawImage(this.buf, x, y, w, h);
    if ('filter' in o) {
      o.save();
      o.filter = `blur(${Math.max(2, (w / this.W) * 1.5)}px)`;
      o.globalCompositeOperation = 'lighter';
      o.globalAlpha = 0.55;
      o.drawImage(this.buf, x, y, w, h);
      o.restore();
    }
  }

  dispose(): void {
    if (this.buf) this.buf.width = this.buf.height = 0;
    this.buf = null;
    this.out = null;
  }

  /** Always scale with hard pixels, even when the Retro look is off. */
  protected crisp = false;

  /** This scene's own colors, used when the palette setting is Scene colors. */
  protected natural: Palette = PALETTES.green;

  protected get palette(): Palette {
    return resolvePalette(this.rc.palette, this.natural);
  }

  /** How far we are between fixed steps (0-1), for smooth full-resolution drawing. */
  protected get stepFraction(): number {
    return Math.min(1, this.acc / RETRO_STEP);
  }

  /** False when Reduce flashing is on: skip strobe-like beat effects. */
  protected get flashAllowed(): boolean {
    return !this.rc.reduceFlashing;
  }

  /**
   * True on the first step after a beat onset. Call at most once per step:
   * several steps can run on the same frame.
   */
  protected beatOnset(frame: AudioFrame): boolean {
    const t = frame.tempo;
    let onset: boolean;
    if (t) {
      // On the grid: the step where the beat number changes.
      const key = t.bar * 1024 + t.beat;
      onset = key !== this.lastTempoBeat;
      this.lastTempoBeat = key;
    } else {
      onset = frame.beat > this.lastBeat + 0.5;
      this.lastTempoBeat = -1;
    }
    this.lastBeat = frame.beat;
    return onset;
  }

  /**
   * True on the first step of a new bar. Only a tempo map knows where bars
   * are, so it never fires on onsets. Call at most once per step.
   */
  protected barOnset(frame: AudioFrame): boolean {
    const bar = frame.tempo ? frame.tempo.bar : -1;
    const onset = bar >= 0 && bar !== this.lastBar;
    this.lastBar = bar;
    return onset;
  }

  /** The bar number on the tempo map, or 0 with none. */
  protected barCount(frame: AudioFrame): number {
    return frame.tempo?.bar ?? 0;
  }

  protected setup(): void {}
  protected onPaletteChange(): void {}
  /** Fixed 30 Hz simulation step. */
  protected abstract step(frame: AudioFrame): void;
  /** Per-animation-frame drawing into the low-res buffer. */
  protected draw(_frame: AudioFrame): void {}
  /**
   * Optional sharp, full-resolution drawing used when the Retro look is off,
   * for scenes whose shapes would blur when upscaled. Draws straight onto the
   * visible canvas, sized in device pixels.
   */
  protected drawHD?(out: CanvasRenderingContext2D, frame: AudioFrame): void;
}
