import type { AudioFrame } from '../../frame';
import { optionValue, type OptionSchema } from '../../Renderer';
import { getScene } from '../../registry';
import { lutCss } from './palettes';
import { RetroScene } from './RetroScene';

const SPAN = 1024;

/** First rising zero crossing, so the trace holds steady. */
export function findTrigger(wave: Uint8Array, span: number): number {
  const limit = wave.length - span;
  for (let i = 1; i < limit; i++) {
    if (wave[i - 1] < 128 && wave[i] >= 128) return i;
  }
  return 0;
}

/** Phosphor oscilloscope with afterglow, in line or dot mode. */
export class ScopeScene extends RetroScene {
  readonly id = 'scope';
  readonly name = 'Oscilloscope';
  readonly options: OptionSchema = getScene('scope')!.options!;
  protected readonly W = 320;
  protected readonly H = 180;

  protected step(frame: AudioFrame): void {
    const g = this.g;
    const p = this.palette;
    const { W, H } = this;

    g.globalAlpha = 0.15;
    g.fillStyle = p.bg;
    g.fillRect(0, 0, W, H);
    g.globalAlpha = 0.35;
    g.fillStyle = p.dim;
    for (let x = 0; x <= W; x += 32) g.fillRect(x, 0, 1, H);
    for (let y = 10; y <= H; y += 32) g.fillRect(0, y, W, 1);
    g.fillRect(0, H / 2, W, 1);
    g.globalAlpha = 1;

    const wave = frame.wave;
    const start = findTrigger(wave, SPAN);
    const amp = H * 0.45;
    const mode = optionValue(this.rc, this.id, this.options, 'mode');
    // With a tempo map the trace takes a new colour on every bar.
    g.fillStyle = g.strokeStyle = frame.tempo ? lutCss(p.cycle, (this.barCount(frame) * 64 + 32) & 255) : p.fg;

    if (mode === 'solid') {
      // Filled from the center line, one column per pixel.
      for (let x = 0; x < W; x++) {
        const y = H / 2 - ((wave[start + ((x / W) * SPAN) | 0] - 128) / 128) * amp;
        g.fillRect(x, Math.min(y, H / 2), 1, Math.max(1, Math.abs(y - H / 2)));
      }
      return;
    }
    if (mode === 'dots') {
      for (let j = 0; j < SPAN; j += 3) {
        const x = (j / SPAN) * W;
        const y = H / 2 - ((wave[start + j] - 128) / 128) * amp;
        g.fillRect(x | 0, y | 0, 1, 1);
      }
      return;
    }
    g.lineWidth = this.flashAllowed && frame.beat > 0.8 ? 2 : 1;
    g.beginPath();
    for (let j = 0; j < SPAN; j += 2) {
      const x = (j / SPAN) * W;
      const y = H / 2 - ((wave[start + j] - 128) / 128) * amp;
      if (j === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    }
    g.stroke();
  }
}
