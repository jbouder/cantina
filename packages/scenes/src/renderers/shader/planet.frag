// Ringed planet: a banded gas giant with rings whose density follows the spectrum.
// Beats brighten the rings; bass lights the atmosphere; the planet turns slowly.

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y);
}

float fbm(vec2 p) {
  float s = 0.0, a = 0.5;
  for (int i = 0; i < 5; i++) {
    s += a * noise(p);
    p = p * 2.1 + 3.7;
    a *= 0.5;
  }
  return s;
}

float sphere(vec3 ro, vec3 rd, float r) {
  float b = dot(ro, rd);
  float c = dot(ro, ro) - r * r;
  float h = b * b - c;
  return h < 0.0 ? -1.0 : -b - sqrt(h);
}

const float R = 1.6;
const float RING_IN = 2.1;
const float RING_OUT = 3.7;

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  float cam = uTime * 0.03;
  vec3 ro = vec3(sin(cam) * 1.5, 1.1, 7.0);
  vec3 ta = vec3(0.6, -0.25, 0.0);
  vec3 fw = normalize(ta - ro);
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
  vec3 up = cross(fw, rt);
  vec3 rd = normalize(fw * 1.7 + uv.x * rt + uv.y * up);

  vec3 L = normalize(vec3(-0.8, 0.35, 0.45));
  vec3 axis = normalize(vec3(0.22, 1.0, 0.1));

  // Background: stars and sun glare
  vec2 sq = rd.xy / (abs(rd.z) + 1.0) * 220.0;
  float sh = hash(floor(sq));
  vec3 col = vec3(0.004, 0.006, 0.015);
  col += step(0.994, sh) * smoothstep(0.4, 0.0, length(fract(sq) - 0.5)) * (0.5 + uTreble * 2.0);
  col += vec3(1.0, 0.8, 0.55) * pow(max(dot(rd, L), 0.0), 60.0) * 0.6;

  float tp = sphere(ro, rd, R);

  // Ring plane
  float denom = dot(rd, axis);
  float tr = abs(denom) > 1e-4 ? -dot(ro, axis) / denom : -1.0;
  vec4 ring = vec4(0.0);
  if (tr > 0.0) {
    vec3 hp = ro + rd * tr;
    float rr = length(hp);
    if (rr > RING_IN && rr < RING_OUT) {
      float u = (rr - RING_IN) / (RING_OUT - RING_IN);
      float bands = 0.45 + 0.55 * noise(vec2(u * 60.0, 0.0));
      float spec = freqAt(0.02 + u * 0.5);
      float dens = bands * (0.35 + spec * 1.2) * smoothstep(0.0, 0.04, u) * smoothstep(1.0, 0.9, u);
      dens *= 1.0 - 0.75 * smoothstep(0.55, 0.58, u) * smoothstep(0.64, 0.61, u); // a gap
      // Planet shadow on the rings, with a soft penumbra: how close the ray toward the sun passes the planet.
      float toward = -dot(hp, L);
      float miss = length(hp + L * max(toward, 0.0));
      float shadow = toward > 0.0 ? mix(0.25, 1.0, smoothstep(R * 0.85, R * 1.08, miss)) : 1.0;
      vec3 rc = mix(vec3(0.75, 0.62, 0.45), vec3(0.95, 0.88, 0.75), bands) * shadow;
      rc *= 0.6 + 0.8 * abs(dot(axis, L)) + uBeat * uFlash * 0.6;
      ring = vec4(rc, clamp(dens, 0.0, 0.95));
    }
  }

  if (tp > 0.0) {
    vec3 hp = ro + rd * tp;
    vec3 n = normalize(hp);
    float lat = dot(n, axis);
    vec3 east = normalize(cross(axis, vec3(0.0, 0.0, 1.0)));
    float lon = atan(dot(n, cross(axis, east)), dot(n, east)) + uTime * 0.04;
    float warp = fbm(vec2(lon * 2.0, lat * 6.0) + uTime * 0.01);
    float band = sin(lat * 18.0 + warp * 5.0) * 0.5 + 0.5;
    vec3 pc = mix(vec3(0.75, 0.5, 0.3), vec3(0.95, 0.85, 0.65), band);
    pc = mix(pc, vec3(0.55, 0.3, 0.2), smoothstep(0.65, 0.9, fbm(vec2(lon * 4.0, lat * 20.0))));
    float spot = smoothstep(0.18, 0.0, length(vec2((lon - 2.0) * 0.35, lat + 0.35)));
    pc = mix(pc, vec3(0.8, 0.35, 0.2), spot * 0.8);
    float diff = max(dot(n, L), 0.0);
    // Ring shadow on the planet
    float trs = -dot(hp, axis) / dot(L, axis);
    if (trs > 0.0) {
      float rr = length(hp + L * trs);
      if (rr > RING_IN && rr < RING_OUT) diff *= 0.45;
    }
    vec3 pcol = pc * (0.03 + diff);
    float rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    pcol += vec3(0.35, 0.55, 1.0) * rim * (0.25 + uBass * 0.9) * (0.3 + diff);
    col = pcol;
    if (tr > 0.0 && tr < tp) col = mix(col, ring.rgb, ring.a);
  } else {
    col = mix(col, ring.rgb, ring.a);
  }

  fragColor = vec4(pow(col, vec3(0.4545)), 1.0);
}
