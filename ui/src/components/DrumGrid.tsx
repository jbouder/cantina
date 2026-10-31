import {
  type FormEvent,
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import {
  currentStep,
  DEFAULT_VELOCITY,
  draggedVelocity,
  isBeatStart,
  lengthLabel,
  ticksAt,
  velocityAt,
  velocityLabel,
} from "../lib/drums";
import {
  type AuditionStatus,
  type EngineStatus,
  type KitInfo,
  kitsRescan,
  type PatternInfo,
  patternAdd,
  patternAudition,
  patternClipAdd,
  patternRemove,
  patternRename,
  patternSetLength,
  patternSetResolution,
  patternSetStep,
  patternSetSwing,
  RESOLUTIONS,
  type Tempo,
  type TrackStatus,
  trackSetKit,
} from "../lib/engine";
import { Button } from "./Button";
import { Led } from "./Led";
import { Select } from "./Select";
import { PATTERN_DRAG } from "./Timeline";

type Props = {
  /** A drum track: `track.kit` is set. */
  track: TrackStatus;
  patterns: PatternInfo[];
  kits: KitInfo[];
  audition: AuditionStatus | null;
  tempo: Tempo;
  /** Playhead, in seconds. */
  position: number;
  playing: boolean;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** A velocity drag on one cell. */
interface Drag {
  pad: string;
  step: number;
  startY: number;
  /** The velocity before the drag; a clear cell starts from the default. */
  from: number;
  /** What the cell shows now. */
  value: number;
  moved: boolean;
}

/**
 * The step sequencer for one drum track: a pattern's pads against its
 * steps. Click a cell to toggle it, drag up or down on it to set the
 * velocity, or with a cell focused press Space to toggle and the arrows
 * to move around and (Shift) to nudge the velocity. Loop auditions the
 * pattern while the transport plays, and the column under the playhead
 * lights up.
 */
export function DrumGrid({
  track,
  patterns,
  kits,
  audition,
  tempo,
  position,
  playing,
  onStatus,
  onError,
}: Props) {
  const id = useId();
  const run = useCallback(
    (action: Promise<EngineStatus>) => action.then(onStatus, (e) => onError(String(e))),
    [onStatus, onError],
  );
  const auditioning = audition?.track === track.id ? audition.pattern : null;
  const [chosen, setChosen] = useState<number | null>(auditioning);
  const pattern =
    patterns.find((p) => p.id === chosen) ??
    patterns.find((p) => p.id === auditioning) ??
    patterns[0] ??
    null;
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(pattern?.name ?? "");
  const [swing, setSwing] = useState(pattern?.swing ?? 0);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const swingPending = useRef<number | null>(null);
  const [focused, setFocused] = useState<{ pad: string; step: number } | null>(null);
  const grid = useRef<HTMLTableElement>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the fields follow the pattern shown
  useEffect(() => {
    setName(pattern?.name ?? "");
    setSwing(pattern?.swing ?? 0);
    setEditing(false);
  }, [pattern?.id, pattern?.name, pattern?.swing]);

  const pads = track.kit?.pads ?? [];
  const step = pattern
    ? currentStep(position, tempo, pattern, playing && auditioning !== null)
    : null;

  // ----- cells -----

  const cellValue = (pad: string, s: number) =>
    drag && drag.pad === pad && drag.step === s
      ? drag.value
      : pattern
        ? velocityAt(pattern, pad, s)
        : 0;

  const onCellDown = (e: PointerEvent<HTMLButtonElement>, pad: string, s: number) => {
    if (e.button !== 0 || !pattern) return;
    const was = velocityAt(pattern, pad, s);
    const d: Drag = {
      pad,
      step: s,
      startY: e.clientY,
      from: was > 0 ? was : DEFAULT_VELOCITY,
      value: was,
      moved: false,
    };
    dragRef.current = d;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onCellMove = (e: PointerEvent<HTMLButtonElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const dy = e.clientY - d.startY;
    if (!d.moved && Math.abs(dy) < 4) return;
    const next = { ...d, value: draggedVelocity(d.from, dy), moved: true };
    dragRef.current = next;
    setDrag(next);
  };

  const onCellUp = (pad: string, s: number) => {
    const d = dragRef.current;
    dragRef.current = null;
    setDrag(null);
    if (!d || !pattern) return;
    if (d.moved) {
      run(patternSetStep(pattern.id, pad, s, d.value));
    } else {
      const was = velocityAt(pattern, pad, s);
      run(patternSetStep(pattern.id, pad, s, was > 0 ? 0 : DEFAULT_VELOCITY));
    }
  };

  const focusCell = (pad: string, s: number) => {
    setFocused({ pad, step: s });
    const el = grid.current?.querySelector<HTMLButtonElement>(
      `[data-pad="${CSS.escape(pad)}"][data-step="${s}"]`,
    );
    el?.focus();
  };

  const onCellKey = (e: KeyboardEvent<HTMLButtonElement>, padIndex: number, s: number) => {
    if (!pattern) return;
    const pad = pads[padIndex]?.id;
    if (!pad) return;
    const was = velocityAt(pattern, pad, s);
    switch (e.key) {
      case " ":
      case "Enter":
        run(patternSetStep(pattern.id, pad, s, was > 0 ? 0 : DEFAULT_VELOCITY));
        break;
      case "ArrowUp":
        if (e.shiftKey) {
          run(patternSetStep(pattern.id, pad, s, Math.min(1, (was || 0) + 0.1)));
        } else if (padIndex > 0) {
          focusCell(pads[padIndex - 1]?.id ?? pad, s);
        }
        break;
      case "ArrowDown":
        if (e.shiftKey) {
          run(patternSetStep(pattern.id, pad, s, Math.max(0, was - 0.1)));
        } else if (padIndex < pads.length - 1) {
          focusCell(pads[padIndex + 1]?.id ?? pad, s);
        }
        break;
      case "ArrowLeft":
        if (s > 0) focusCell(pad, s - 1);
        break;
      case "ArrowRight":
        if (s < pattern.length - 1) focusCell(pad, s + 1);
        break;
      case "Home":
        focusCell(pad, 0);
        break;
      case "End":
        focusCell(pad, pattern.length - 1);
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  // ----- the controls -----

  const rename = (e: FormEvent) => {
    e.preventDefault();
    setEditing(false);
    if (pattern && name.trim() && name.trim() !== pattern.name) {
      run(patternRename(pattern.id, name));
    } else {
      setName(pattern?.name ?? "");
    }
  };

  // Swing is continuous: at most one value per frame goes to the engine.
  const queueSwing = (value: number) => {
    if (!pattern) return;
    setSwing(value);
    const first = swingPending.current === null;
    swingPending.current = value;
    if (!first) return;
    requestAnimationFrame(() => {
      const v = swingPending.current;
      swingPending.current = null;
      if (v !== null) patternSetSwing(pattern.id, v).catch((e) => onError(String(e)));
    });
  };

  const toggleLoop = () => {
    if (!pattern) return;
    run(patternAudition(auditioning === pattern.id ? null : track.id, pattern.id));
  };

  const steps = pattern
    ? Array.from({ length: pattern.length }, (_, s) => ({ s, key: `step-${s}` }))
    : [];
  const tabbable = (pad: string, s: number) =>
    focused ? focused.pad === pad && focused.step === s : pad === pads[0]?.id && s === 0;

  return (
    <section aria-label={`${track.name} drum grid`} className="grid gap-2">
      <div className="flex flex-wrap items-end gap-2">
        <Select
          id={`${id}-kit`}
          label="Kit"
          value={track.kit?.id ?? ""}
          onChange={(e) => run(trackSetKit(track.id, e.target.value))}
        >
          {track.kit?.missing ? (
            <option value={track.kit.id}>{track.kit.name} (not installed)</option>
          ) : null}
          {kits.map((k) => (
            <option key={k.id} value={k.id}>
              {k.name}
            </option>
          ))}
        </Select>
        <Button onClick={() => run(kitsRescan())} title="Look for kits installed since launch">
          Rescan
        </Button>
        <Select
          id={`${id}-pattern`}
          label="Pattern"
          value={pattern?.id ?? ""}
          onChange={(e) => setChosen(Number(e.target.value))}
          disabled={patterns.length === 0}
        >
          {patterns.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
        <Button
          onClick={() =>
            run(
              patternAdd().then((s) => {
                setChosen(s.project.patterns[s.project.patterns.length - 1]?.id ?? null);
                return s;
              }),
            )
          }
        >
          New
        </Button>
        {pattern ? (
          <>
            {editing ? (
              <form onSubmit={rename} className="flex items-center gap-2">
                <label htmlFor={`${id}-name`} className="sr-only">
                  Pattern name
                </label>
                <input
                  id={`${id}-name`}
                  // biome-ignore lint/a11y/noAutofocus: the field appears on request, under the pointer
                  autoFocus
                  className="surface-lcd min-h-11 w-40 px-2 font-lcd text-lg"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onBlur={rename}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      setName(pattern.name);
                      setEditing(false);
                    }
                  }}
                />
              </form>
            ) : (
              <Button onClick={() => setEditing(true)}>Rename</Button>
            )}
            <Button
              aria-label={`Remove pattern ${pattern.name}`}
              onClick={() => run(patternRemove(pattern.id))}
            >
              Remove
            </Button>
            <Button
              aria-pressed={auditioning === pattern.id}
              onClick={toggleLoop}
              variant={auditioning === pattern.id ? "transport" : "panel"}
              title="Loop this pattern on the track while the transport plays"
            >
              <Led on={auditioning === pattern.id} label="Looping" />
              Loop
            </Button>
            <Button
              draggable
              onDragStart={(e) => {
                e.dataTransfer.setData(PATTERN_DRAG, String(pattern.id));
                e.dataTransfer.effectAllowed = "copy";
              }}
              onClick={() =>
                run(patternClipAdd(track.id, pattern.id, Math.round(ticksAt(position, tempo))))
              }
              title="Place the pattern on this track at the playhead, or drag it onto the timeline; drag the clip's right edge to loop it for more bars"
            >
              Place at playhead
            </Button>
          </>
        ) : null}
      </div>
      {pattern ? (
        <div className="flex flex-wrap items-end gap-3">
          <Select
            id={`${id}-resolution`}
            label="Steps"
            value={pattern.stepsPerBeat}
            onChange={(e) => run(patternSetResolution(pattern.id, Number(e.target.value)))}
          >
            {RESOLUTIONS.map((r) => (
              <option key={r.stepsPerBeat} value={r.stepsPerBeat}>
                {r.label}
              </option>
            ))}
          </Select>
          <span className="flex items-center gap-1 text-sm">
            <label
              htmlFor={`${id}-length`}
              className="font-bold text-[11px] uppercase tracking-[0.08em]"
            >
              Length
            </label>
            <input
              id={`${id}-length`}
              type="number"
              min={1}
              max={128}
              step={1}
              value={pattern.length}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (Number.isInteger(v) && v >= 1 && v <= 128 && v !== pattern.length) {
                  run(patternSetLength(pattern.id, v));
                }
              }}
              className="surface-lcd min-h-11 w-20 px-2 text-center font-lcd text-lg tabular-nums"
            />
            <span className="text-ink-dim">{lengthLabel(pattern, tempo)}</span>
          </span>
          <span className="grid w-48 gap-0.5">
            <span className="flex items-baseline justify-between">
              <label
                htmlFor={`${id}-swing`}
                className="font-bold text-[11px] uppercase tracking-[0.08em]"
              >
                Swing
              </label>
              <output
                htmlFor={`${id}-swing`}
                className="surface-lcd px-1.5 py-0.5 font-lcd text-sm leading-none tabular-nums"
              >
                <span className="lcd-text">{Math.round(swing * 100)}%</span>
              </output>
            </span>
            <input
              id={`${id}-swing`}
              type="range"
              className="fader"
              min={0}
              max={1}
              step={0.01}
              value={swing}
              aria-valuetext={`${Math.round(swing * 100)}%`}
              onChange={(e) => queueSwing(Number(e.target.value))}
              onDoubleClick={() => queueSwing(0)}
            />
          </span>
        </div>
      ) : null}
      {pattern && pads.length > 0 ? (
        <div className="surface-lcd overflow-x-auto p-2">
          <table
            ref={grid}
            aria-label={`${pattern.name}: ${pads.length} pads by ${pattern.length} steps. Space toggles a step, Shift with the up and down arrows sets its velocity.`}
            className="border-separate border-spacing-0"
          >
            <tbody>
              {pads.map((pad, padIndex) => (
                <tr key={pad.id}>
                  <th
                    scope="row"
                    className="lcd-text w-24 max-w-24 truncate pr-2 text-left font-lcd font-normal text-base"
                    title={pad.name}
                  >
                    {pad.name}
                  </th>
                  {steps.map(({ s, key }) => {
                    const v = cellValue(pad.id, s);
                    const beat = isBeatStart(s, pattern);
                    const now = step === s;
                    return (
                      <td key={key} className={`p-0 ${beat ? "pl-1.5" : "pl-0.5"}`}>
                        <button
                          type="button"
                          data-pad={pad.id}
                          data-step={s}
                          tabIndex={tabbable(pad.id, s) ? 0 : -1}
                          aria-pressed={v > 0}
                          aria-label={`${pad.name}, step ${s + 1}${v > 0 ? `, ${velocityLabel(v)}` : ""}`}
                          onPointerDown={(e) => onCellDown(e, pad.id, s)}
                          onPointerMove={onCellMove}
                          onPointerUp={() => onCellUp(pad.id, s)}
                          onPointerCancel={() => {
                            dragRef.current = null;
                            setDrag(null);
                          }}
                          onKeyDown={(e) => onCellKey(e, padIndex, s)}
                          onFocus={() => setFocused({ pad: pad.id, step: s })}
                          className={`relative my-0.5 block h-8 w-7 cursor-ns-resize touch-none overflow-hidden border-0 p-0 ${
                            now ? "bg-tl-bar" : beat ? "bg-tl-lane" : "bg-tl-bg"
                          } ${now ? "outline outline-1 outline-tl-playhead" : ""}`}
                        >
                          {v > 0 ? (
                            <span
                              aria-hidden="true"
                              className="absolute inset-x-0.5 bottom-0.5 block bg-lcd-fg"
                              style={{
                                height: `${Math.round(v * 90) + 10}%`,
                                opacity: 0.45 + v * 0.55,
                              }}
                            />
                          ) : null}
                        </button>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : pattern ? (
        <p className="m-0 text-ink-dim text-sm">
          {track.kit?.missing
            ? `The kit "${track.kit.name}" is not installed, so this track plays silence; pick another.`
            : "This kit has no pads."}
        </p>
      ) : (
        <p className="m-0 text-ink-dim text-sm">No patterns yet: make one with New.</p>
      )}
    </section>
  );
}
