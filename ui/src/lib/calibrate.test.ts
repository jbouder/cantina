import { describe, expect, it } from "vitest";
import { judge, profileTrim, signedDb } from "./calibrate";

describe("calibrate", () => {
  it("takes a peak from playing, refusing silence and clipping", () => {
    expect(judge(0.5)).toEqual({ ok: true, peakDb: -6 });
    expect(judge(0.0001).ok).toBe(false);
    expect(judge(1).ok).toBe(false);
  });

  it("trims to the rigs' reference peak, or by the pickups before a calibration", () => {
    const base = { instrument: "guitar" as const, tuning: [40, 45, 50, 55, 59, 64] };
    expect(profileTrim({ ...base, pickups: "humbucker", calibratedPeakDb: null })).toBe(0);
    expect(profileTrim({ ...base, pickups: "singleCoil", calibratedPeakDb: null })).toBe(4);
    expect(profileTrim({ ...base, pickups: "active", calibratedPeakDb: null })).toBe(-6);
    expect(profileTrim({ ...base, pickups: "active", calibratedPeakDb: -14.3 })).toBe(8.5);
    expect(profileTrim({ ...base, pickups: "humbucker", calibratedPeakDb: -80 })).toBe(18);
  });

  it("shows a trim signed", () => {
    expect(signedDb(3.5)).toBe("+3.5 dB");
    expect(signedDb(-6)).toBe("−6 dB");
    expect(signedDb(0)).toBe("0 dB");
  });
});
