// Hyperspace: stars stretch into streaks as the ship jumps to light speed.
// Bass sets the speed and streak length, beats surge forward, treble tints the streaks.

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

void main() {
  vec2 uv = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
  float r = length(uv);
  float a = atan(uv.y, uv.x) / 6.28318 + 0.5;
  vec3 col = vec3(0.0);

  for (int l = 0; l < 4; l++) {
    float fl = float(l);
    float cells = 70.0 + fl * 45.0;
    float ac = a * cells + fl * 0.37;
    float id = floor(ac);
    float h = hash(vec2(id, fl));
    if (h < 0.25) continue;
    // Depth cycles toward the camera; projected radius grows as depth shrinks.
    float z = fract(h * 7.31 - uTravel * (0.25 + 0.08 * fl));
    float sr = 0.04 / (z + 0.03);
    float len = (0.03 + uBass * 0.5 + uBeat * 0.35) * sr;
    float across = abs(fract(ac) - 0.5) / cells * 6.28318 * r;
    float along = smoothstep(sr - len, sr, r) * smoothstep(sr + 0.01, sr, r);
    float w = smoothstep(0.0025 + 0.0015 * fl, 0.0, across);
    vec3 tint = mix(vec3(0.55, 0.75, 1.0), vec3(0.95, 0.6, 1.0), fract(h * 13.0) * uTreble * 2.0);
    col += tint * along * w * (1.0 - z) * 1.6;
  }

  col += vec3(0.35, 0.55, 1.0) * exp(-r * 5.0) * (0.25 + uBass * 1.2);
  col += vec3(0.6, 0.8, 1.0) * uBeat * uFlash * 0.12 * smoothstep(1.2, 0.0, r);
  col *= smoothstep(1.4, 0.3, r) * 0.6 + 0.4;
  fragColor = vec4(1.0 - exp(-col * 1.6), 1.0);
}
