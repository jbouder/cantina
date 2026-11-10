import { useEffect, useRef, useState } from "react";
import { CALIBRATE_SECONDS, judge, signedDb } from "../lib/calibrate";
import {
  type EngineStatus,
  type Instrument,
  type InstrumentProfile,
  instrumentProfileSet,
  onMeters,
  type Pickups,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { TUNINGS } from "./TranscribeSheet";

type Props = {
  status: EngineStatus;
  onStatusChange: (status: EngineStatus) => void;
  /** Scroll the section into view as the sheet opens. */
  reveal?: boolean;
};

const PICKUPS: { value: Pickups; name: string }[] = [
  { value: "singleCoil", name: "Single coils" },
  { value: "humbucker", name: "Humbuckers" },
  { value: "active", name: "Active pickups" },
];

/** Every tuning Cantina names for an instrument, any string count. */
function tuningsFor(instrument: Instrument) {
  return Object.values(TUNINGS[instrument]).flat();
}

/**
 * The instrument profile in Settings: what you play through, so rigs suit
 * it. The instrument, pickups and tuning, and a calibration: play hard for
 * a few seconds and Cantina measures the input's peak. Rigs trim their
 * input gain from it when they are auditioned or applied.
 */
export function InstrumentSettings({ status, onStatusChange, reveal = false }: Props) {
  const section = useRef<HTMLElement>(null);
  const revealOnOpen = useRef(reveal);
  useEffect(() => {
    if (revealOnOpen.current) section.current?.scrollIntoView({ block: "start" });
  }, []);
  const { trimDb, ...profile } = status.instrumentProfile;
  const [left, setLeft] = useState(0);
  const [heard, setHeard] = useState(0);
  const [problem, setProblem] = useState<string | null>(null);
  const peak = useRef(0);
  const listening = left > 0;

  const save = (next: InstrumentProfile) =>
    instrumentProfileSet(next).then(onStatusChange, (e) => setProblem(String(e)));

  // While calibrating, the loudest input peak, and a countdown to the end.
  useEffect(() => {
    if (!listening) return;
    const off = onMeters((frame) => {
      peak.current = Math.max(peak.current, frame.input.peak);
      setHeard(peak.current);
    });
    const tick = window.setTimeout(() => setLeft(left - 1), 1000);
    return () => {
      off();
      window.clearTimeout(tick);
    };
  }, [listening, left]);

  // The countdown is over: judge what was heard.
  const done = useRef(false);
  useEffect(() => {
    if (left !== 0 || !done.current) return;
    done.current = false;
    const result = judge(peak.current);
    if (result.ok) save({ ...profile, calibratedPeakDb: result.peakDb });
    else setProblem(result.reason);
  });

  const calibrate = () => {
    peak.current = 0;
    setHeard(0);
    setProblem(null);
    done.current = true;
    setLeft(CALIBRATE_SECONDS);
  };

  const tunings = tuningsFor(profile.instrument);
  const tuning = tunings.findIndex((t) => t.notes.join() === profile.tuning.join());
  const heardDb = heard > 0 ? 20 * Math.log10(heard) : -Infinity;

  return (
    <section ref={section} aria-labelledby="instrument-heading" className="grid gap-2">
      <h3 id="instrument-heading" className="caption m-0">
        Instrument
      </h3>
      <Select
        id="profile-instrument"
        label="You play"
        value={profile.instrument}
        onChange={(e) => {
          const instrument = e.target.value as Instrument;
          const first = tuningsFor(instrument)[0];
          save({ ...profile, instrument, tuning: first?.notes ?? profile.tuning });
        }}
      >
        <option value="guitar">Guitar</option>
        <option value="bass">Bass</option>
      </Select>
      <Select
        id="profile-pickups"
        label="Pickups"
        value={profile.pickups}
        onChange={(e) => save({ ...profile, pickups: e.target.value as Pickups })}
      >
        {PICKUPS.map((p) => (
          <option key={p.value} value={p.value}>
            {p.name}
          </option>
        ))}
      </Select>
      <Select
        id="profile-tuning"
        label="Tuning"
        hint="A new tab starts in it"
        value={tuning}
        onChange={(e) => {
          const t = tunings[Number(e.target.value)];
          if (t) save({ ...profile, tuning: t.notes });
        }}
      >
        {tuning < 0 ? <option value={-1}>Your own</option> : null}
        {tunings.map((t, i) => (
          <option key={`${t.notes.length}-${t.name}`} value={i}>
            {t.notes.length} strings: {t.name}
          </option>
        ))}
      </Select>
      <div className="flex items-center gap-2">
        <Lcd aria-live="polite" className="min-w-0 flex-1 text-readout">
          {listening
            ? `Play hard… ${left} · peak ${Number.isFinite(heardDb) ? heardDb.toFixed(1) : "—"} dBFS`
            : profile.calibratedPeakDb !== null
              ? `Peak ${profile.calibratedPeakDb.toFixed(1)} dBFS · rigs ${signedDb(trimDb)}`
              : `Not calibrated · rigs ${signedDb(trimDb)} for the pickups`}
        </Lcd>
        <Button
          disabled={!status.running || listening}
          onClick={calibrate}
          title={status.running ? undefined : "Start the engine to calibrate"}
          className="shrink-0"
        >
          Calibrate
        </Button>
        {profile.calibratedPeakDb !== null ? (
          <Button
            disabled={listening}
            onClick={() => save({ ...profile, calibratedPeakDb: null })}
            className="shrink-0"
          >
            Forget
          </Button>
        ) : null}
      </div>
      <p className="m-0 text-ink-dim text-label">
        {problem ??
          (status.running
            ? `Calibrate plays nothing: press it, then play your hardest for ${CALIBRATE_SECONDS} seconds. Rigs you audition or apply from then on trim their input to suit.`
            : "Start the engine to calibrate. Until then, rigs trim their input for the pickups.")}
      </p>
    </section>
  );
}
