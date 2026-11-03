/**
 * A packed-pixel framebuffer for feedback effects in the style of classic
 * player visualization studios: each step moves, blurs and fades the last
 * frame, then draws new scopes and dots on top.
 * Pixels are little-endian ABGR, as ImageData stores them.
 */
export class PixelBuffer {
  px: Uint32Array;
  private img: ImageData;
  private backImg: ImageData;
  private back: Uint32Array;
  private readonly fadeLut = new Uint8Array(256);
  private fadeFactor = -1;

  constructor(
    g: CanvasRenderingContext2D,
    readonly w: number,
    readonly h: number,
  ) {
    this.img = g.createImageData(w, h);
    this.backImg = g.createImageData(w, h);
    this.px = new Uint32Array(this.img.data.buffer);
    this.back = new Uint32Array(this.backImg.data.buffer);
    this.clear();
  }

  clear(color = 0xff000000): void {
    this.px.fill(color);
  }

  /** Multiplies every channel by f (0-1). */
  fade(f: number): void {
    if (f !== this.fadeFactor) {
      for (let i = 0; i < 256; i++) this.fadeLut[i] = Math.floor(i * f);
      this.fadeFactor = f;
    }
    const lut = this.fadeLut;
    const px = this.px;
    for (let i = 0; i < px.length; i++) {
      const c = px[i];
      px[i] = 0xff000000 | (lut[(c >>> 16) & 255] << 16) | (lut[(c >>> 8) & 255] << 8) | lut[c & 255];
    }
  }

  /** Light blur: each pixel averaged with the mean of its four neighbours. */
  blur(): void {
    const { w, h, px, back } = this;
    const avg = (a: number, b: number) => ((a & 0xfefefe) >>> 1) + ((b & 0xfefefe) >>> 1);
    back.set(px);
    for (let y = 1; y < h - 1; y++) {
      let i = y * w + 1;
      for (let x = 1; x < w - 1; x++, i++) {
        const n = avg(avg(px[i - 1], px[i + 1]), avg(px[i - w], px[i + w]));
        back[i] = 0xff000000 | avg(px[i], n);
      }
    }
    this.swap();
  }

  /**
   * Zoom and rotate the frame about its center (plus an optional shift).
   * zoom > 1 pushes the image outward, so trails stream away from the center.
   */
  move(zoom: number, rotate: number, shiftX = 0, shiftY = 0): void {
    const { w, h, px, back } = this;
    const cx = w / 2;
    const cy = h / 2;
    const cos = Math.cos(rotate) / zoom;
    const sin = Math.sin(rotate) / zoom;
    let i = 0;
    for (let y = 0; y < h; y++) {
      const ry = y - cy;
      let sx = cx + shiftX - cx * cos + ry * sin;
      let sy = cy + shiftY - cx * -sin + ry * cos;
      for (let x = 0; x < w; x++, i++) {
        back[i] = sx >= 0 && sy >= 0 && sx < w && sy < h ? px[(sy | 0) * w + (sx | 0)] : 0xff000000;
        sx += cos;
        sy -= sin;
      }
    }
    this.swap();
  }

  /** Rebuilds the frame from a source-index table: out[i] = in[map[i]]. */
  remap(map: Int32Array): void {
    const { px, back } = this;
    for (let i = 0; i < map.length; i++) back[i] = px[map[i]];
    this.swap();
  }

  dot(x: number, y: number, c: number, size = 1): void {
    const xi = x | 0;
    const yi = y | 0;
    for (let dy = 0; dy < size; dy++) {
      for (let dx = 0; dx < size; dx++) {
        const xx = xi + dx;
        const yy = yi + dy;
        if (xx >= 0 && yy >= 0 && xx < this.w && yy < this.h) this.px[yy * this.w + xx] = c;
      }
    }
  }

  line(x0: number, y0: number, x1: number, y1: number, c: number): void {
    const dx = x1 - x0;
    const dy = y1 - y0;
    const steps = Math.min(2000, Math.max(Math.abs(dx), Math.abs(dy), 1));
    const sx = dx / steps;
    const sy = dy / steps;
    for (let s = 0; s <= steps; s++) {
      const x = (x0 + sx * s) | 0;
      const y = (y0 + sy * s) | 0;
      if (x >= 0 && y >= 0 && x < this.w && y < this.h) this.px[y * this.w + x] = c;
    }
  }

  present(g: CanvasRenderingContext2D): void {
    g.putImageData(this.img, 0, 0);
  }

  private swap(): void {
    [this.px, this.back] = [this.back, this.px];
    [this.img, this.backImg] = [this.backImg, this.img];
  }
}

/** Reads the waveform at a 0-1 position as -1..1. */
export function waveAt(wave: Uint8Array, t: number): number {
  return (wave[Math.min(wave.length - 1, Math.max(0, (t * (wave.length - 1)) | 0))] - 128) / 128;
}
