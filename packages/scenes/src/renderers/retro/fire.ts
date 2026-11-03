import type { AudioFrame } from '../../frame';
import { SCENE_PALETTES } from './palettes';
import { RetroScene } from './RetroScene';

/** Classic palette-mapped fire: a heat buffer rising through a scrolling cooling map. */
export class FireScene extends RetroScene {
  readonly id = 'fire';
  readonly name = 'Fire';
  protected readonly W = 160;
  protected readonly H = 100;
  protected natural = SCENE_PALETTES.fire;

  private heat = new Uint8Array(0);
  private cool: Uint8Array = new Uint8Array(0);
  private image: ImageData | null = null;
  private pixels = new Uint32Array(0);
  private coolOffset = 0;

  protected setup(): void {
    const { W, H } = this;
    this.heat = new Uint8Array(W * (H + 1));
    this.cool = buildCoolingMap(W, H);
    this.image = this.g.createImageData(W, H);
    this.pixels = new Uint32Array(this.image.data.buffer);
  }

  protected step(frame: AudioFrame): void {
    const { W, H, heat, cool } = this;
    const bottom = H * W;
    const level = Math.min(1, 0.35 + frame.bass * 1.2);

    // Bass sets heat on the (hidden) bottom row, in clumps so flames form tongues.
    for (let x = 0; x < W; ) {
      const run = 3 + Math.floor(Math.random() * 6);
      const hot = Math.random() < 0.55 ? Math.min(255, 255 * level * (0.8 + Math.random() * 0.4)) : 0;
      for (let k = 0; k < run && x < W; k++, x++) heat[bottom + x] = hot;
    }
    // The downbeat on the tempo map flares the whole bed.
    if (this.barOnset(frame)) heat.fill(255, bottom, bottom + W);
    // Treble throws sparks.
    const sparks = Math.floor(frame.treble * 60);
    for (let i = 0; i < sparks; i++) {
      const x = 1 + Math.floor(Math.random() * (W - 2));
      const y = H - 2 - Math.floor(Math.random() * 25);
      heat[y * W + x] = 255;
    }

    this.coolOffset = (this.coolOffset + 1) % H;
    for (let y = 0; y < H; y++) {
      const below = (y + 1) * W;
      const below2 = Math.min(H, y + 2) * W;
      const coolRow = ((y + this.coolOffset) % H) * W;
      for (let x = 0; x < W; x++) {
        const l = x > 0 ? x - 1 : x;
        const r = x < W - 1 ? x + 1 : x;
        const v = ((heat[below + l] + heat[below + x] + heat[below + r] + heat[below2 + x]) >> 2) - cool[coolRow + x];
        heat[y * W + x] = v > 0 ? v : 0;
      }
    }

    const lut = this.palette.heat;
    const px = this.pixels;
    for (let i = 0; i < W * H; i++) px[i] = lut[heat[i]];
    this.g.putImageData(this.image!, 0, 0);
  }
}

/** Smooth noise in 0-6, tiled vertically so it can scroll forever. */
function buildCoolingMap(w: number, h: number): Uint8Array {
  const raw = new Float32Array(w * h);
  for (let i = 0; i < raw.length; i++) raw[i] = Math.random();
  const out = new Uint8Array(w * h);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let s = 0;
      for (let dy = -2; dy <= 2; dy++) {
        for (let dx = -2; dx <= 2; dx++) {
          const xx = (x + dx + w) % w;
          const yy = (y + dy + h) % h;
          s += raw[yy * w + xx];
        }
      }
      const v = s / 25;
      out[y * w + x] = Math.max(0, Math.min(6, Math.round((v - 0.44) * 32)));
    }
  }
  return out;
}
