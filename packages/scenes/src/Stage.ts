import type { AudioFrame } from './frame';
import { QualityGovernor, nextLowerQuality } from './quality';
import type { RenderContext, Renderer } from './Renderer';
import { getScene } from './registry';
import { Counterpart } from './counterpart';
import { RetroLook } from './retroLook';

export const CROSSFADE_MS = 400;
const GPU_MAX_PIXEL_RATIO = 1.5;
const RETRO_MAX_PIXEL_RATIO = 2;

interface Layer {
  renderer: Renderer;
  canvas: HTMLCanvasElement;
  cleanup: () => void;
  /** False while a lost WebGL context is being restored. */
  live: boolean;
}

/** Where the stage gets each frame: an engine adapter or a Web Audio one. */
export interface FrameSource {
  readFrame(): AudioFrame;
}

export interface StageEvents {
  onSceneChange?(renderer: Renderer): void;
  /** Auto quality stepped down to a new scale. */
  onQualityStep?(scale: number): void;
  onError?(message: string): void;
}

/**
 * Owns the one animation loop and the active renderer, and draws each
 * scene from the frames a {@link FrameSource} hands it. Each renderer gets its
 * own canvas (2D and WebGL contexts cannot share one); switching scenes stacks
 * the new canvas on top and crossfades opacity, then disposes the old one.
 */
export class Stage {
  readonly rc: RenderContext;
  /** The frame rendered most recently, for UI meters. Reused every frame. */
  lastFrame: AudioFrame | null = null;
  private active: Layer | null = null;
  private outgoing: Layer[] = [];
  private raf = 0;
  private lastTime = 0;
  private token = 0;
  private transitionUntil = 0;
  private autoQuality = true;
  private settleUntil = 0;
  private readonly governor = new QualityGovernor();
  private readonly resizeObserver: ResizeObserver;
  private width = 1;
  private height = 1;

  constructor(
    private readonly host: HTMLElement,
    private readonly source: FrameSource,
    rc: RenderContext,
    private readonly events: StageEvents = {},
  ) {
    this.rc = rc;
    this.resizeObserver = new ResizeObserver(() => this.measure());
    this.resizeObserver.observe(host);
    this.measure();
    document.addEventListener('visibilitychange', this.onVisibility);
  }

  get renderer(): Renderer | null {
    return this.active?.renderer ?? null;
  }

  /** True during a crossfade; auto-cycle must not fire then. */
  get transitioning(): boolean {
    return performance.now() < this.transitionUntil;
  }

  start(): void {
    if (this.raf) return;
    this.lastTime = performance.now();
    this.raf = requestAnimationFrame(this.loop);
  }

  stop(): void {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  /** Auto lets the engine step quality down on slow frames; a number fixes the scale. */
  setQuality(mode: 'auto' | number, autoScale = 1): void {
    this.autoQuality = mode === 'auto';
    const q = mode === 'auto' ? autoScale : mode;
    this.governor.reset();
    if (q !== this.rc.quality) {
      this.rc.quality = q;
      this.resizeAll();
    }
  }

  async setScene(id: string, presetId?: string): Promise<Renderer> {
    const entry = getScene(id);
    if (!entry) throw new Error(`Unknown scene: ${id}`);
    const token = ++this.token;

    // In the Retro look, GPU scenes use their classic counterpart, or else a low-res wrapper.
    let renderer: Renderer;
    if (this.rc.retroLook && entry.family === 'gpu') {
      renderer = entry.retro
        ? new Counterpart(await entry.retro(), entry.id, entry.name)
        : new RetroLook(await entry.load(), { native: entry.retroNative, pixels: entry.retroNative ? 76800 : undefined });
    } else {
      renderer = await entry.load();
    }
    const canvas = document.createElement('canvas');
    canvas.className = 'viz-layer';
    canvas.dataset.family = renderer.family;
    canvas.dataset.scene = renderer.id;
    canvas.style.opacity = '0';
    if (token !== this.token) return this.active?.renderer ?? renderer;

    this.host.appendChild(canvas);
    const layer: Layer = { renderer, canvas, cleanup: () => {}, live: true };
    try {
      await renderer.init(canvas, this.rc);
      if (renderer.family === 'gpu') layer.cleanup = this.watchContext(layer);
      this.resizeLayer(layer);
      if (presetId && renderer.setPreset) renderer.setPreset(presetId, 0);
    } catch (err) {
      canvas.remove();
      safeDispose(renderer);
      throw err;
    }
    if (token !== this.token) {
      this.disposeLayer(layer);
      return this.active?.renderer ?? renderer;
    }

    // Finish any crossfade still in flight before starting a new one.
    for (const l of this.outgoing) this.disposeLayer(l);
    this.outgoing = [];

    const previous = this.active;
    this.active = layer;
    this.transitionUntil = performance.now() + CROSSFADE_MS;
    this.settleUntil = performance.now() + 1000;
    this.governor.reset();
    requestAnimationFrame(() => {
      canvas.style.opacity = '1';
      if (previous) previous.canvas.style.opacity = '0';
    });
    if (previous) {
      this.outgoing.push(previous);
      window.setTimeout(() => {
        const i = this.outgoing.indexOf(previous);
        if (i >= 0) {
          this.outgoing.splice(i, 1);
          this.disposeLayer(previous);
        }
      }, CROSSFADE_MS + 50);
    }
    this.events.onSceneChange?.(renderer);
    return renderer;
  }

  /** Re-applies size and quality, for example after the CRT or quality setting changes. */
  resizeAll(): void {
    if (this.active) this.resizeLayer(this.active);
    for (const l of this.outgoing) this.resizeLayer(l);
  }

  destroy(): void {
    this.stop();
    this.token++;
    this.resizeObserver.disconnect();
    document.removeEventListener('visibilitychange', this.onVisibility);
    for (const l of this.outgoing) this.disposeLayer(l);
    if (this.active) this.disposeLayer(this.active);
    this.outgoing = [];
    this.active = null;
  }

  private loop = (now: number) => {
    this.raf = requestAnimationFrame(this.loop);
    const dt = Math.min(0.1, Math.max(0, (now - this.lastTime) / 1000));
    this.lastTime = now;
    if (document.hidden) return;

    const frame = this.source.readFrame();
    this.lastFrame = frame;
    for (const l of this.outgoing) if (l.live) l.renderer.render(frame, dt);
    if (this.active?.live) {
      try {
        this.active.renderer.render(frame, dt);
      } catch (err) {
        console.error(err);
        this.events.onError?.('This scene stopped working. Pick another scene.');
        this.active.live = false;
      }
    }

    if (this.autoQuality && now > this.settleUntil && this.governor.sample(dt)) {
      const lower = nextLowerQuality(this.rc.quality);
      if (lower !== null) {
        this.rc.quality = lower;
        this.resizeAll();
        this.events.onQualityStep?.(lower);
      }
    }
  };

  private onVisibility = () => {
    // Avoid one huge dt (and a false slow-frame reading) after the tab returns.
    this.lastTime = performance.now();
    this.settleUntil = performance.now() + 1000;
    this.governor.reset();
  };

  private measure(): void {
    const r = this.host.getBoundingClientRect();
    this.width = Math.max(1, Math.round(r.width));
    this.height = Math.max(1, Math.round(r.height));
    this.resizeAll();
  }

  private resizeLayer(layer: Layer): void {
    const dpr = window.devicePixelRatio || 1;
    const pr =
      layer.renderer.family === 'gpu'
        ? Math.min(dpr, GPU_MAX_PIXEL_RATIO) * this.rc.quality
        : Math.min(dpr, RETRO_MAX_PIXEL_RATIO);
    if (layer.live) layer.renderer.resize(this.width, this.height, pr);
  }

  private watchContext(layer: Layer): () => void {
    const onLost = (e: Event) => {
      e.preventDefault();
      layer.live = false;
    };
    const onRestored = async () => {
      try {
        // GPU objects died with the context, so re-run init without dispose().
        await layer.renderer.init(layer.canvas, this.rc);
        layer.live = true;
        this.resizeLayer(layer);
      } catch {
        this.events.onError?.('The graphics context was lost. Pick a scene to restart it.');
      }
    };
    layer.canvas.addEventListener('webglcontextlost', onLost);
    layer.canvas.addEventListener('webglcontextrestored', onRestored);
    return () => {
      layer.canvas.removeEventListener('webglcontextlost', onLost);
      layer.canvas.removeEventListener('webglcontextrestored', onRestored);
    };
  }

  private disposeLayer(layer: Layer): void {
    layer.cleanup();
    safeDispose(layer.renderer);
    layer.canvas.remove();
    // Release the canvas backing store promptly.
    layer.canvas.width = layer.canvas.height = 0;
  }
}

function safeDispose(r: Renderer): void {
  try {
    r.dispose();
  } catch (err) {
    console.error(err);
  }
}

/** Feature check for the GPU family. */
export function hasWebGL2(): boolean {
  try {
    const c = document.createElement('canvas');
    const gl = c.getContext('webgl2');
    gl?.getExtension('WEBGL_lose_context')?.loseContext();
    return !!gl;
  } catch {
    return false;
  }
}
