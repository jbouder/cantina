import type { AudioFrame } from '../../frame';
import { PALETTES } from './palettes';
import { RETRO_STEP, RetroScene } from './RetroScene';

/** Sum-of-sines plasma with palette cycling. */
export class PlasmaScene extends RetroScene {
  readonly id = 'plasma';
  readonly name = 'Palette plasma';
  protected readonly W = 160;
  protected readonly H = 90;
  protected natural = PALETTES.vga;

  private image: ImageData | null = null;
  private pixels = new Uint32Array(0);
  private t = 0;
  private cycle = 0;
  private scale = 1;

  protected setup(): void {
    this.image = this.g.createImageData(this.W, this.H);
    this.pixels = new Uint32Array(this.image.data.buffer);
  }

  protected step(frame: AudioFrame): void {
    const { W, H } = this;
    this.t += RETRO_STEP;
    this.cycle = (this.cycle + RETRO_STEP * (12 + frame.rms * 260)) % 256;
    // Each bar on the tempo map turns the palette a quarter of the way round.
    if (this.barOnset(frame)) this.cycle = (this.cycle + 64) % 256;
    // Mid scales wave frequency; ease it so the pattern breathes rather than jitters.
    this.scale += (1 + frame.mid * 2.5 - this.scale) * 0.08;

    const t = this.t;
    const s = this.scale;
    const lut = this.palette.cycle;
    const px = this.pixels;
    const cx = W / 2 + Math.sin(t * 0.4) * W * 0.3;
    const cy = H / 2 + Math.cos(t * 0.3) * H * 0.3;
    const offset = this.cycle | 0;
    for (let y = 0; y < H; y++) {
      const sy = Math.sin(y * 0.07 * s + t * 0.7);
      for (let x = 0; x < W; x++) {
        const dx = x - cx;
        const dy = y - cy;
        const v =
          Math.sin(x * 0.06 * s + t) +
          sy +
          Math.sin((x * 0.04 + y * 0.05) * s + t * 1.3) +
          Math.sin(Math.sqrt(dx * dx + dy * dy) * 0.09 * s - t * 1.1);
        // Posterize to 32 bands for the banded look of 8-bit palette plasma.
        px[y * W + x] = lut[(((((v + 4) * 32) | 0) + offset) & 255) & ~7];
      }
    }
    this.g.putImageData(this.image!, 0, 0);
  }
}
