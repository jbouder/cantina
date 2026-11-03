import type { AudioFrame } from '../../../frame';
import { PixelBuffer, waveAt } from '../fx';
import { PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';

/**
 * A feedback effect folded through mirrored wedges, like a studio preset with
 * a mirror pass. Classic counterpart of Kaleidoscope bloom: mid sets the fold
 * count, beats drop a bright seed in the middle.
 */
export class MirrorFeedback extends RetroScene {
  readonly id = 'classic-mirror';
  readonly name = 'Kaleidoscope bloom';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = PALETTES.vga;

  private fb!: PixelBuffer;
  private maps = new Map<number, Int32Array>();
  private t = 0;
  private folds = 6;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
  }

  /** Source index for each pixel: fold its angle into the first wedge. */
  private mapFor(folds: number): Int32Array {
    let map = this.maps.get(folds);
    if (map) return map;
    const { W, H } = this;
    const seg = (Math.PI * 2) / folds;
    map = new Int32Array(W * H);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const dx = x - W / 2;
        const dy = y - H / 2;
        const r = Math.sqrt(dx * dx + dy * dy);
        let a = Math.atan2(dy, dx);
        a = ((a % seg) + seg) % seg;
        a = Math.abs(a - seg / 2);
        const sx = Math.min(W - 1, Math.max(0, Math.round(W / 2 + Math.cos(a) * r)));
        const sy = Math.min(H - 1, Math.max(0, Math.round(H / 2 + Math.sin(a) * r)));
        map[y * W + x] = sy * W + sx;
      }
    }
    this.maps.set(folds, map);
    return map;
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    this.t += RETRO_STEP;
    // Ease the fold count so it doesn't flicker between values.
    const target = 4 + Math.round(frame.mid * 10);
    if (target !== this.folds && Math.random() < 0.08) this.folds = target;

    fb.move(1.035, 0.012);
    fb.blur();
    fb.fade(0.94);

    const lut = this.palette.cycle;
    const cx = W / 2;
    const cy = H / 2;
    let px = 0;
    let py = 0;
    for (let i = 0; i <= 96; i++) {
      const s = (i % 96) / 96;
      const a = s * Math.PI * 2;
      const r = H * (0.16 + 0.12 * waveAt(frame.wave, s) + 0.12 * frame.bass);
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i > 0) fb.line(px, py, x, y, lut[((this.t * 30 + s * 60) | 0) & 255]);
      px = x;
      py = y;
    }
    // A waveform spoke inside the first wedge: the mirror pass turns it into petals.
    const seg = (Math.PI * 2) / this.folds;
    const sa = seg * 0.3;
    const nx = -Math.sin(sa);
    const ny = Math.cos(sa);
    px = cx;
    py = cy;
    for (let i = 1; i <= 48; i++) {
      const s = i / 48;
      const w = waveAt(frame.wave, s) * H * 0.08;
      const x = cx + Math.cos(sa) * s * H * 0.55 + nx * w;
      const y = cy + Math.sin(sa) * s * H * 0.55 + ny * w;
      fb.line(px, py, x, y, lut[((this.t * 30 + 128 + s * 90) | 0) & 255]);
      px = x;
      py = y;
    }
    if (this.beatOnset(frame)) {
      const c = lut[((this.t * 30 + 128) | 0) & 255];
      const size = this.flashAllowed ? 9 : 4;
      for (let k = 0; k < 40; k++) fb.dot(cx + (Math.random() - 0.5) * size * 2, cy + (Math.random() - 0.5) * size * 2, c, 2);
    }

    fb.remap(this.mapFor(this.folds));
    fb.present(this.g);
  }
}
