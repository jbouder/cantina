import { type FormEvent, type PointerEvent, useEffect, useId, useRef, useState } from "react";
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
  trackMove,
  trackRemove,
  trackRename,
} from "../lib/engine";
import { MASTER_HEIGHT, TRACK_HEIGHT } from "../lib/timeline";
import { ContextMenu } from "./ContextMenu";
import { Led } from "./Led";
import { Meter } from "./Meter";

/** A small square key. On, it lights up through `aria-pressed`. */
const KEY =
  "button-standard grid size-6 shrink-0 place-items-center border-0 p-0 font-bold text-label leading-none";
/** A strip's value beside its fader: display text with no box of its own. */
const VALUE = "font-lcd text-label text-lcd-fg tabular-nums leading-none";

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

/** The pointer handlers that let a header be dragged to a new place. */
export type RowGrip = {
  onPointerDown: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (e: PointerEvent<HTMLDivElement>) => void;
  onPointerCancel: () => void;
};

type Props = {
  track: TrackStatus;
  /** Its place among the tracks, and how many there are, for Move up and down. */
  index: number;
  count: number;
  /** While it is dragged: how far it has moved, in px; null otherwise. */
  offset: number | null;
  /** True where a dragged header would land. */
  dropTarget: boolean;
  grip: RowGrip;
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
 * A track's header beside its lane, in two rows: the name (click to
 * select, double-click to rename) with arm or the kit mark, mute and
 * solo; then the strip's gain and pan, each a fader with its value
 * beside it. A thin post-fader meter runs along the bottom edge. Drag the
 * name row to move the track; right-click for Rename, Move and Remove.
 */
export function TrackHeader({
  track,
  index,
  count,
  offset,
  dropTarget,
  grip,
  selected,
  silenced,
  level,
  onSelect,
  onStatus,
  onError,
}: Props) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(track.name);
  const nameId = useId();
  const rename = (e?: FormEvent) => {
    e?.preventDefault();
    setEditing(false);
    const next = name.trim();
    if (next && next !== track.name) run(trackRename(track.id, next));
    else setName(track.name);
  };
  const startRename = () => {
    setName(track.name);
    setEditing(true);
  };
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: right-click is a shortcut; every action in its menu is also in the editor's track menu and Track in the menu bar
    <div
      style={{
        height: TRACK_HEIGHT,
        ...(offset === null ? {} : { transform: `translateY(${offset}px)` }),
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      className={`relative grid content-center gap-0.5 border-tl-grid border-b px-2 pb-1 ${selected ? "bg-tl-lane" : ""} ${silenced ? "opacity-70" : ""} ${offset === null ? "" : "z-10 bg-tl-lane shadow-lg"} ${dropTarget ? "outline-2 outline-focus -outline-offset-2" : ""}`}
    >
      <div
        className={`flex touch-none select-none items-center gap-1 ${offset === null ? "" : "cursor-grabbing"}`}
        {...(editing ? {} : grip)}
      >
        {editing ? (
          <form onSubmit={rename} className="flex min-w-0 flex-1">
            <label htmlFor={nameId} className="sr-only">
              Track name
            </label>
            <input
              id={nameId}
              // biome-ignore lint/a11y/noAutofocus: the field appears on request, in place of the name
              autoFocus
              className="surface-field min-h-6 min-w-0 flex-1 px-1"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onFocus={(e) => e.currentTarget.select()}
              onBlur={() => rename()}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setName(track.name);
                  setEditing(false);
                }
              }}
            />
          </form>
        ) : (
          <button
            type="button"
            aria-pressed={selected}
            title="Click to select, double-click to rename, drag to move"
            onClick={onSelect}
            onDoubleClick={startRename}
            className="min-h-6 min-w-0 flex-1 cursor-grab truncate border-0 bg-transparent p-0 text-left font-bold text-body text-lcd-fg"
          >
            {track.name}
          </button>
        )}
        {track.kit ? (
          <span
            role="img"
            aria-label="Drum track"
            title={track.kit.name}
            className="grid size-6 shrink-0 place-items-center text-lcd-fg"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
              <path d="M8 2c3.3 0 6 1.1 6 2.5v6C14 12.9 11.3 14 8 14s-6-1.1-6-3.5v-6C2 3.1 4.7 2 8 2zm0 1.5c-2.8 0-4.5.8-4.5 1s1.7 1 4.5 1 4.5-.8 4.5-1-1.7-1-4.5-1z" />
            </svg>
          </span>
        ) : (
          <button
            type="button"
            data-no-grip
            aria-pressed={track.armed}
            aria-label={`Arm ${track.name}`}
            title="Arm for recording"
            onClick={() => run(trackArm(track.id, !track.armed))}
            className="grid size-6 shrink-0 place-items-center border-0 bg-transparent p-0"
          >
            <Led on={track.recording || track.armed} tone="record" label="Armed" />
          </button>
        )}
        <button
          type="button"
          data-no-grip
          aria-pressed={track.strip.mute}
          aria-label={`${track.strip.mute ? "Unmute" : "Mute"} ${track.name}`}
          title={`${track.strip.mute ? "Unmute" : "Mute"} ${track.name}`}
          onClick={() => run(stripMute(track.id, !track.strip.mute))}
          className={KEY}
        >
          <MuteGlyph />
        </button>
        <button
          type="button"
          data-no-grip
          aria-pressed={track.strip.solo}
          aria-label={`${track.strip.solo ? "Unsolo" : "Solo"} ${track.name}`}
          title={
            track.strip.solo
              ? `Unsolo ${track.name}`
              : `Solo ${track.name}: hear only the soloed tracks`
          }
          onClick={() => run(stripSolo(track.id, !track.strip.solo))}
          className={KEY}
        >
          <SoloGlyph />
        </button>
      </div>
      {menu ? (
        <ContextMenu
          label={`${track.name} actions`}
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { label: "Rename…", onSelect: startRename },
            {
              label: "Move up",
              separated: true,
              disabled: index === 0,
              onSelect: () => run(trackMove(track.id, index - 1)),
            },
            {
              label: "Move down",
              disabled: index === count - 1,
              onSelect: () => run(trackMove(track.id, index + 1)),
            },
            {
              label: "Remove track",
              separated: true,
              disabled: track.recording,
              onSelect: () => run(trackRemove(track.id)),
            },
          ]}
        />
      ) : null}
      <StripControls
        name={track.name}
        strip={track.strip}
        onGain={(db) => stripGain(track.id, db)}
        onPan={(pan) => stripPan(track.id, pan)}
        onError={onError}
      />
      <div className="absolute inset-x-2 bottom-1">
        <Meter label={`${track.name} output`} level={level} bare />
      </div>
    </div>
  );
}

type MasterProps = {
  master: Strip;
  level: Level;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/**
 * The master strip, pinned along the bottom of the arrangement and out
 * of the scrolling tracks: mute, gain, and the output meter across the
 * width the lanes take above it.
 */
export function MasterHeader({ master, level, onStatus, onError }: MasterProps) {
  const run = (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e)));
  return (
    <div
      style={{ height: MASTER_HEIGHT }}
      className={`flex items-center gap-2 border-tl-bar border-t px-2 ${master.mute ? "opacity-70" : ""}`}
    >
      <span className="w-14 shrink-0 truncate font-bold text-body text-lcd-fg">Master</span>
      <button
        type="button"
        aria-pressed={master.mute}
        aria-label={master.mute ? "Unmute master" : "Mute master"}
        title={master.mute ? "Unmute master" : "Mute master"}
        onClick={() => run(stripMute(null, !master.mute))}
        className={KEY}
      >
        <MuteGlyph />
      </button>
      <div className="w-56 shrink-0">
        <StripControls
          name="Master"
          strip={master}
          onGain={(db) => stripGain(null, db)}
          onError={onError}
        />
      </div>
      <div className="min-w-0 flex-1">
        <Meter label="Master output" level={level} bare />
      </div>
    </div>
  );
}

type StripProps = {
  name: string;
  strip: Strip;
  onGain: (db: number) => Promise<void>;
  onPan?: (pan: number) => Promise<void>;
  onError: (message: string) => void;
};

function StripControls({ name, strip, onGain, onPan, onError }: StripProps) {
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
    <div
      className={`grid items-center gap-2 ${onPan ? "grid-cols-[1fr_3rem_3.5rem_1.75rem]" : "grid-cols-[1fr_3rem]"}`}
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
      <output htmlFor={`${id}-gain`} className={`${VALUE} text-right`}>
        {gain.toFixed(1)} dB
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
            title="Pan; double-click for center"
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
          <output htmlFor={`${id}-pan`} className={`${VALUE} text-right`}>
            {panLabel(pan)}
          </output>
        </>
      ) : null}
    </div>
  );
}

function panLabel(pan: number): string {
  if (Math.abs(pan) < 0.005) return "C";
  const pct = Math.round(Math.abs(pan) * 100);
  return pan < 0 ? `L${pct}` : `R${pct}`;
}
