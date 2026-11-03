import type { AudioFrame } from '../frame';
import type { RenderContext, Renderer } from '../Renderer';
import { FeedbackBuffers } from './feedback';
import { FULLSCREEN_VERT, compileProgram } from './program';
import { createDataTexture, uploadDataTexture } from './textures';

export interface ShaderSceneConfig {
  id: string;
  name: string;
  /** Fragment body; SHADER_HEADER is prepended. */
  frag: string;
  /** Read last frame through uPrev (ping-pong framebuffers). */
  feedback?: boolean;
  /** Speed for the uTravel accumulator, from smoothed bands. Default 1. */
  travelSpeed?: (b: Bands) => number;
}

export interface Bands {
  bass: number;
  mid: number;
  treble: number;
  rms: number;
}

/** Uniforms and helpers every scene receives. */
export const SHADER_HEADER = `#version 300 es
precision highp float;
uniform float uTime;
uniform vec2 uResolution;
uniform float uBass, uMid, uTreble, uRms;
uniform float uBeat;
uniform float uFlash;
uniform float uTravel;
uniform sampler2D uFreq;
uniform sampler2D uWave;
uniform sampler2D uPrev;
out vec4 fragColor;
float freqAt(float x) { return texture(uFreq, vec2(x, 0.5)).r; }
float waveAt(float x) { return texture(uWave, vec2(x, 0.5)).r * 2.0 - 1.0; }
#line 1
`;

const COPY_FRAG = `#version 300 es
precision highp float;
uniform sampler2D uSrc;
uniform vec2 uResolution;
out vec4 fragColor;
void main() { fragColor = vec4(texture(uSrc, gl_FragCoord.xy / uResolution).rgb, 1.0); }`;

const UNIFORMS = ['uTime', 'uResolution', 'uBass', 'uMid', 'uTreble', 'uRms', 'uBeat', 'uFlash', 'uTravel', 'uFreq', 'uWave', 'uPrev'] as const;
type UniformName = (typeof UNIFORMS)[number];

/**
 * Draws one full-screen fragment shader per frame. A WebGPU runner can implement
 * the same Renderer contract later without the scenes' configs changing shape.
 */
export class ShaderScene implements Renderer {
  readonly family = 'gpu';
  readonly id: string;
  readonly name: string;

  private gl: WebGL2RenderingContext | null = null;
  private rc: RenderContext | null = null;
  private prog: WebGLProgram | null = null;
  private copy: WebGLProgram | null = null;
  private copySrc: WebGLUniformLocation | null = null;
  private copyRes: WebGLUniformLocation | null = null;
  private vao: WebGLVertexArrayObject | null = null;
  private freqTex: WebGLTexture | null = null;
  private waveTex: WebGLTexture | null = null;
  private fb: FeedbackBuffers | null = null;
  private loc = {} as Record<UniformName, WebGLUniformLocation | null>;
  private time = 0;
  private travel = 0;
  private readonly bands: Bands = { bass: 0, mid: 0, treble: 0, rms: 0 };

  constructor(private readonly config: ShaderSceneConfig) {
    this.id = config.id;
    this.name = config.name;
  }

  init(canvas: HTMLCanvasElement, rc: RenderContext): void {
    const gl = canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      premultipliedAlpha: false,
      powerPreference: 'high-performance',
    });
    if (!gl) throw new Error('WebGL 2 is not available.');
    this.gl = gl;
    this.rc = rc;
    this.prog = compileProgram(gl, FULLSCREEN_VERT, SHADER_HEADER + this.config.frag);
    for (const u of UNIFORMS) this.loc[u] = gl.getUniformLocation(this.prog, u);
    this.vao = gl.createVertexArray();
    this.freqTex = createDataTexture(gl, 1024);
    this.waveTex = createDataTexture(gl, 2048);
    if (this.config.feedback) {
      this.fb = new FeedbackBuffers(gl);
      this.copy = compileProgram(gl, FULLSCREEN_VERT, COPY_FRAG);
      this.copySrc = gl.getUniformLocation(this.copy, 'uSrc');
      this.copyRes = gl.getUniformLocation(this.copy, 'uResolution');
      this.fb.resize(Math.max(1, canvas.width), Math.max(1, canvas.height));
    }
  }

  resize(width: number, height: number, pixelRatio: number): void {
    const gl = this.gl;
    if (!gl) return;
    const c = gl.canvas as HTMLCanvasElement;
    c.width = Math.max(1, Math.round(width * pixelRatio));
    c.height = Math.max(1, Math.round(height * pixelRatio));
    this.fb?.resize(c.width, c.height);
  }

  render(frame: AudioFrame, dt: number): void {
    const gl = this.gl;
    if (!gl || !this.prog || gl.isContextLost()) return;
    this.smooth(frame, dt);
    this.time += dt;
    this.travel += dt * (this.config.travelSpeed?.(this.bands) ?? 1);

    uploadDataTexture(gl, this.freqTex!, frame.freq);
    uploadDataTexture(gl, this.waveTex!, frame.wave);

    const w = gl.drawingBufferWidth;
    const h = gl.drawingBufferHeight;
    const flash = this.rc?.reduceFlashing ? 0.25 : 1;

    gl.useProgram(this.prog);
    gl.bindVertexArray(this.vao);
    const L = this.loc;
    gl.uniform1f(L.uTime, this.time);
    gl.uniform2f(L.uResolution, w, h);
    gl.uniform1f(L.uBass, this.bands.bass);
    gl.uniform1f(L.uMid, this.bands.mid);
    gl.uniform1f(L.uTreble, this.bands.treble);
    gl.uniform1f(L.uRms, this.bands.rms);
    gl.uniform1f(L.uBeat, frame.beat);
    gl.uniform1f(L.uFlash, flash);
    gl.uniform1f(L.uTravel, this.travel);
    this.bindTex(0, this.freqTex!, L.uFreq);
    this.bindTex(1, this.waveTex!, L.uWave);

    if (this.fb) {
      this.bindTex(2, this.fb.read, L.uPrev);
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.fb.write);
      gl.viewport(0, 0, this.fb.width, this.fb.height);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, w, h);
      gl.useProgram(this.copy);
      gl.uniform2f(this.copyRes, w, h);
      this.bindTex(0, this.fb.writeTexture, this.copySrc);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      this.fb.swap();
    } else {
      gl.viewport(0, 0, w, h);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }
  }

  dispose(): void {
    const gl = this.gl;
    if (!gl) return;
    this.fb?.dispose();
    gl.deleteTexture(this.freqTex);
    gl.deleteTexture(this.waveTex);
    gl.deleteVertexArray(this.vao);
    gl.deleteProgram(this.prog);
    gl.deleteProgram(this.copy);
    this.fb = null;
    this.prog = this.copy = null;
    this.freqTex = this.waveTex = null;
    this.vao = null;
    this.gl = null;
  }

  private bindTex(unit: number, tex: WebGLTexture, loc: WebGLUniformLocation | null): void {
    const gl = this.gl!;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(loc, unit);
  }

  /** Fast attack, slower release, so levels feel punchy without jitter. */
  private smooth(f: AudioFrame, dt: number): void {
    const up = 1 - Math.exp(-dt * 25);
    const down = 1 - Math.exp(-dt * 6);
    const b = this.bands;
    const step = (cur: number, v: number) => cur + (v - cur) * (v > cur ? up : down);
    b.bass = step(b.bass, f.bass);
    b.mid = step(b.mid, f.mid);
    b.treble = step(b.treble, f.treble);
    b.rms = step(b.rms, f.rms);
  }
}
