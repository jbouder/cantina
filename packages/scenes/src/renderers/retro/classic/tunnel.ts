import type { AudioFrame } from '../../../frame';
import { SCENE_PALETTES } from '../palettes';
import { RETRO_STEP, RetroScene } from '../RetroScene';
import { scale } from './shared';

const TEX = 256;

/**
 * The demoscene tunnel: per-pixel distance and angle tables index a texture
 * that scrolls toward the viewer. Classic counterpart of Warp tunnel.
 */
export class ClassicTunnel extends RetroScene {
  readonly id = 'classic-tunnel';
  readonly name = 'Warp tunnel';
  protected readonly W = 320;
  protected readonly H = 200;
  protected natural = SCENE_PALETTES.tunnel;

  private dist = new Uint8Array(0);
  private angle = new Uint8Array(0);
  private shade = new Float32Array(0);
  private tex = new Uint8Array(TEX * TEX);
  private image: ImageData | null = null;
  private px = new Uint32Array(0);
  private travel = 0;
  private spin = 0;
  private hue = 0;

  protected setup(): void {
    const { W, H } = this;
    this.dist = new Uint8Array(W * H);
    this.angle = new Uint8Array(W * H);
    this.shade = new Float32Array(W * H);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const dx = x - W / 2;
        const dy = y - H / 2;
        const d = Math.max(0.5, Math.sqrt(dx * dx + dy * dy));
        const i = y * W + x;
        this.dist[i] = ((24 * TEX) / d) & 255;
        this.angle[i] = ((Math.atan2(dy, dx) / Math.PI) * 0.5 + 0.5) * TEX * 2;
        this.shade[i] = Math.min(1, d / 110);
      }
    }
    // Ribbed walls: bright rings along the depth axis over an XOR pattern.
    for (let v = 0; v < TEX; v++) {
      for (let u = 0; u < TEX; u++) {
        const rib = (v & 31) < 3;
        this.tex[v * TEX + u] = rib ? 255 : 70 + ((u ^ v) & 63);
      }
    }
    this.image = this.g.createImageData(W, H);
    this.px = new Uint32Array(this.image.data.buffer);
  }

  protected step(frame: AudioFrame): void {
    this.travel += RETRO_STEP * (0.6 + frame.bass * 3.5) * 90;
    this.spin += RETRO_STEP * 14;
    this.hue += RETRO_STEP * 10;
    const flash = this.flashAllowed ? frame.beat : frame.beat * 0.25;
    const lut = this.palette.cycle;
    const { dist, angle, shade, tex, px } = this;
    const tr = this.travel | 0;
    const sp = this.spin | 0;
    const hue = this.hue | 0;
    const sparkle = frame.treble * 0.012;
    for (let i = 0; i < px.length; i++) {
      const v = (dist[i] + tr) & 255;
      const t = tex[v * TEX + ((angle[i] + sp) & 255)];
      const lit = (t / 255) * shade[i] * (t === 255 ? 1 + flash * 0.8 : 1);
      px[i] = Math.random() < sparkle * shade[i] ? 0xffffffff : scale(lut[(dist[i] + tr * 0.3 + hue) & 255], lit);
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
