import type { AudioFrame } from './frame';
import type { RenderContext, Renderer } from './Renderer';

/**
 * A GPU scene's classic counterpart, presented under the GPU scene's id and
 * name so links, favorites and the scene label don't change with the look.
 */
export class Counterpart implements Renderer {
  readonly family = 'retro';

  constructor(
    readonly inner: Renderer,
    readonly id: string,
    readonly name: string,
  ) {}

  get options() {
    return this.inner.options;
  }
  init(canvas: HTMLCanvasElement, rc: RenderContext) {
    return this.inner.init(canvas, rc);
  }
  resize(width: number, height: number, pixelRatio: number): void {
    this.inner.resize(width, height, pixelRatio);
  }
  render(frame: AudioFrame, dt: number): void {
    this.inner.render(frame, dt);
  }
  dispose(): void {
    this.inner.dispose();
  }
}
