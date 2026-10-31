import { useEffect, useRef, useState } from "react";
import { type Level, SILENT, toDb } from "../lib/engine";

/** The quietest level the bar shows, in dB. */
const FLOOR = -60;
/** How long the peak marker holds before it falls, in ms. */
const HOLD_MS = 900;
/** How fast the peak marker falls after the hold, in dB per second. */
const FALL_DB_PER_S = 30;

type Props = {
  label: string;
  level: Level;
  /** Stands the meter up, for a mixer strip. */
  vertical?: boolean;
};

function position(db: number): number {
  return Math.min(1, Math.max(0, (db - FLOOR) / -FLOOR));
}

/**
 * A horizontal level meter: RMS as the filled bar, the held peak as a
 * marker, and a dB readout. A native <meter> carries the value for
 * assistive tech; the drawn bar is decoration. Ballistics run here at
 * display rate; the engine only sends what it measured.
 */
export function Meter({ label, level, vertical = false }: Props) {
  const [peakDb, setPeakDb] = useState(FLOOR);
  const hold = useRef({ db: FLOOR, since: 0 });

  useEffect(() => {
    const now = performance.now();
    const incoming = toDb(level.peak);
    const h = hold.current;
    if (incoming >= h.db) {
      h.db = incoming;
      h.since = now;
    } else if (now - h.since > HOLD_MS) {
      const elapsed = (now - h.since - HOLD_MS) / 1000;
      h.db = Math.max(incoming, h.db - FALL_DB_PER_S * elapsed);
      h.since = now - HOLD_MS;
    }
    setPeakDb(h.db);
  }, [level]);

  const rmsDb = toDb(level.rms);
  const rmsPos = position(rmsDb);
  const peakPos = position(peakDb);
  const clipping = peakDb > -0.5;
  const silent = level.peak === SILENT.peak && level.rms === SILENT.rms;
  const readout = silent ? "-∞" : rmsDb.toFixed(1);

  const bar = (
    <meter
      aria-label={`${label} level`}
      min={FLOOR}
      max={0}
      value={Math.max(FLOOR, rmsDb)}
      aria-valuetext={`${readout} dB`}
      className="sr-only"
    />
  );

  if (vertical) {
    return (
      <div className="grid h-full grid-rows-[1fr_auto] justify-items-center gap-1">
        <span className="relative block h-full w-4">
          {bar}
          <span aria-hidden="true" className="surface-lcd absolute inset-0 block overflow-hidden">
            <span
              className="surface-meter-v absolute inset-0 block"
              style={{ clipPath: `inset(${(1 - rmsPos) * 100}% 0 0 0)` }}
            />
            <span
              className={`absolute right-0 left-0 block h-0.5 ${clipping ? "bg-vol-hi" : "bg-ink"}`}
              style={{ bottom: `calc(${peakPos * 100}% - 1px)` }}
            />
          </span>
        </span>
        <span
          className={`surface-lcd w-full px-1 text-center font-lcd text-sm leading-tight tabular-nums ${clipping ? "text-lcd-error" : ""}`}
        >
          <span className="lcd-text">{readout}</span>
        </span>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-[2.5rem_1fr_3.5rem] items-center gap-2">
      <span className="font-bold text-[11px] uppercase tracking-[0.1em]">{label}</span>
      <span className="relative block h-4">
        {bar}
        <span aria-hidden="true" className="surface-lcd absolute inset-0 block overflow-hidden">
          <span
            className="surface-meter absolute inset-0 block"
            style={{ clipPath: `inset(0 ${(1 - rmsPos) * 100}% 0 0)` }}
          />
          <span
            className={`absolute top-0 bottom-0 block w-0.5 ${clipping ? "bg-vol-hi" : "bg-ink"}`}
            style={{ left: `calc(${peakPos * 100}% - 1px)` }}
          />
        </span>
      </span>
      <span
        className={`surface-lcd px-1 text-right font-lcd text-base leading-tight tabular-nums ${clipping ? "text-lcd-error" : ""}`}
      >
        <span className="lcd-text">{readout}</span>
      </span>
    </div>
  );
}
