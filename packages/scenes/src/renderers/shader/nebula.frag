// Nebula drift: a slow flight through glowing volumetric gas against a starfield.
// Bass brightens the gas and drives the drift; beats light it from within; treble twinkles the stars.

float hash3(vec3 p) {
  p = fract(p * 0.3183099 + 0.1);
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float noise3(vec3 x) {
  vec3 i = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(mix(hash3(i), hash3(i + vec3(1, 0, 0)), f.x), mix(hash3(i + vec3(0, 1, 0)), hash3(i + vec3(1, 1, 0)), f.x), f.y),
    mix(mix(hash3(i + vec3(0, 0, 1)), hash3(i + vec3(1, 0, 1)), f.x), mix(hash3(i + vec3(0, 1, 1)), hash3(i + vec3(1, 1, 1)), f.x), f.y),
    f.z);
}

float fbm(vec3 p) {
  float s = 0.0;
  float a = 0.5;
  for (int i = 0; i < 4; i++) {
    s += a * noise3(p);
    p = p * 2.03 + vec3(1.7, 9.2, 3.1);
    a *= 0.5;
  }
  return s;
}

vec3 stars(vec3 rd) {
  vec3 q = rd * 160.0;
  vec3 id = floor(q);
  float h = hash3(id);
  if (h < 0.996) return vec3(0.0);
  float d = length(fract(q) - 0.5);
  float tw = 0.6 + 0.4 * sin(uTime * (2.0 + h * 6.0) + h * 40.0);
  return mix(vec3(0.7, 0.8, 1.0), vec3(1.0, 0.85, 0.7), fract(h * 91.0)) * smoothstep(0.3, 0.0, d) * tw * (1.0 + uTreble * 3.0);
}

mat2 rot(float a) {
  float c = cos(a), s = sin(a);
  return mat2(c, -s, s, c);
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  vec3 ro = vec3(0.0, 0.0, uTravel);
  vec3 rd = normalize(vec3(uv, 1.25));
  rd.xy *= rot(uTime * 0.02);
  rd.xz *= rot(sin(uTime * 0.05) * 0.25);

  vec3 acc = vec3(0.0);
  float trans = 1.0;
  float t = 0.4;
  float glow = 0.55 + uBass * 1.8 + uBeat * uFlash * 0.9;
  for (int i = 0; i < 44; i++) {
    vec3 p = ro + rd * t;
    float d = smoothstep(0.42, 0.85, fbm(p * 0.32 + vec3(0.0, 0.0, uTime * 0.015)));
    if (d > 0.001) {
      vec3 emit = mix(vec3(0.95, 0.2, 0.55), vec3(0.15, 0.4, 1.0), noise3(p * 0.12 + 3.0));
      emit = mix(emit, vec3(1.0, 0.62, 0.25), smoothstep(0.6, 0.9, noise3(p * 0.5 + 11.0)));
      float a = d * 0.13;
      acc += trans * a * emit * glow;
      trans *= 1.0 - a;
      if (trans < 0.03) break;
    }
    t += 0.33;
  }

  vec3 col = stars(rd) * trans + acc;
  col += vec3(0.05, 0.02, 0.1) * (1.0 - length(uv));
  col = 1.0 - exp(-col * 1.5);
  fragColor = vec4(pow(col, vec3(0.85)), 1.0);
}
