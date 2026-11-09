// The instrument profile's calibration: a few seconds of playing hard,
// measured from the input meter the frame stream carries. Kept apart from
// the component so tests need no window.

import type { InstrumentProfile } from "./engine";

/** The peak the stock rigs are leveled for, in dBFS: a humbucker played hard. */
export const RIG_REFERENCE_PEAK_DB = -6;

/** The trim a profile gives rigs, as the shell works it out. */
export function profileTrim(p: InstrumentProfile): number {
  const nominal = { singleCoil: 4, humbucker: 0, active: -6 }[p.pickups];
  const trim =
    p.calibratedPeakDb !== null && Number.isFinite(p.calibratedPeakDb)
      ? RIG_REFERENCE_PEAK_DB - p.calibratedPeakDb
      : nominal;
  return Math.max(-18, Math.min(18, Math.round(trim * 2) / 2));
}

/** Linear amplitude to decibels, floored so silence reads finite. */
function toDb(linear: number): number {
  return 20 * Math.log10(Math.max(linear, 1e-5));
}

/** Seconds of playing a calibration listens to. */
export const CALIBRATE_SECONDS = 4;

/** A peak below this is no playing at all: nothing to calibrate from. */
export const SILENT_DB = -60;

/** A peak at or above this clipped the input: the reading is too low to trust. */
export const CLIPPED_DB = -0.1;

/** What a calibration found: the peak, or why it cannot be used. */
export type Calibration = { ok: true; peakDb: number } | { ok: false; reason: string };

/** Judges the loudest input peak heard, linear, over a calibration. */
export function judge(peak: number): Calibration {
  const db = Math.round(toDb(peak) * 10) / 10;
  if (db < SILENT_DB) {
    return {
      ok: false,
      reason: "Nothing came in. Check the input is your instrument and play harder.",
    };
  }
  if (db >= CLIPPED_DB) {
    return {
      ok: false,
      reason: "The input clipped. Turn the interface's gain down and calibrate again.",
    };
  }
  return { ok: true, peakDb: db };
}

/** A trim in decibels as the settings show it: signed, to half a decibel. */
export function signedDb(db: number): string {
  if (Math.abs(db) < 0.05) return "0 dB";
  return `${db > 0 ? "+" : "−"}${Math.abs(db).toFixed(1).replace(/\.0$/, "")} dB`;
}
