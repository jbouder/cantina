interface Target {
  fb: WebGLFramebuffer;
  tex: WebGLTexture;
}

/** A ping-pong pair of framebuffers for scenes that read their previous frame. */
export class FeedbackBuffers {
  private a: Target | null = null;
  private b: Target | null = null;
  width = 0;
  height = 0;

  constructor(private readonly gl: WebGL2RenderingContext) {}

  /** The texture holding last frame's output. */
  get read(): WebGLTexture {
    return this.a!.tex;
  }
  /** The framebuffer to draw this frame into. */
  get write(): WebGLFramebuffer {
    return this.b!.fb;
  }
  get writeTexture(): WebGLTexture {
    return this.b!.tex;
  }

  resize(width: number, height: number): void {
    if (width === this.width && height === this.height && this.a) return;
    this.dispose();
    this.width = width;
    this.height = height;
    this.a = this.create(width, height);
    this.b = this.create(width, height);
  }

  swap(): void {
    [this.a, this.b] = [this.b, this.a];
  }

  dispose(): void {
    for (const t of [this.a, this.b]) {
      if (!t) continue;
      this.gl.deleteFramebuffer(t.fb);
      this.gl.deleteTexture(t.tex);
    }
    this.a = this.b = null;
  }

  private create(w: number, h: number): Target {
    const gl = this.gl;
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, w, h);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const fb = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { fb, tex };
  }
}
