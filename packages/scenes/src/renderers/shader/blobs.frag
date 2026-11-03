// Liquid blobs: raymarched metaballs with glossy shading.
// Each blob follows a band (bass, mid, treble, rms); rms widens the merge distance.

float smin(float a, float b, float k) {
  float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
  return mix(b, a, h) - k * h * (1.0 - h);
}

vec3 blobPos(float i) {
  return vec3(
    sin(uTime * 0.5 * (1.0 + i * 0.27) + i * 1.7) * 1.15,
    cos(uTime * 0.43 * (1.0 + i * 0.19) + i * 2.3) * 0.8,
    sin(uTime * 0.31 + i * 1.1) * 0.6);
}

float level(int i) {
  return i == 0 ? uBass : i == 1 ? uMid : i == 2 ? uTreble * 2.0 : uRms * 2.0;
}

float map(vec3 p) {
  float k = 0.25 + uRms * 1.2;
  float d = 1e9;
  for (int i = 0; i < 4; i++) {
    float r = 0.3 + level(i) * 0.65 + 0.05 * uBeat;
    d = smin(d, length(p - blobPos(float(i))) - r, k);
  }
  return d;
}

vec3 env(vec3 dir) {
  vec3 c = mix(vec3(0.02, 0.02, 0.06), vec3(0.3, 0.5, 0.9), smoothstep(-0.3, 1.0, dir.y));
  c += vec3(1.0, 0.6, 0.3) * smoothstep(0.92, 1.0, dot(dir, normalize(vec3(0.6, 0.7, 0.4)))) * 3.0;
  c += vec3(0.9, 0.2, 0.8) * smoothstep(0.02, 0.0, abs(dir.y - 0.15)) * 0.6;
  return c;
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  vec3 ro = vec3(0.0, 0.0, 4.2);
  vec3 rd = normalize(vec3(uv, -1.6));

  vec3 bg = mix(vec3(0.03, 0.01, 0.08), vec3(0.08, 0.02, 0.15), uv.y + 0.5);
  bg += vec3(0.3, 0.1, 0.5) * exp(-length(uv) * 2.5) * (0.2 + uBass * 0.6);
  vec3 col = bg;

  float t = 0.0;
  for (int i = 0; i < 80; i++) {
    float h = map(ro + rd * t);
    if (h < 0.001) break;
    t += h;
    if (t > 10.0) break;
  }

  if (t < 10.0) {
    vec3 p = ro + rd * t;
    vec2 e = vec2(0.002, 0.0);
    vec3 n = normalize(vec3(
      map(p + e.xyy) - map(p - e.xyy),
      map(p + e.yxy) - map(p - e.yxy),
      map(p + e.yyx) - map(p - e.yyx)));
    vec3 l = normalize(vec3(0.6, 0.7, 0.4));
    float diff = max(dot(n, l), 0.0);
    float spec = pow(max(dot(reflect(rd, n), l), 0.0), 64.0);
    float fres = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    vec3 base = 0.5 + 0.5 * cos(6.28318 * (p.x * 0.15 + p.y * 0.1 + uTime * 0.05 + vec3(0.0, 0.33, 0.67)));
    col = base * (0.15 + 0.7 * diff);
    col += env(reflect(rd, n)) * (0.25 + 0.75 * fres);
    col += spec * (1.0 + uBeat * uFlash);
  }

  col *= 1.0 - 0.35 * dot(uv, uv);
  fragColor = vec4(pow(col, vec3(0.4545)), 1.0);
}
