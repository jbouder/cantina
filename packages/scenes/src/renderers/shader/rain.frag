// Digital rain: columns of glyphs streaming down a green screen.
// Bass drives the fall speed, each column's brightness and trail follow its slice of the spectrum,
// beats flare the leading glyphs, and treble makes glyphs flicker faster.

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

/** A procedural 5 x 7 glyph inside one cell. */
float glyph(vec2 uv, float seed) {
  vec2 p = (uv - vec2(0.18, 0.12)) / vec2(0.64, 0.76);
  if (p.x < 0.0 || p.y < 0.0 || p.x >= 1.0 || p.y >= 1.0) return 0.0;
  vec2 g = floor(p * vec2(5.0, 7.0));
  // About half the glyphs are mirror-symmetric, which reads as more "character-like".
  if (hash(vec2(seed, 7.0)) > 0.5) g.x = min(g.x, 4.0 - g.x);
  float on = step(0.52, hash(vec2(seed * 31.7 + g.x, g.y + seed * 3.1)));
  vec2 f = fract(p * vec2(5.0, 7.0));
  float dot = smoothstep(0.5, 0.35, max(abs(f.x - 0.5), abs(f.y - 0.5)));
  return on * dot;
}

void main() {
  float cellPx = max(9.0, floor(uResolution.y / 38.0));
  vec2 cell = floor(gl_FragCoord.xy / cellPx);
  vec2 inCell = fract(gl_FragCoord.xy / cellPx);
  float cols = floor(uResolution.x / cellPx);
  float rows = uResolution.y / cellPx;
  float row = rows - cell.y; // 0 at the top

  float level = freqAt(clamp(0.02 + (cell.x / max(cols, 1.0)) * 0.55, 0.0, 1.0));
  float bright = 0.0;
  float head = 0.0;
  for (int k = 0; k < 2; k++) {
    float fk = float(k);
    float r = hash(vec2(cell.x, 11.0 + fk));
    float period = rows + 12.0 + r * 40.0;
    float pos = mod(uTravel * (9.0 + r * 11.0) + r * 211.0 + fk * period * 0.5, period);
    float d = pos - row; // >0: behind (above) the leading glyph
    float trail = 6.0 + r * 14.0 + level * 22.0;
    if (d >= 0.0 && d < trail) {
      bright = max(bright, pow(1.0 - d / trail, 1.6));
      if (d < 1.0) head = 1.0;
    }
  }

  float rate = 0.4 + uTreble * 8.0 + head * 6.0;
  float seed = hash(cell + floor(uTime * rate + hash(cell) * 10.0));
  float g = glyph(inCell, seed);

  float colBoost = 0.55 + level * 1.4;
  vec3 green = vec3(0.12, 1.0, 0.35);
  vec3 col = green * g * bright * colBoost;
  col += vec3(0.75, 1.0, 0.8) * g * head * (0.9 + uBeat * uFlash * 0.8);
  // Soft phosphor halo and a faint screen tint
  col += green * bright * 0.06 * colBoost;
  col += vec3(0.0, 0.035, 0.012);
  fragColor = vec4(1.0 - exp(-col * 1.6), 1.0);
}
