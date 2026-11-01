import { useEffect, useId, useRef, useState } from "react";
import {
  type EngineStatus,
  type Level,
  type Strip,
  stripGain,
  stripMute,
  stripPan,
  stripSolo,
  type TrackStatus,
  trackArm,
} from "../lib/engine";
import { TRACK_HEIGHT } from "../lib/timeline";
import { Led } from "./Led";
import { Meter } from "./Meter";

/** A small square key; pressed, it takes the transport material so it reads from across the room. */
const key = (on: boolean) =>
  `${on ? "surface-button" : "surface-pbutton"} grid size-7 shrink-0 place-items-center border-0 p-0 font-bold text-[11px] uppercase leading-none`;

/** A speaker with a cross: the mute key's glyph. */
function MuteGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
      <path d="M1.5 5.5h3L9 2v12L4.5 10.5h-3z" />
      <path
        d="M10.75 5.75l4 4m0-4l-4 4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** Headphones: the solo key's glyph. */
function SoloGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
      <path d="M8 2a6 6 0 0 0-6 6v4.5A1.5 1.5 0 0 0 3.5 14H5V8.5H3.5V8a4.5 4.5 0 0 1 9 0v.5H11V14h1.5a1.5 1.5 0 0 0 1.5-1.5V8a6 6 0 0 0-6-6z" />
    </svg>
  );
}

type Props = {
  track: TrackStatus;
  selected: boolean;
  /** True when the strip is silent, by its own mute or someone's solo. */
  silenced: boolean;
  /** The track's post-fader level from the latest frame. */
  level: Level;
  onSelect: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * A track's header beside its lane: the name (click to select), arm or
 * the kit mark, mute and solo, then the strip's gain and pan with a
 * thin post-fader meter under them.
 */
export function TrackHeader({
  track,
  selected,
  silenced,
  level,
  onSelect,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  return (
    <div
      style={{ height: TRACK_HEIGHT }}
      className={`grid content-center gap-1 border-tl-grid border-b px-1 ${selected ? "bg-tl-lane" : ""} ${silenced ? "opacity-70" : ""}`}
    >
      <div className="flex items-center gap-1">
        <button
          type="button"
          aria-pressed={selected}
          onClick={onSelect}
          className="min-h-7 min-w-0 flex-1 truncate border-0 bg-transparent px-1 text-left font-bold text-[11px] text-lcd-fg uppercase tracking-[0.08em]"
        >
          {track.name}
        </button>
        {track.kit ? (
          <span
            role="img"
            aria-label="Drum track"
            title={track.kit.name}
            className="grid size-7 shrink-0 place-items-center text-lcd-fg"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
              <path d="M8 2c3.3 0 6 1.1 6 2.5v6C14 12.9 11.3 14 8 14s-6-1.1-6-3.5v-6C2 3.1 4.7 2 8 2zm0 1.5c-2.8 0-4.5.8-4.5 1s1.7 1 4.5 1 4.5-.8 4.5-1-1.7-1-4.5-1z" />
            </svg>
          </span>
        ) : (
          <button
            type="button"
            aria-pressed={track.armed}
            aria-label={`Arm ${track.name}`}
            title="Arm for recording"
            onClick={() => run(trackArm(track.id, !track.armed))}
            className="grid size-7 shrink-0 place-items-center border-0 bg-transparent p-0"
          >
            <Led on={track.recording || track.armed} tone="record" label="Armed" />
          </button>
        )}
        <button
          type="button"
          aria-pressed={track.strip.mute}
          aria-label={`${track.strip.mute ? "Unmute" : "Mute"} ${track.name}`}
          title={`${track.strip.mute ? "Unmute" : "Mute"} ${track.name}`}
          onClick={() => run(stripMute(track.id, !track.strip.mute))}
          className={key(track.strip.mute)}
        >
          <MuteGlyph />
        </button>
        <button
          type="button"
          aria-pressed={track.strip.solo}
          aria-label={`${track.strip.solo ? "Unsolo" : "Solo"} ${track.name}`}
          title={
            track.strip.solo
              ? `Unsolo ${track.name}`
              : `Solo ${track.name}: hear only the soloed tracks`
          }
          onClick={() => run(stripSolo(track.id, !track.strip.solo))}
          className={key(track.strip.solo)}
        >
          <SoloGlyph />
        </button>
      </div>
      <StripControls
        name={track.name}
        strip={track.strip}
        level={level}
        onGain={(db) => stripGain(track.id, db)}
        onPan={(pan) => stripPan(track.id, pan)}
        onError={onError}
      />
    </div>
  );
}

type MasterProps = {
  master: Strip;
  level: Level;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The master strip, pinned under the track headers: gain, mute and the output meter. */
export function MasterHeader({ master, level, onStatus, onError }: MasterProps) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  return (
    <div
      style={{ height: TRACK_HEIGHT }}
      className={`grid content-center gap-1 border-tl-bar border-t border-b px-1 ${master.mute ? "opacity-70" : ""}`}
    >
      <div className="flex items-center gap-1">
        <span className="min-w-0 flex-1 truncate px-1 font-bold text-[11px] text-lcd-fg uppercase tracking-[0.08em]">
          Master
        </span>
        <button
          type="button"
          aria-pressed={master.mute}
          aria-label={master.mute ? "Unmute master" : "Mute master"}
          title={master.mute ? "Unmute master" : "Mute master"}
          onClick={() => run(stripMute(null, !master.mute))}
          className={key(master.mute)}
        >
          <MuteGlyph />
        </button>
      </div>
      <StripControls
        name="Master"
        strip={master}
        level={level}
        onGain={(db) => stripGain(null, db)}
        onError={onError}
      />
    </div>
  );
}

type StripProps = {
  name: string;
  strip: Strip;
  level: Level;
  onGain: (db: number) => Promise<void>;
  onPan?: (pan: number) => Promise<void>;
  onError: (message: string) => void;
};

function StripControls({ name, strip, level, onGain, onPan, onError }: StripProps) {
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
    <>
      <div
        className={`grid items-center gap-1 ${onPan ? "grid-cols-[1fr_3.5rem_4rem_2rem]" : "grid-cols-[1fr_3.5rem]"}`}
      >
        <input
          id={`${id}-gain`}
          type="range"
          className="fader-slim"
          min={-60}
          max={12}
          step={0.5}
          value={gain}
          aria-label={`${name} gain`}
          aria-valuetext={`${gain.toFixed(1)} dB`}
          title="Gain; double-click for 0 dB"
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
        <output
          htmlFor={`${id}-gain`}
          className="surface-lcd px-1 text-right font-lcd text-xs leading-tight tabular-nums"
        >
          <span className="lcd-text">{gain.toFixed(1)} dB</span>
        </output>
        {onPan ? (
          <>
            <input
              id={`${id}-pan`}
              type="range"
              className="fader-slim"
              min={-1}
              max={1}
              step={0.01}
              value={pan}
              aria-label={`${name} pan`}
              aria-valuetext={panLabel(pan)}
              title="Pan; double-click for centre"
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
            <output
              htmlFor={`${id}-pan`}
              className="surface-lcd px-1 text-center font-lcd text-xs leading-tight tabular-nums"
            >
              <span className="lcd-text">{panLabel(pan)}</span>
            </output>
          </>
        ) : null}
      </div>
      <Meter label={`${name} output`} level={level} bare />
    </>
  );
}

function panLabel(pan: number): string {
  if (Math.abs(pan) < 0.005) return "C";
  const pct = Math.round(Math.abs(pan) * 100);
  return pan < 0 ? `L${pct}` : `R${pct}`;
}
