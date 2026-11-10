import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";
import {
  type EngineStatus,
  type PatternInfo,
  padHit,
  patternAudition,
  patternReplace,
  type Tempo,
  type TrackStatus,
  transportPlay,
} from "../lib/engine";
import { setCells } from "../lib/gridEdit";
import {
  addHit,
  KEY_VELOCITY,
  overdub,
  PAD_KEYS,
  padForKey,
  placeOf,
  type Take,
} from "../lib/liveRecord";
import { Button } from "./Button";
import { Led } from "./Led";

type Props = {
  track: TrackStatus;
  pattern: PatternInfo;
  /** The pattern looping on this track, if one is. */
  auditioning: number | null;
  tempo: Tempo;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  run: (action: Promise<EngineStatus>) => void;
  onError: (message: string) => void;
};

/** What recording does: nothing, play-along into the loop, or one step at a time. */
type Mode = "off" | "record" | "step";

/**
 * The kit as pads to play, with the mouse or the keyboard (A S D F… for
 * the kit's pads in order, ⇧ for an accent), and recording into the
 * pattern shown. **Record** loops the pattern and keeps what is played,
 * quantized to the nearest step and overdubbed on what is there, one
 * edit a pass. **Step** writes each hit at a cursor that moves on a
 * step, with ← and → to move it.
 */
export function LivePads({
  track,
  pattern,
  auditioning,
  tempo,
  position,
  playing,
  run,
  onError,
}: Props) {
  const pads = track.kit?.pads ?? [];
  const [mode, setMode] = useState<Mode>("off");
  const [cursor, setCursor] = useState(0);
  const [lit, setLit] = useState<string | null>(null);
  const take = useRef<Take>(new Map());
  const pass = useRef<number | null>(null);
  const looping = playing && auditioning === pattern.id;
  const here = placeOf(position, tempo, pattern);

  // A pass ends: what was played in it becomes one edit.
  const commit = () => {
    const next = overdub(pattern, take.current);
    take.current = new Map();
    if (next) run(patternReplace(next));
  };
  // The current pass, while recording: when it moves on, commit.
  useEffect(() => {
    if (mode !== "record" || !looping) return;
    if (pass.current !== null && here.pass !== pass.current) commit();
    pass.current = here.pass;
  });
  // Recording stopped, or the loop did: keep what was played.
  // biome-ignore lint/correctness/useExhaustiveDependencies: on the switch only
  useEffect(() => {
    if (mode !== "record" || !looping) {
      if (take.current.size > 0) commit();
      pass.current = null;
    }
  }, [mode, looping]);

  const hit = (index: number, velocity: number) => {
    const pad = pads[index];
    if (!pad) return;
    setLit(pad.id);
    window.setTimeout(() => setLit((l) => (l === pad.id ? null : l)), 120);
    padHit(track.id, pad.id, velocity).catch((e) => onError(String(e)));
    if (mode === "record" && looping) {
      const at = placeOf(position, tempo, pattern);
      // A hit quantized onto the next pass's first step waits for it.
      take.current = addHit(take.current, pad.id, at.step, velocity);
    } else if (mode === "step") {
      run(patternReplace(setCells(pattern, [{ pad: pad.id, step: cursor, value: velocity }])));
      setCursor((c) => (c + 1) % pattern.length);
    }
  };

  const record = () => {
    if (mode === "record") {
      setMode("off");
      return;
    }
    setMode("record");
    // Recording loops the pattern shown, from the top, playing.
    if (auditioning !== pattern.id) run(patternAudition(track.id, pattern.id));
    if (!playing) run(transportPlay());
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const index = padForKey(e.key);
    if (index !== null && index < pads.length) {
      if (!e.repeat) hit(index, e.shiftKey ? 1 : KEY_VELOCITY);
    } else if (mode === "step" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      const by = e.key === "ArrowLeft" ? -1 : 1;
      setCursor((c) => (c + by + pattern.length) % pattern.length);
    } else if (e.key === "Escape" && mode !== "off") {
      setMode("off");
    } else {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  // A pad played with the mouse is harder toward its top.
  const press = (e: PointerEvent<HTMLButtonElement>, index: number) => {
    const box = e.currentTarget.getBoundingClientRect();
    const height = Math.max(1, box.height);
    const velocity = 1 - 0.6 * Math.min(1, Math.max(0, (e.clientY - box.top) / height));
    hit(index, velocity);
  };

  return (
    <section aria-label={`Play ${track.name}'s pads`} className="grid gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          aria-pressed={mode === "record"}
          onClick={record}
          title="Loop the pattern and record what you play into it, quantized to the step, one edit a pass"
        >
          <Led on={mode === "record" && looping} label="Recording" tone="record" />
          Record
        </Button>
        <Button
          aria-pressed={mode === "step"}
          onClick={() => setMode(mode === "step" ? "off" : "step")}
          title="Write each hit at the cursor and move it on a step; ← and → move the cursor"
        >
          Step
        </Button>
        <span role="status" className="text-body">
          {mode === "record"
            ? looping
              ? `Recording pass ${here.pass + 1}, step ${here.step + 1}`
              : "Starting the loop…"
            : mode === "step"
              ? `Step ${cursor + 1} of ${pattern.length}`
              : "Play the pads, or the keys under them"}
        </span>
      </div>
      <div
        role="application"
        aria-roledescription="drum pads"
        aria-label="Pads: the keys A S D F G H J K L ; Z X C V B N play them in order, Shift for an accent"
        onKeyDown={onKey}
        className="surface-lcd flex flex-wrap gap-2 p-2"
      >
        {pads.map((pad, i) => (
          <button
            key={pad.id}
            type="button"
            tabIndex={i === 0 ? 0 : -1}
            aria-label={`${pad.name}${PAD_KEYS[i] ? ` (${PAD_KEYS[i].toUpperCase()})` : ""}`}
            onPointerDown={(e) => press(e, i)}
            className={`grid h-16 w-20 content-between border-0 p-2 text-left font-lcd ${
              lit === pad.id ? "bg-lcd-fg text-lcd-bg" : "bg-tl-lane text-lcd-fg"
            }`}
          >
            <span className="truncate text-body">{pad.name}</span>
            <span className="text-right text-label opacity-70">
              {PAD_KEYS[i]?.toUpperCase() ?? ""}
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
