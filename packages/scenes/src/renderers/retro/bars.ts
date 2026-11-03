import type { AudioFrame } from '../../frame';
import { optionValue, type OptionSchema } from '../../Renderer';
import { getScene } from '../../registry';
import { lutCss } from './palettes';
import { RetroScene } from './RetroScene';

const BARS = 32;
const BAR_W = 3;
const PITCH = 4;
const SEG_H = 2; // 1 lit row + 1 gap
const SEGS = 30;
const PEAK_HOLD_STEPS = 15; // ~0.5 s at 30 Hz
const LO_HZ = 40;
const HI_HZ = 16000;

/** Log-spaced [start, end) bin ranges for each bar. */
export function logBinRanges(bars: number, binCount: number, sampleRate: number, lo = LO_HZ, hi = HI_HZ): [number, number][] {
  const binHz = sampleRate / 2 / binCount;
  const ranges: [number, number][] = [];
  let prevEnd = Math.max(1, Math.floor(lo / binHz));
  for (let i = 0; i < bars; i++) {
    const hz = lo * (hi / lo) ** ((i + 1) / bars);
    const end = Math.min(binCount, Math.max(prevEnd + 1, Math.round(hz / binHz)));
    ranges.push([prevEnd, end]);
    prevEnd = end;
  }
  return ranges;
}

/** 32 quantized bars with a low-to-high gradient and falling peak caps. */
export class BarsScene extends RetroScene {
  readonly id = 'bars';
  readonly name = 'Spectrum bars';
  readonly options: OptionSchema = getScene('bars')!.options!;
  protected readonly W = 152;
  protected readonly H = 64;

  private ranges: [number, number][] = [];
  private heights = new Float32Array(BARS);
  private peaks = new Float32Array(BARS);
  private holds = new Int16Array(BARS);
  private lit: string[] = [];
  private unlit = '';

  protected setup(): void {
    this.ranges = logBinRanges(BARS, 1024, this.rc.sampleRate);
    this.onPaletteChange();
  }

  protected onPaletteChange(): void {
    const p = this.palette;
    this.lit = Array.from({ length: SEGS }, (_, s) => lutCss(p.meter, (s / (SEGS - 1)) * 255));
    this.unlit = p.dim;
  }

  protected step(frame: AudioFrame): void {
    for (let i = 0; i < BARS; i++) {
      const [a, b] = this.ranges[i];
      let v = 0;
      for (let k = a; k < b; k++) if (frame.freq[k] > v) v = frame.freq[k];
      // Lift the treble end a little; real music falls off steeply there.
      const tilt = 0.85 + 0.35 * (i / BARS);
      const target = Math.min(SEGS, Math.round((v / 255) * tilt * SEGS));
      this.heights[i] = Math.max(target, this.heights[i] - 1.5);

      if (this.heights[i] >= this.peaks[i]) {
        this.peaks[i] = this.heights[i];
        this.holds[i] = PEAK_HOLD_STEPS;
      } else if (this.holds[i] > 0) {
        this.holds[i]--;
      } else {
        this.peaks[i] = Math.max(0, this.peaks[i] - 0.75);
      }
    }
  }

  protected draw(frame: AudioFrame): void {
    const x0 = Math.floor((this.W - (BARS * PITCH - (PITCH - BAR_W))) / 2);
    this.paint(this.g, frame, { width: this.W, height: this.H, x0, base: this.H - 2, pitch: PITCH, barW: BAR_W, segPitch: SEG_H, segH: 1 });
  }

  /** With the Retro look off: the same analyzer drawn crisply at screen resolution. */
  protected drawHD(out: CanvasRenderingContext2D, frame: AudioFrame): void {
    const width = out.canvas.width;
    const height = out.canvas.height;
    const areaW = width * 0.86;
    const areaH = Math.min(height * 0.6, areaW * 0.42);
    const pitch = Math.floor(areaW / BARS);
    const segPitch = Math.max(2, Math.floor(areaH / SEGS));
    this.paint(out, frame, {
      width,
      height,
      x0: Math.round((width - pitch * BARS) / 2 + pitch * 0.1),
      base: Math.round(height / 2 + (segPitch * SEGS) / 2),
      pitch,
      barW: Math.max(1, Math.round(pitch * 0.8)),
      segPitch,
      segH: Math.max(1, Math.round(segPitch * 0.62)),
    });
  }

  private paint(
    g: CanvasRenderingContext2D,
    frame: AudioFrame,
    l: { width: number; height: number; x0: number; base: number; pitch: number; barW: number; segPitch: number; segH: number },
  ): void {
    const p = this.palette;
    g.fillStyle = p.bg;
    g.fillRect(0, 0, l.width, l.height);
    if (this.flashAllowed && frame.beat > 0.85) {
      g.fillStyle = p.dim;
      g.globalAlpha = (frame.beat - 0.85) * 3;
      g.fillRect(0, 0, l.width, l.height);
      g.globalAlpha = 1;
    }

    // Normal: color by segment height. Fire: hottest at each bar's top. Line: one color per bar.
    const style = optionValue(this.rc, this.id, this.options, 'style');
    for (let i = 0; i < BARS; i++) {
      const x = l.x0 + i * l.pitch;
      const h = Math.floor(this.heights[i]);
      for (let s = 0; s < SEGS; s++) {
        if (s >= h) g.fillStyle = this.unlit;
        else if (style === 'fire') g.fillStyle = this.lit[SEGS - h + s];
        else if (style === 'line') g.fillStyle = this.lit[h - 1];
        else g.fillStyle = this.lit[s];
        g.fillRect(x, l.base - s * l.segPitch - l.segH, l.barW, l.segH);
      }
      const pk = Math.floor(this.peaks[i]);
      if (pk > 0) {
        g.fillStyle = p.fg;
        g.fillRect(x, l.base - Math.min(SEGS - 1, pk) * l.segPitch - l.segH, l.barW, l.segH);
      }
    }
  }
}
