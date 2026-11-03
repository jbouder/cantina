import type { AudioFrame } from '../../frame';
import { RetroScene } from './RetroScene';

const DELAY = 32;

/** XY scope plotting the waveform against a delayed copy of itself. */
export class LissajousScene extends RetroScene {
  readonly id = 'lissajous';
  readonly name = 'Lissajous XY';
  protected readonly W = 256;
  protected readonly H = 256;

  protected step(frame: AudioFrame): void {
    const { g, W, H } = this;
    const p = this.palette;
    g.globalAlpha = 0.15;
    g.fillStyle = p.bg;
    g.fillRect(0, 0, W, H);
    g.globalAlpha = 0.4;
    g.fillStyle = p.dim;
    g.fillRect(W / 2, 0, 1, H);
    g.fillRect(0, H / 2, W, 1);
    g.globalAlpha = 1;

    const wave = frame.wave;
    const scale = (W / 2) * Math.min(1.6, 0.5 + frame.rms * 4);
    g.strokeStyle = p.fg;
    g.lineWidth = 1;
    g.beginPath();
    for (let i = 0; i < wave.length - DELAY; i += 2) {
      const x = W / 2 + ((wave[i] - 128) / 128) * scale;
      const y = H / 2 - ((wave[i + DELAY] - 128) / 128) * scale;
      if (i === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    }
    g.stroke();
  }
}
