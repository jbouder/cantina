/*
 * Draws every scene from a recorded sequence of engine frames and reports
 * what each drew, for scripts/scene-harness.mjs to check. Runs in a real
 * browser so the WebGL and Canvas 2D paths are the ones the app uses.
 */
import { analysisOf, decodeFrame } from '../../../ui/src/lib/frame';
import { EngineFrames } from '../src/sources/engine';
import { SCENES } from '../src/registry';
import type { RenderContext } from '../src/Renderer';

const W = 320;
const H = 180;
const THUMB_W = 160;
const THUMB_H = 90;

interface Result {
  id: string;
  ok: boolean;
  error?: string;
  /** Share of pixels away from the darkest color, after the sequence. */
  lit: number;
  /** Mean absolute pixel change between the middle and the end of the sequence. */
  motion: number;
  thumbnail: string;
}

async function recording(): Promise<ArrayBuffer[]> {
  const bytes = await (await fetch(new URL('./recording.bin', import.meta.url))).arrayBuffer();
  const view = new DataView(bytes);
  const frames: ArrayBuffer[] = [];
  for (let at = 0; at < bytes.byteLength; ) {
    const len = view.getUint32(at, true);
    frames.push(bytes.slice(at + 4, at + 4 + len));
    at += 4 + len;
  }
  return frames;
}

function snapshot(canvas: HTMLCanvasElement, w = W, h = H): ImageData {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  const g = c.getContext('2d')!;
  g.drawImage(canvas, 0, 0, w, h);
  return g.getImageData(0, 0, w, h);
}

function lit(image: ImageData): number {
  const d = image.data;
  const sum = (i: number) => (d[i] ?? 0) + (d[i + 1] ?? 0) + (d[i + 2] ?? 0);
  let min = 765;
  for (let i = 0; i < d.length; i += 4) min = Math.min(min, sum(i));
  let count = 0;
  for (let i = 0; i < d.length; i += 4) if (sum(i) > min + 24) count++;
  return count / (d.length / 4);
}

function difference(a: ImageData, b: ImageData): number {
  let sum = 0;
  for (let i = 0; i < a.data.length; i++) sum += Math.abs((a.data[i] ?? 0) - (b.data[i] ?? 0));
  return sum / a.data.length / 255;
}

async function run(): Promise<Result[]> {
  const frames = (await recording()).map((b) => analysisOf(decodeFrame(b))!);
  const host = document.getElementById('host')!;
  const results: Result[] = [];
  for (const entry of SCENES) {
    const canvas = document.createElement('canvas');
    canvas.style.cssText = 'position:absolute;inset:0;width:100%;height:100%';
    // WebGL scenes are read back after drawing, so keep the buffer.
    const getContext = canvas.getContext.bind(canvas) as (id: string, o?: object) => RenderingContext | null;
    canvas.getContext = ((id: string, o?: object) =>
      getContext(id, { ...o, preserveDrawingBuffer: true })) as typeof canvas.getContext;
    host.appendChild(canvas);
    const rc: RenderContext = {
      sampleRate: 48_000,
      quality: 1,
      reduceFlashing: false,
      retroLook: false,
      palette: 'auto',
      options: {},
    };
    const source = new EngineFrames();
    const result: Result = { id: entry.id, ok: false, lit: 0, motion: 0, thumbnail: '' };
    try {
      const scene = await entry.load();
      await scene.init(canvas, rc);
      scene.resize(W, H, 1);
      let middle: ImageData | null = null;
      for (const [i, frame] of frames.entries()) {
        source.push(frame);
        scene.render(source.readFrame(), 1 / 60);
        if (i === Math.floor(frames.length / 2)) middle = snapshot(canvas);
      }
      const end = snapshot(canvas);
      result.lit = lit(end);
      result.motion = middle ? difference(middle, end) : 0;
      const thumb = document.createElement('canvas');
      thumb.width = THUMB_W;
      thumb.height = THUMB_H;
      thumb.getContext('2d')!.drawImage(canvas, 0, 0, THUMB_W, THUMB_H);
      result.thumbnail = thumb.toDataURL('image/png');
      result.ok = true;
      scene.dispose();
    } catch (e) {
      result.error = String(e instanceof Error ? (e.stack ?? e.message) : e);
    }
    canvas.remove();
    results.push(result);
  }
  return results;
}

run().then(
  (results) => {
    document.getElementById('results')!.textContent = `RESULTS ${JSON.stringify(results)} END`;
  },
  (e) => {
    document.getElementById('results')!.textContent = `FAILED ${String(e)} END`;
  },
);
