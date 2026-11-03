// Warp tunnel: a raymarched ribbed tunnel with the camera flying forward.
// Bass drives speed (uTravel), beats pulse the ring radius, treble adds shimmer.
// SHADER_HEADER (uniforms, freqAt, waveAt, fragColor) is prepended by the runner.

#define PI 3.14159265

vec2 path(float z) {
  return vec2(sin(z * 0.17) * 1.1 + sin(z * 0.07) * 0.6, cos(z * 0.13) * 0.8);
}

float ridge(float z) {
  return pow(abs(sin(z * PI * 0.5)), 18.0);
}

float map(vec3 p) {
  vec2 c = p.xy - path(p.z);
  float ang = atan(c.y, c.x);
  float r = 1.5 + 0.06 * sin(ang * 6.0 + p.z * 0.8) - (0.14 + 0.28 * uBeat) * ridge(p.z);
  return (r - length(c)) * 0.75;
}

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

vec3 pal(float t) {
  return 0.5 + 0.5 * cos(6.28318 * (t + vec3(0.0, 0.33, 0.67)));
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  float t = uTravel * 2.0;
  vec3 ro = vec3(path(t), t);
  vec3 ta = vec3(path(t + 1.5), t + 1.5);
  vec3 fw = normalize(ta - ro);
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
  vec3 up = cross(fw, rt);
  vec3 rd = normalize(fw * 0.9 + uv.x * rt + uv.y * up);

  float d = 0.0;
  vec3 p = ro;
  for (int i = 0; i < 90; i++) {
    p = ro + rd * d;
    float h = map(p);
    if (h < 0.002 * d || d > 40.0) break;
    d += h;
  }

  vec2 e = vec2(0.01, 0.0);
  vec3 n = normalize(vec3(
    map(p + e.xyy) - map(p - e.xyy),
    map(p + e.yxy) - map(p - e.yxy),
    map(p + e.yyx) - map(p - e.yyx)));

  float diff = clamp(dot(n, -rd), 0.0, 1.0);
  float rg = ridge(p.z);
  vec3 base = pal(p.z * 0.025 + uTime * 0.03);
  vec3 col = base * (0.12 + 0.55 * diff * diff);
  col += base * rg * (0.5 + 1.2 * uBeat * uFlash + 0.6 * uBass);

  vec2 c = p.xy - path(p.z);
  float ang = atan(c.y, c.x);
  float cell = hash(floor(vec2(ang * 24.0, p.z * 10.0)) + floor(uTime * 20.0));
  col += vec3(0.75, 0.9, 1.0) * step(0.982, cell) * uTreble * 3.0;

  float fog = 1.0 - exp(-d * 0.075);
  col = mix(col, vec3(0.01, 0.0, 0.03), fog);
  col += vec3(0.4, 0.2, 0.9) * exp(-length(uv) * 3.0) * 0.25 * uRms;

  fragColor = vec4(pow(col, vec3(0.4545)), 1.0);
}
