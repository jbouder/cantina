import { useEffect, useId, useRef, useState } from "react";
import {
  type EngineStatus,
  type Level,
  SILENT,
  type Strip,
  stripGain,
  stripMute,
  stripPan,
  stripSolo,
  type TrackStatus,
} from "../lib/engine";
import { Button } from "./Button";
import { Meter } from "./Meter";

type Props = {
  tracks: TrackStatus[];
  master: Strip;
  /** Post-fader levels per track, in track order, and the master. */
  levels: Level[];
  masterLevel: Level;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

const FADER_PX = 160;

/** A strip per track and the master: gain, pan, mute, solo and a post-fader meter. */
export function Mixer({ tracks, master, levels, masterLevel, onStatus, onError }: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const anySolo = tracks.some((t) => t.strip.solo);
  return (
    <ol className="m-0 flex list-none gap-2 overflow-x-auto p-0 pb-1">
      {tracks.map((track, i) => (
        <li key={track.id}>
          <StripView
            name={track.name}
            track={track.id}
            strip={track.strip}
            silenced={track.strip.mute || (anySolo && !track.strip.solo)}
            level={levels[i] ?? SILENT}
            onGain={(db) => stripGain(track.id, db)}
            onPan={(pan) => stripPan(track.id, pan)}
            onMute={(on) => run(stripMute(track.id, on))}
            onSolo={(on) => run(stripSolo(track.id, on))}
            onError={onError}
          />
        </li>
      ))}
      <li>
        <StripView
          name="Master"
          track={null}
          strip={master}
          silenced={master.mute}
          level={masterLevel}
          onGain={(db) => stripGain(null, db)}
          onMute={(on) => run(stripMute(null, on))}
          onError={onError}
        />
      </li>
    </ol>
  );
}

type StripProps = {
  name: string;
  track: number | null;
  strip: Strip;
  /** True when the strip is silent, by its own mute or someone's solo. */
  silenced: boolean;
  level: Level;
  onGain: (db: number) => Promise<void>;
  onPan?: (pan: number) => Promise<void>;
  onMute: (on: boolean) => void;
  onSolo?: (on: boolean) => void;
  onError: (message: string) => void;
};

function StripView({
  name,
  track,
  strip,
  silenced,
  level,
  onGain,
  onPan,
  onMute,
  onSolo,
  onError,
}: StripProps) {
  const id = useId();
  // Continuous values live here between status pushes, since the
  // commands that set them are not echoed back.
  const [gain, setGain] = useState(strip.gainDb);
  const [pan, setPan] = useState(strip.pan);
  const pending = useRef<{ gain?: number; pan?: number }>({});
  const scheduled = useRef(false);

  useEffect(() => setGain(strip.gainDb), [strip.gainDb]);
  useEffect(() => setPan(strip.pan), [strip.pan]);

  // A drag sends at most one value per control per frame.
  const flush = () => {
    scheduled.current = false;
    const batch = pending.current;
    pending.current = {};
    if (batch.gain !== undefined) onGain(batch.gain).catch((e) => onError(String(e)));
    if (batch.pan !== undefined && onPan) onPan(batch.pan).catch((e) => onError(String(e)));
  };
  const queue = (patch: { gain?: number; pan?: number }) => {
    Object.assign(pending.current, patch);
    if (!scheduled.current) {
      scheduled.current = true;
      requestAnimationFrame(flush);
    }
  };

  return (
    <section
      aria-label={`${name} strip`}
      className={`surface-panel grid w-40 shrink-0 gap-2 p-2 ${silenced ? "opacity-70" : ""}`}
    >
      <h3 className="m-0 truncate text-center font-bold text-[11px] uppercase tracking-[0.1em]">
        {name}
      </h3>
      {onPan ? (
        <div className="grid gap-0.5">
          <div className="flex items-baseline justify-between">
            <label
              htmlFor={`${id}-pan`}
              className="font-bold text-[10px] uppercase tracking-[0.08em]"
            >
              Pan
            </label>
            <output
              htmlFor={`${id}-pan`}
              className="surface-lcd px-1.5 py-0.5 font-lcd text-sm leading-none tabular-nums"
            >
              {panLabel(pan)}
            </output>
          </div>
          <input
            id={`${id}-pan`}
            type="range"
            className="fader"
            min={-1}
            max={1}
            step={0.01}
            value={pan}
            aria-valuetext={panLabel(pan)}
            onChange={(e) => {
              const v = Number(e.target.value);
              setPan(v);
              queue({ pan: v });
            }}
            onDoubleClick={() => {
              setPan(0);
              queue({ pan: 0 });
            }}
          />
        </div>
      ) : null}
      <div className="grid grid-cols-[1fr_auto] gap-2" style={{ height: FADER_PX }}>
        <div className="relative h-full overflow-hidden">
          <label htmlFor={`${id}-gain`} className="sr-only">
            {name} gain
          </label>
          <input
            id={`${id}-gain`}
            type="range"
            className="fader fader-vertical"
            style={{ width: FADER_PX }}
            min={-60}
            max={12}
            step={0.5}
            value={gain}
            aria-orientation="vertical"
            aria-valuetext={`${gain.toFixed(1)} dB`}
            onChange={(e) => {
              const v = Number(e.target.value);
              setGain(v);
              queue({ gain: v });
            }}
            onDoubleClick={() => {
              setGain(0);
              queue({ gain: 0 });
            }}
          />
        </div>
        <Meter label={name} level={level} vertical />
      </div>
      <output
        htmlFor={`${id}-gain`}
        className="surface-lcd px-1 py-0.5 text-center font-lcd text-sm leading-none tabular-nums"
      >
        <span className="lcd-text">{gain.toFixed(1)} dB</span>
      </output>
      <div className="grid grid-cols-2 gap-1">
        <Button
          aria-pressed={strip.mute}
          onClick={() => onMute(!strip.mute)}
          variant={strip.mute ? "transport" : "panel"}
          className="min-w-0 px-1"
        >
          Mute
        </Button>
        {onSolo && track !== null ? (
          <Button
            aria-pressed={strip.solo}
            onClick={() => onSolo(!strip.solo)}
            variant={strip.solo ? "transport" : "panel"}
            className="min-w-0 px-1"
          >
            Solo
          </Button>
        ) : (
          <span />
        )}
      </div>
    </section>
  );
}

function panLabel(pan: number): string {
  if (Math.abs(pan) < 0.005) return "C";
  const pct = Math.round(Math.abs(pan) * 100);
  return pan < 0 ? `L${pct}` : `R${pct}`;
}
