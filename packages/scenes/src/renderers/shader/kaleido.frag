// Kaleidoscope bloom: a feedback kaleidoscope with slow hue drift.
// Mid sets the fold count; each beat injects a bright seed at the center.
// uPrev holds last frame (ping-pong framebuffers owned by the runner).

vec2 rot(vec2 p, float a) {
  float c = cos(a), s = sin(a);
  return mat2(c, -s, s, c) * p;
}

void main() {
  vec2 res = uResolution;
  vec2 p = (gl_FragCoord.xy - 0.5 * res) / res.y;
  float r = length(p);

  float folds = floor(4.0 + uMid * 12.0);
  float seg = 6.28318 / folds;
  float a = mod(atan(p.y, p.x) + uTime * 0.07, seg);
  a = abs(a - seg * 0.5);
  vec2 k = vec2(cos(a), sin(a)) * r;

  // Feed back last frame, pulled slightly inward and rotated so it blooms outward.
  vec2 q = rot(k * 0.965, 0.012);
  vec2 prevUv = q * vec2(res.y / res.x, 1.0) + 0.5;
  vec3 prev = texture(uPrev, prevUv).rgb;
  prev = mix(prev, prev.gbr, 0.015) * 0.94;

  vec3 hue = 0.5 + 0.5 * cos(6.28318 * (uTime * 0.025 + r * 0.9 + vec3(0.0, 0.33, 0.67)));
  float wv = waveAt(a / seg);
  float ringR = 0.18 + 0.12 * wv + 0.15 * uBass;
  float ring = smoothstep(0.014, 0.0, abs(r - ringR));
  vec3 col = prev + hue * ring * (0.5 + 1.2 * uRms);

  float seed = smoothstep(0.12 + 0.08 * uBeat, 0.0, r) * uBeat * uFlash;
  col += hue.zxy * seed * 1.6;
  col += hue * smoothstep(0.02, 0.0, abs(k.y - 0.01)) * uTreble * 0.4 * step(r, 0.6);

  fragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}
