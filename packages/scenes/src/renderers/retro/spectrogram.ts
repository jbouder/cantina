import type { AudioFrame } from '../../frame';
import { SCENE_PALETTES } from './palettes';
import { RetroScene } from './RetroScene';

const LO_HZ = 40;
const HI_HZ = 16000;
/** Pixels scrolled per 30 Hz step. */
const SCROLL = 2;

/** A scrolling waterfall of the spectrum: time runs left to right, low notes at the bottom. */
export class SpectrogramScene extends RetroScene {
  readonly id = 'spectrogram';
  readonly name = 'Spectrogram';
  protected readonly W = 320;
  protected readonly H = 180;
  protected natural = SCENE_PALETTES.fire;

  private column: ImageData | null = null;
  private colPx = new Uint32Array(0);
  /** [start, end) bin range for each row, top row first. */
  private rows: [number, number][] = [];

  protected setup(): void {
    const { H } = this;
    this.column = this.g.createImageData(1, H);
    this.colPx = new Uint32Array(this.column.data.buffer);
    const binHz = this.rc.sampleRate / 2 / 1024;
    for (let y = 0; y < H; y++) {
      const f0 = LO_HZ * (HI_HZ / LO_HZ) ** (1 - (y + 1) / H);
      const f1 = LO_HZ * (HI_HZ / LO_HZ) ** (1 - y / H);
      const a = Math.max(1, Math.floor(f0 / binHz));
      this.rows.push([a, Math.max(a + 1, Math.ceil(f1 / binHz))]);
    }
  }

  protected step(frame: AudioFrame): void {
    const { g, W, H } = this;
    g.drawImage(g.canvas, -SCROLL, 0);
    const lut = this.palette.heat;
    for (let y = 0; y < H; y++) {
      const [a, b] = this.rows[y];
      let v = 0;
      for (let k = a; k < b && k < 1024; k++) if (frame.freq[k] > v) v = frame.freq[k];
      // Stretch contrast so quiet detail is still visible.
      this.colPx[y] = lut[Math.min(255, Math.max(0, (v - 40) * 1.3)) | 0];
    }
    for (let x = W - SCROLL; x < W; x++) g.putImageData(this.column!, x, 0);
  }
}
