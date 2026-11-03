import { describe, expect, it } from "vitest";
import { createSkinBeat } from "./beat";

function target() {
  const props = new Map<string, string>();
  return {
    props,
    setProperty: (name: string, value: string) => props.set(name, value),
    removeProperty: (name: string) => props.delete(name),
  };
}

const quiet = { spectrum: new Float32Array(128), waveform: new Float32Array(2048), rms: 0 };
const at = (beat: number, phase: number) => ({
  ...quiet,
  beat: { bpm: 120, bar: 0, beat, phase },
});

describe("skin beat", () => {
  it("follows the tempo map's phase: full on the beat, gone well before the next", () => {
    const t = target();
    const beat = createSkinBeat(t, () => true);
    beat.push(at(0, 0));
    expect(t.props.get("--beat")).toBe("1.000");
    // At 120 BPM phase 0.3 is 150 ms after the beat: half the pulse left.
    beat.push(at(0, 0.3));
    expect(t.props.get("--beat")).toBe("0.500");
    beat.push(at(0, 0.9));
    expect(t.props.get("--beat")).toBe("0.000");
    beat.push(at(1, 0.02));
    expect(Number(t.props.get("--beat"))).toBeGreaterThan(0.9);
  });

  it("leaves the property alone for a skin without a pulse, or with motion reduced", () => {
    const t = target();
    let active = false;
    const beat = createSkinBeat(t, () => active);
    beat.push(at(0, 0));
    expect(t.props.has("--beat")).toBe(false);
    active = true;
    beat.push(at(1, 0));
    expect(t.props.get("--beat")).toBe("1.000");
    // Turning it off mid-pulse removes the inline value, so the skin's own 0 holds.
    active = false;
    beat.push(at(1, 0.1));
    expect(t.props.has("--beat")).toBe(false);
  });

  it("falls back to onsets heard in the input with the transport stopped", () => {
    const t = target();
    const beat = createSkinBeat(t, () => true);
    const bass = (amp: number) => {
      const spectrum = new Float32Array(128);
      spectrum.fill(amp, 10, 30);
      return { ...quiet, spectrum, beat: null };
    };
    for (let i = 0; i < 20; i++) beat.push(bass(0.01));
    expect(t.props.get("--beat")).toBe("0.000");
    beat.push(bass(1));
    expect(Number(t.props.get("--beat"))).toBeGreaterThan(0.5);
  });
});
