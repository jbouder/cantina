// Black hole: light bends around the event horizon, lensing a glowing accretion disk and the stars.
// Bass heats the disk, mid spins it faster, beats flare the inner edge, treble adds sparks.

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}

vec3 stars(vec3 rd) {
  vec2 q = vec2(atan(rd.z, rd.x) * 120.0, asin(clamp(rd.y, -1.0, 1.0)) * 120.0);
  vec2 id = floor(q);
  float h = hash(id);
  vec3 c = vec3(0.0);
  if (h > 0.993) c = vec3(0.85, 0.9, 1.0) * smoothstep(0.35, 0.0, length(fract(q) - 0.5)) * (0.5 + h * 0.5);
  // A faint galactic band
  c += vec3(0.12, 0.08, 0.16) * exp(-abs(rd.y + 0.2 * sin(atan(rd.z, rd.x) * 2.0)) * 6.0) * noise(q * 0.05);
  return c;
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  float orbit = uTime * 0.04;
  vec3 ro = vec3(sin(orbit) * 14.0, 1.6 + sin(uTime * 0.07) * 0.9, cos(orbit) * 14.0);
  vec3 fw = normalize(-ro);
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
  vec3 up = cross(fw, rt);
  vec3 rd = normalize(fw * 1.5 + uv.x * rt + uv.y * up);

  vec3 p = ro;
  vec3 v = rd;
  vec3 hv = cross(p, v);
  float h2 = dot(hv, hv);
  vec3 col = vec3(0.0);
  float alpha = 0.0;
  bool captured = false;

  for (int i = 0; i < 160; i++) {
    float r2 = dot(p, p);
    float r = sqrt(r2);
    if (r < 1.0) { captured = true; break; }
    float dt = clamp(0.07 * r, 0.03, 0.8);
    vec3 prev = p;
    v += -1.5 * h2 * p / pow(r2, 2.5) * dt;
    p += v * dt;

    if (prev.y * p.y < 0.0) {
      vec3 hit = mix(prev, p, prev.y / (prev.y - p.y));
      float hr = length(hit.xz);
      if (hr > 2.3 && hr < 9.5) {
        float ang = atan(hit.z, hit.x);
        float swirl = ang + uTime * (1.4 / sqrt(hr)) * (1.0 + uMid * 2.0);
        vec2 dp = vec2(hr * 2.2, swirl * 3.0);
        float n = noise(dp) * 0.6 + noise(dp * 2.7) * 0.4;
        float heat = smoothstep(9.5, 2.3, hr);
        vec3 dc = mix(vec3(1.0, 0.3, 0.08), vec3(1.0, 0.92, 0.75), heat * heat);
        dc *= (0.35 + 1.3 * n) * (0.6 + uBass * 1.6);
        // Doppler beaming: the side moving toward the camera is brighter.
        vec3 vel = normalize(vec3(-hit.z, 0.0, hit.x));
        dc *= 1.0 + 0.7 * dot(vel, -normalize(v));
        dc += vec3(1.0, 0.85, 0.6) * smoothstep(0.5, 0.0, abs(hr - 2.5)) * (0.4 + uBeat * uFlash * 1.5);
        dc += vec3(1.0) * step(0.985, hash(floor(dp * 8.0) + floor(uTime * 15.0))) * uTreble * 2.0;
        float a = smoothstep(2.3, 2.8, hr) * smoothstep(9.5, 7.0, hr) * 0.9;
        col += dc * a * (1.0 - alpha);
        alpha += a * (1.0 - alpha);
      }
    }
    if (r > 40.0) break;
  }

  if (!captured) col += stars(normalize(v)) * (1.0 - alpha);
  col = 1.0 - exp(-col * 1.3);
  fragColor = vec4(pow(col, vec3(0.9)), 1.0);
}
