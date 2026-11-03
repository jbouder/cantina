// Spectrum grid: a synthwave landscape of neon wireframe terrain under a low sun.
// Terrain height follows the spectrum; beats pulse the sun.

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}

float terrain(vec2 xz) {
  float ax = abs(xz.x);
  float side = smoothstep(1.4, 4.5, ax);
  float band = freqAt(clamp(0.02 + ax / 20.0 * 0.4, 0.0, 1.0));
  float n = noise(xz * vec2(0.35, 0.25)) * 0.65 + noise(xz * 0.9) * 0.35;
  return side * (0.25 + band * band * 4.0) * n;
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  const float horizon = 0.08;
  vec3 ro = vec3(0.0, 1.1, 0.0);
  vec3 rd = normalize(vec3(uv.x, uv.y - horizon, 1.1));

  // Sky
  vec3 col = mix(vec3(0.95, 0.3, 0.45), vec3(0.12, 0.02, 0.22), smoothstep(horizon, horizon + 0.35, uv.y));
  col = mix(col, vec3(0.01, 0.0, 0.05), smoothstep(horizon + 0.25, horizon + 0.8, uv.y));
  vec2 sg = floor(uv * 90.0);
  float star = step(0.994, hash(sg)) * smoothstep(horizon + 0.15, horizon + 0.5, uv.y);
  col += star * (0.5 + 0.5 * sin(uTime * 3.0 + hash(sg + 3.0) * 6.28));

  // Sun with scan stripes cut through its lower half
  vec2 sc = vec2(0.0, horizon + 0.16);
  float sr = 0.27 * (1.0 + 0.07 * uBeat);
  float sd = length(uv - sc);
  vec3 sun = mix(vec3(1.0, 0.15, 0.55), vec3(1.0, 0.85, 0.3), smoothstep(sc.y - sr, sc.y + sr, uv.y));
  float k = clamp((sc.y - uv.y) / sr, 0.0, 1.0);
  float stripe = uv.y < sc.y ? step(k * 0.75, fract((uv.y - sc.y) * 22.0 + uTime * 0.4)) : 1.0;
  float disc = smoothstep(sr, sr - 0.004, sd) * stripe;
  col = mix(col, sun * (1.0 + 0.5 * uBeat * uFlash), disc);
  col += vec3(1.0, 0.3, 0.6) * exp(-sd * 5.0) * (0.35 + 0.4 * uBeat * uFlash);

  // Terrain
  if (rd.y < 0.05) {
    float t = 0.0;
    bool hit = false;
    vec3 p = ro;
    for (int i = 0; i < 140; i++) {
      p = ro + rd * t;
      float h = p.y - terrain(vec2(p.x, p.z + uTravel));
      if (h < 0.002 * t) { hit = true; break; }
      t += max(0.03, h * 0.4);
      if (t > 70.0) break;
    }
    if (hit) {
      vec2 w = vec2(p.x, p.z + uTravel) * 0.5;
      vec2 g = abs(fract(w) - 0.5);
      float lw = t * 1.4 / uResolution.y + 0.012;
      float line = 1.0 - smoothstep(lw * 0.5, lw * 1.5, min(g.x, g.y));
      vec3 neon = mix(vec3(1.0, 0.1, 0.8), vec3(0.1, 0.9, 1.0), clamp(p.y / 2.0, 0.0, 1.0));
      vec3 ground = vec3(0.04, 0.0, 0.08) + neon * line * (1.2 + 0.6 * uBass);
      float fog = 1.0 - exp(-t * 0.045);
      col = mix(ground, vec3(0.6, 0.15, 0.4), fog);
    }
  }

  fragColor = vec4(col, 1.0);
}
