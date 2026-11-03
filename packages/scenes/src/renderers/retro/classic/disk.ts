import type { AudioFrame } from '../../../frame';
import { PixelBuffer } from '../fx';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';
import { scale } from './shared';

const N = 2600;
const R_IN = 2.3;
const R_OUT = 9;
const TILT = 0.24;

/**
 * A particle accretion disk circling a black disc, with the far side of the
 * disk lensed into an arc over the top. Classic counterpart of Black hole:
 * bass heats the disk, mid spins it, beats flare the photon ring.
 */
export class ParticleDisk extends RetroScene {
  readonly id = 'classic-disk';
  readonly name = 'Black hole';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.ember;

  private fb!: PixelBuffer;
  private r = new Float32Array(N);
  private a = new Float32Array(N);
  private stars = new Int32Array(0);
  private ring = 0;

  protected setup(): void {
    this.fb = new PixelBuffer(this.g, this.W, this.H);
    for (let i = 0; i < N; i++) {
      this.r[i] = R_IN + (R_OUT - R_IN) * Math.random() ** 1.6;
      this.a[i] = Math.random() * Math.PI * 2;
    }
    this.stars = Int32Array.from({ length: 90 }, () => Math.floor(Math.random() * this.W * this.H));
  }

  protected step(frame: AudioFrame): void {
    const { fb, W, H } = this;
    const S = 17; // pixels per unit; the horizon has radius 1
    const cx = W / 2;
    const cy = H / 2 + 6;
    const lut = this.palette.heat;
    fb.fade(0.55);
    for (const i of this.stars) fb.px[i] = scale(0xffffffff, 0.35 + Math.random() * 0.2);

    const spin = 1 + frame.mid * 2;
    const heat = frame.bass * 50;
    const sparkle = frame.treble * 0.03;
    const color = (i: number, cosA: number) => {
      const t = 1 - (this.r[i] - R_IN) / (R_OUT - R_IN);
      // Doppler: the side moving toward us (left) is brighter.
      const k = 0.55 + 0.45 * t + 0.35 * -cosA;
      return Math.random() < sparkle ? 0xffffffff : scale(lut[Math.min(255, 70 + t * 160 + heat) | 0], k);
    };

    // Far half of the disk, then its lensed image arcing over the hole.
    for (let i = 0; i < N; i++) {
      this.a[i] += RETRO_STEP * (1.6 / Math.sqrt(this.r[i])) * spin;
      const sa = Math.sin(this.a[i]);
      if (sa >= 0) continue;
      const ca = Math.cos(this.a[i]);
      const c = color(i, ca);
      fb.dot(cx + ca * this.r[i] * S, cy + sa * this.r[i] * S * TILT, c);
      // Lensed image: the far disk bends up over the top of the hole as a wide arc.
      const lr = 1.5 + (this.r[i] - R_IN) * 0.42;
      fb.dot(cx + ca * lr * S, cy - Math.sqrt(Math.max(0, 1 - ca * ca)) * lr * S * 0.92, scale(c, 1.15), 2);
    }

    // Event horizon and photon ring
    this.ring = Math.max(this.ring * 0.85, frame.beat * (this.flashAllowed ? 1 : 0.3));
    const hr = S * 1.25;
    for (let y = -hr; y <= hr; y++) {
      const half = Math.sqrt(Math.max(0, hr * hr - y * y));
      for (let x = -half; x <= half; x++) fb.dot(cx + x, cy + y, 0xff000000);
    }
    const ringColor = scale(lut[230], 0.6 + this.ring * 0.8);
    for (let k = 0; k < 160; k++) {
      const t = (k / 160) * Math.PI * 2;
      fb.dot(cx + Math.cos(t) * (hr + 1), cy + Math.sin(t) * (hr + 1), ringColor);
    }

    // Near half, crossing in front of the hole
    for (let i = 0; i < N; i++) {
      const sa = Math.sin(this.a[i]);
      if (sa < 0) continue;
      const ca = Math.cos(this.a[i]);
      fb.dot(cx + ca * this.r[i] * S, cy + sa * this.r[i] * S * TILT, color(i, ca), this.r[i] < 5 ? 2 : 1);
    }
    fb.present(this.g);
  }
}
