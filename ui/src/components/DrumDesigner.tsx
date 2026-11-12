import {
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { isTyping } from "../lib/actions";
import {
  addSection,
  addStep,
  duplicateSection,
  gridPads,
  moveStep,
  removeSection,
  removeStep,
  renameSection,
  SECTION_KINDS,
  setFill,
  setStep,
  withPattern,
  withSection,
} from "../lib/designEdit";
import { type DrumPart, layout, MAX_REPEAT, patternTicks } from "../lib/drumPart";
import { DEFAULT_VELOCITY, isBeatStart, lengthLabel, stepTicks, ticksAt } from "../lib/drums";
import {
  type DesignPlays,
  drumDesignPlay,
  drumDesignPlays,
  drumDesignSet,
  drumDesignStop,
  type EngineStatus,
  type GrooveSection,
  type MeterFrame,
  type PatternInfo,
  RESOLUTIONS,
} from "../lib/engine";
import { resizePattern, setCells, strokeCells } from "../lib/gridEdit";
import { Button } from "./Button";
import { DesignLanes } from "./DesignLanes";
import { Field, Toggle } from "./Field";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

/** What App reaches into the designer for: its own undo and redo, and play. */
export interface DesignerHandle {
  undo: () => void;
  redo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  /** Plays the loop from the top, or stops it. */
  togglePlay: () => void;
  /** Closes, asking first when the part has changes not on the track. */
  leave: () => void;
}

type Props = {
  status: EngineStatus;
  frame: MeterFrame;
  handle: RefObject<DesignerHandle | null>;
  /** Closes the designer for good: the project is heard again. */
  onClose: () => void;
  onStatus: (s: EngineStatus) => void;
  onError: (e: string) => void;
};

/** Bars apart a fill can come, as the designer offers them. */
const FILL_EVERY = [0, 2, 4, 8, 16];

/** Undo steps the designer keeps. */
const HISTORY = 200;

/** A cell being painted: the value it lays down and the cells crossed so far. */
interface Stroke {
  value: number;
  last: { row: number; step: number };
  pattern: PatternInfo;
}

/**
 * The drum designer: the whole window for building a drum track's part.
 * The kit's pads run down the left of a large grid, an inspector sits on
 * the right, and its transport plays the part on loop at the project's
 * tempo. It works on a copy, heard in place of the project, with a
 * history of its own; nothing reaches the project until the part is
 * placed.
 */
export function DrumDesigner({ status, frame, handle, onClose, onStatus, onError }: Props) {
  const ids = useId();
  const design = status.drumDesign;
  const track = status.project.tracks.find((t) => t.id === design?.track) ?? null;
  // The part as the designer has it: ahead of the engine's echo, so a
  // click shows at once.
  const [part, setPart] = useState<DrumPart | null>(design?.part ?? null);
  const [past, setPast] = useState<DrumPart[]>([]);
  const [future, setFuture] = useState<DrumPart[]>([]);
  const [selected, setSelected] = useState<string>(() =>
    design?.plays.kind === "section" ? design.plays.name : (design?.part.sections[0]?.name ?? ""),
  );
  const [selectedStep, setSelectedStep] = useState<number | null>(null);
  const [stroke, setStroke] = useState<Stroke | null>(null);
  const strokeRef = useRef<Stroke | null>(null);
  const [leaving, setLeaving] = useState(false);
  const grid = useRef<HTMLTableElement>(null);

  const report = useCallback(
    (p: Promise<EngineStatus>) => p.then(onStatus, (e) => onError(String(e))),
    [onStatus, onError],
  );

  const send = useCallback(
    (next: DrumPart, then?: () => Promise<EngineStatus>) => {
      setPart(next);
      drumDesignSet(next)
        .then((s) => (then ? then() : s))
        .then(onStatus, (e) => {
          onError(String(e));
          if (design) setPart(design.part);
        });
    },
    [design, onStatus, onError],
  );

  /** One step in the designer's history. */
  const commit = useCallback(
    (next: DrumPart, then?: () => Promise<EngineStatus>) => {
      if (!part || JSON.stringify(next) === JSON.stringify(part)) return;
      setPast((p) => [...p.slice(-(HISTORY - 1)), part]);
      setFuture([]);
      send(next, then);
    },
    [part, send],
  );

  const undo = useCallback(() => {
    const prev = past.at(-1);
    if (!prev || !part) return;
    setPast((p) => p.slice(0, -1));
    setFuture((f) => [part, ...f]);
    send(prev);
    if (!prev.sections.some((s) => s.name === selected)) setSelected(prev.sections[0]?.name ?? "");
  }, [past, part, send, selected]);

  const redo = useCallback(() => {
    const next = future[0];
    if (!next || !part) return;
    setFuture((f) => f.slice(1));
    setPast((p) => [...p, part]);
    send(next);
    if (!next.sections.some((s) => s.name === selected)) setSelected(next.sections[0]?.name ?? "");
  }, [future, part, send, selected]);

  const playing = status.transport.playing || frame.countIn > 0;
  const togglePlay = useCallback(() => {
    report(playing ? drumDesignStop() : drumDesignPlay());
  }, [playing, report]);

  // Close asks first when the part has changes not on the track.
  const leave = useCallback(() => {
    if (design?.changed) setLeaving(true);
    else onClose();
  }, [design?.changed, onClose]);

  useImperativeHandle(
    handle,
    () => ({
      undo,
      redo,
      canUndo: past.length > 0,
      canRedo: future.length > 0,
      togglePlay,
      leave,
    }),
    [undo, redo, past.length, future.length, togglePlay, leave],
  );

  const section = part?.sections.find((s) => s.name === selected) ?? part?.sections[0] ?? null;
  const pattern = stroke?.pattern ?? section?.pattern ?? null;
  const length = pattern?.length ?? 0;
  const steps = useMemo(
    () => Array.from({ length }, (_, s) => ({ s, key: `step-${s}` })),
    [length],
  );
  const pads = useMemo(
    () => (pattern ? gridPads(track?.kit?.pads ?? [], pattern) : []),
    [track, pattern],
  );

  // What is under the playhead: the span of the plan it is in, the step
  // of this section's grid, and where it is in the song or the section.
  const tempo = status.project.tempo;
  const ticks = status.transport.playing ? ticksAt(frame.position, tempo) : null;
  const now =
    ticks === null
      ? null
      : (design?.spans.find((s) => ticks >= s.start && ticks < s.start + s.length) ?? null);
  let step: number | null = null;
  if (ticks !== null && now && section && pattern && now.section === section.name) {
    const local = (ticks - now.start) % patternTicks(pattern);
    step = Math.min(pattern.length - 1, Math.floor(local / stepTicks(pattern)));
  }
  const barTicks = design?.barTicks ?? 3840;
  const beatTicks = (960 * 4) / tempo.denominator;
  const stepStart =
    now?.step != null && part ? (layout(part).find((s) => s.step === now.step)?.start ?? 0) : 0;
  const at = ticks !== null && now ? stepStart + ticks - now.start : 0;
  const position = `${Math.floor(at / barTicks) + 1}.${Math.floor((at % barTicks) / beatTicks) + 1}`;

  // Picks the section the grid edits; looping a section, it is the one
  // heard from the next bar.
  const plays: DesignPlays | null = design?.plays ?? null;
  const pick = useCallback(
    (name: string) => {
      setSelected(name);
      if (plays?.kind === "section" && plays.name !== name) {
        report(drumDesignPlays({ kind: "section", name }));
      }
    },
    [plays, report],
  );

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (leaving || document.activeElement?.closest("[role=dialog]") !== null) return;
      if (e.key === "Escape") {
        leave();
        return;
      }
      if (isTyping(document.activeElement) || e.metaKey || e.ctrlKey || e.altKey) return;
      const n = Number(e.key);
      const chosen = Number.isInteger(n) && n >= 1 ? part?.sections[n - 1] : undefined;
      if (chosen) {
        e.preventDefault();
        pick(chosen.name);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [leave, leaving, part, pick]);

  if (!design || !part || !section || !pattern || !plays) return null;

  const valueAt = (pad: string, s: number) =>
    pattern.rows.find((r) => r.pad === pad)?.steps[s] ?? 0;

  const paint = (s: Stroke, row: number, at: number): Stroke => {
    const cells = strokeCells(s.last, { row, step: at })
      .map((c) => ({ pad: pads[c.row]?.id ?? "", step: c.step, value: s.value }))
      .filter((c) => c.pad);
    return { ...s, last: { row, step: at }, pattern: setCells(s.pattern, cells) };
  };

  const onCellDown = (e: PointerEvent<HTMLButtonElement>, row: number, at: number) => {
    if (e.button !== 0) return;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    const value = valueAt(pads[row]?.id ?? "", at) > 0 ? 0 : DEFAULT_VELOCITY;
    const s = paint({ value, last: { row, step: at }, pattern: section.pattern }, row, at);
    strokeRef.current = s;
    setStroke(s);
  };
  const onCellEnter = (row: number, at: number) => {
    const s = strokeRef.current;
    if (!s) return;
    const next = paint(s, row, at);
    strokeRef.current = next;
    setStroke(next);
  };
  const endStroke = () => {
    const s = strokeRef.current;
    strokeRef.current = null;
    setStroke(null);
    if (s) commit(withPattern(part, section.name, s.pattern));
  };

  const toggleCell = (row: number, at: number) => {
    const pad = pads[row]?.id;
    if (!pad) return;
    const value = valueAt(pad, at) > 0 ? 0 : DEFAULT_VELOCITY;
    commit(withPattern(part, section.name, setCells(section.pattern, [{ pad, step: at, value }])));
  };

  const focusCell = (row: number, at: number) => {
    grid.current
      ?.querySelector<HTMLButtonElement>(`[data-row="${row}"][data-step="${at}"]`)
      ?.focus();
  };
  const onCellKey = (e: KeyboardEvent<HTMLButtonElement>, row: number, at: number) => {
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    };
    const move = moves[e.key];
    if (move) {
      e.preventDefault();
      focusCell(
        Math.max(0, Math.min(pads.length - 1, row + move[0])),
        Math.max(0, Math.min(pattern.length - 1, at + move[1])),
      );
    } else if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      toggleCell(row, at);
    }
  };

  const perBar = pattern.stepsPerBeat * tempo.numerator;
  const orderStep = selectedStep === null ? undefined : part.order[selectedStep];
  const bars = Math.max(1, Math.round(pattern.length / perBar));
  const lengthOptions = [1, 2, 3, 4, 6, 8].filter((b) => b * perBar <= 128);

  return (
    <section
      aria-label="Drum designer"
      className="grid min-h-0 flex-1 grid-rows-[auto_auto_minmax(0,1fr)] gap-3"
    >
      <header className="@container flex min-w-0 items-center gap-2">
        <span className="caption shrink-0">Drum designer</span>
        <Lcd className="min-w-0 max-w-80 truncate" title={`${track?.name} · ${track?.kit?.name}`}>
          {track?.name ?? "Drums"} · {track?.kit?.name ?? part.kit}
        </Lcd>
        <span className="flex-1" />
        <fieldset className="m-0 flex shrink-0 gap-0.5 border-0 p-0">
          <legend className="sr-only">What plays</legend>
          <Button
            tight
            aria-pressed={plays.kind === "section"}
            title={`Loop ${section.name} alone`}
            onClick={() => report(drumDesignPlays({ kind: "section", name: section.name }))}
          >
            <span className="@3xl:inline hidden">Loop section</span>
            <span className="@3xl:hidden">Section</span>
          </Button>
          <Button
            tight
            aria-pressed={plays.kind === "order"}
            disabled={part.order.length === 0}
            title="Loop the whole song, in order"
            onClick={() => report(drumDesignPlays({ kind: "order", from: 0 }))}
          >
            <span className="@3xl:inline hidden">Whole song</span>
            <span className="@3xl:hidden">Song</span>
          </Button>
        </fieldset>
        <Button
          square
          aria-label={playing ? "Stop" : "Play"}
          aria-pressed={playing}
          title={playing ? "Stop (Space)" : "Play on loop (Space)"}
          disabled={!status.running}
          onClick={togglePlay}
        >
          {playing ? (
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 3h10v10H3z" />
            </svg>
          ) : (
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M4 2.5v11l9-5.5z" />
            </svg>
          )}
        </Button>
        <Lcd
          className="shrink-0 tabular-nums"
          aria-label={`Bar ${position}, ${Math.round(tempo.bpm)} beats per minute`}
        >
          {position}
          <span className="@4xl:inline hidden">
            {" "}
            · {Math.round(tempo.bpm)} bpm {tempo.numerator}/{tempo.denominator}
          </span>
        </Lcd>
        <Button
          square
          aria-label="Undo"
          title="Undo in the designer (⌘Z)"
          disabled={past.length === 0}
          onClick={undo}
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
            <path d="M5.5 3 1.5 7l4 4V8h4a3 3 0 0 1 0 6H8v1.5h1.5a4.5 4.5 0 0 0 0-9h-4z" />
          </svg>
        </Button>
        <Button
          square
          aria-label="Redo"
          title="Redo in the designer (⇧⌘Z)"
          disabled={future.length === 0}
          onClick={redo}
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 -scale-x-100 fill-current">
            <path d="M5.5 3 1.5 7l4 4V8h4a3 3 0 0 1 0 6H8v1.5h1.5a4.5 4.5 0 0 0 0-9h-4z" />
          </svg>
        </Button>
        <Button
          tight
          onClick={leave}
          title={`Back to ${status.project.name}; the part reaches it only when placed`}
        >
          <span className="@2xl:inline hidden">Back to project</span>
          <span className="@2xl:hidden">Back</span>
        </Button>
      </header>

      <DesignLanes
        part={part}
        editing={section.name}
        playingSection={now?.section ?? null}
        playingStep={now?.step ?? null}
        selectedStep={selectedStep}
        barTicks={barTicks}
        onEdit={(name) => {
          setSelectedStep(null);
          pick(name);
        }}
        onAddSection={(kind) => {
          const [next, name] = addSection(part, kind, section.pattern, perBar);
          commit(next);
          setSelected(name);
          setSelectedStep(null);
        }}
        onSelectStep={(i) => {
          setSelectedStep(selectedStep === i ? null : i);
          const name = part.order[i]?.section;
          if (name) setSelected(name);
        }}
        onAddStep={() => {
          commit(addStep(part, section.name));
          setSelectedStep(part.order.length);
        }}
      />

      <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_minmax(14rem,18rem)] gap-3">
        <div className="surface-lcd scroll-area min-h-0 p-2">
          {pads.length > 0 ? (
            <table
              ref={grid}
              aria-label={`${section.name}: ${pads.length} pads by ${pattern.length} steps. Arrows move, Space or Return toggles a step.`}
              className="border-separate border-spacing-0"
            >
              <tbody>
                {pads.map((pad, row) => (
                  <tr key={pad.id}>
                    <th
                      scope="row"
                      className="lcd-text sticky left-0 z-[1] w-28 max-w-28 truncate bg-lcd-bg pr-2 text-left font-lcd font-normal text-readout"
                      title={pad.name}
                    >
                      {pad.name}
                    </th>
                    {steps.map(({ s, key }) => {
                      const v = valueAt(pad.id, s);
                      const beat = isBeatStart(s, pattern);
                      const now = step === s;
                      return (
                        <td key={key} className={`p-0 ${beat ? "pl-2" : "pl-0.5"}`}>
                          <button
                            type="button"
                            data-row={row}
                            data-step={s}
                            tabIndex={row === 0 && s === 0 ? 0 : -1}
                            aria-pressed={v > 0}
                            aria-label={`${pad.name}, step ${s + 1}${v > 0 ? `, ${Math.round(v * 100)}%` : ""}`}
                            onPointerDown={(e) => onCellDown(e, row, s)}
                            onPointerEnter={() => onCellEnter(row, s)}
                            onPointerUp={endStroke}
                            onPointerCancel={endStroke}
                            onClick={(e) => {
                              // A click from the keyboard: pointer clicks were painted.
                              if (e.detail === 0) toggleCell(row, s);
                            }}
                            onKeyDown={(e) => onCellKey(e, row, s)}
                            className={`relative my-0.5 block h-9 w-8 touch-none overflow-hidden border-0 p-0 ${
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
          ) : (
            <p className="lcd-text m-0 font-lcd text-readout">
              {track?.kit?.missing
                ? `The kit "${track.kit.name}" is not installed, so the part plays silence.`
                : "This kit has no pads."}
            </p>
          )}
        </div>

        <aside
          aria-label="Inspector"
          className="surface-panel scroll-area grid min-h-0 content-start gap-3 p-3"
        >
          {orderStep && selectedStep !== null ? (
            <>
              <h3 className="caption m-0">Step {selectedStep + 1} of the order</h3>
              <Select
                id={`${ids}-step-section`}
                label="Plays"
                value={orderStep.section}
                onChange={(e) => {
                  commit(setStep(part, selectedStep, { section: e.target.value }));
                  setSelected(e.target.value);
                }}
              >
                {part.sections.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.name}
                  </option>
                ))}
              </Select>
              <Field
                label="Times"
                hint={`${
                  (orderStep.times *
                    patternTicks(
                      part.sections.find((x) => x.name === orderStep.section)?.pattern ??
                        section.pattern,
                    )) /
                  barTicks
                } bars`}
                htmlFor={`${ids}-times`}
              >
                <span className="flex w-full items-center gap-1">
                  <Button
                    square
                    aria-label="Fewer times"
                    disabled={orderStep.times <= 1}
                    onClick={() =>
                      commit(setStep(part, selectedStep, { times: orderStep.times - 1 }))
                    }
                  >
                    −
                  </Button>
                  <input
                    key={orderStep.times}
                    id={`${ids}-times`}
                    type="number"
                    min={1}
                    max={MAX_REPEAT}
                    defaultValue={orderStep.times}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") e.currentTarget.blur();
                    }}
                    onBlur={(e) => {
                      const times = Number(e.currentTarget.value);
                      if (Number.isFinite(times)) commit(setStep(part, selectedStep, { times }));
                    }}
                    className="surface-field min-h-8 w-12 min-w-0 flex-1 px-2 text-center tabular-nums"
                  />
                  <Button
                    square
                    aria-label="More times"
                    disabled={orderStep.times >= MAX_REPEAT}
                    onClick={() =>
                      commit(setStep(part, selectedStep, { times: orderStep.times + 1 }))
                    }
                  >
                    +
                  </Button>
                </span>
              </Field>
              <span className="flex flex-wrap gap-1">
                <Button
                  onClick={() => report(drumDesignPlays({ kind: "order", from: selectedStep }))}
                  title="Play the song from this step, from the next bar while it rolls"
                >
                  Play from here
                </Button>
                <Button
                  square
                  aria-label="Earlier"
                  title="Move the step earlier"
                  disabled={selectedStep === 0}
                  onClick={() => {
                    commit(moveStep(part, selectedStep, -1));
                    setSelectedStep(selectedStep - 1);
                  }}
                >
                  ←
                </Button>
                <Button
                  square
                  aria-label="Later"
                  title="Move the step later"
                  disabled={selectedStep >= part.order.length - 1}
                  onClick={() => {
                    commit(moveStep(part, selectedStep, 1));
                    setSelectedStep(selectedStep + 1);
                  }}
                >
                  →
                </Button>
                <Button
                  tier="quiet"
                  onClick={() => {
                    commit(removeStep(part, selectedStep));
                    setSelectedStep(null);
                  }}
                >
                  Remove step
                </Button>
              </span>
            </>
          ) : null}
          <h3 className="caption m-0">Section</h3>
          <TextInput
            key={section.name}
            id={`${ids}-name`}
            label="Name"
            defaultValue={section.name}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
            }}
            onBlur={(e) => {
              const to = e.currentTarget.value;
              const next = renameSection(part, section.name, to);
              if (next === part) {
                e.currentTarget.value = section.name;
                return;
              }
              const renamed = next.sections[part.sections.indexOf(section)]?.name ?? to;
              // The loop follows the section it was playing.
              const follow =
                plays.kind === "section" && plays.name === section.name
                  ? () => drumDesignPlays({ kind: "section", name: renamed })
                  : undefined;
              commit(next, follow);
              setSelected(renamed);
            }}
          />
          <Select
            id={`${ids}-kind`}
            label="Kind"
            hint="What part of the song it is"
            value={section.kind ?? ""}
            onChange={(e) =>
              commit(
                withSection(part, section.name, (s) => ({
                  ...s,
                  kind: (e.target.value || undefined) as GrooveSection | undefined,
                })),
              )
            }
          >
            <option value="">Your own</option>
            {SECTION_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
          </Select>
          <Select
            id={`${ids}-length`}
            label="Length"
            hint={`${lengthLabel(pattern, tempo)} of ${RESOLUTIONS.find((r) => r.stepsPerBeat === pattern.stepsPerBeat)?.label ?? ""} steps`}
            value={pattern.length % perBar === 0 ? bars : ""}
            onChange={(e) =>
              commit(
                withPattern(
                  part,
                  section.name,
                  resizePattern(section.pattern, Number(e.target.value) * perBar),
                ),
              )
            }
          >
            {pattern.length % perBar === 0 ? null : (
              <option value="">{pattern.length} steps</option>
            )}
            {lengthOptions.map((b) => (
              <option key={b} value={b}>
                {b} bar{b === 1 ? "" : "s"}
              </option>
            ))}
          </Select>
          <Field
            label="Swing"
            hint={`${Math.round(pattern.swing * 100)}%`}
            htmlFor={`${ids}-swing`}
          >
            <input
              id={`${ids}-swing`}
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={pattern.swing}
              onChange={(e) =>
                commit(
                  withPattern(part, section.name, {
                    ...section.pattern,
                    swing: Number(e.target.value),
                  }),
                )
              }
              className="w-full"
            />
          </Field>
          <Select
            id={`${ids}-fill`}
            label="Fill"
            hint="Another section's first bar, played in its place"
            value={section.fill?.section ?? ""}
            onChange={(e) => {
              const fill = e.target.value;
              commit(
                setFill(
                  part,
                  section.name,
                  fill
                    ? {
                        section: fill,
                        every: section.fill?.every ?? 4,
                        atEnd: section.fill?.atEnd ?? false,
                      }
                    : null,
                ),
              );
            }}
          >
            <option value="">None</option>
            {part.sections
              .filter((s) => s.name !== section.name)
              .map((s) => (
                <option key={s.name} value={s.name}>
                  {s.name}
                </option>
              ))}
          </Select>
          {section.fill ? (
            <>
              <Select
                id={`${ids}-every`}
                label="Every"
                hint="The last bar of every so many"
                value={section.fill.every}
                onChange={(e) => {
                  const fill = section.fill;
                  if (fill) {
                    commit(setFill(part, section.name, { ...fill, every: Number(e.target.value) }));
                  }
                }}
              >
                {FILL_EVERY.map((n) => (
                  <option key={n} value={n}>
                    {n === 0 ? "Never" : `${n} bars`}
                  </option>
                ))}
              </Select>
              <Toggle
                id={`${ids}-at-end`}
                label="At its end"
                hint="Its last bar in each step of the order"
                checked={section.fill.atEnd}
                onChange={(atEnd) => {
                  const fill = section.fill;
                  if (fill) commit(setFill(part, section.name, { ...fill, atEnd }));
                }}
              />
            </>
          ) : null}
          <span className="flex flex-wrap gap-1">
            <Button
              onClick={() => {
                const [next, name] = duplicateSection(part, section.name);
                commit(next);
                setSelected(name);
              }}
            >
              Duplicate
            </Button>
            <Button
              tier="quiet"
              disabled={part.sections.length < 2}
              title={`Remove ${section.name}, its steps in the order and fills from it`}
              onClick={() => {
                const i = part.sections.indexOf(section);
                const next = removeSection(part, section.name);
                commit(next);
                setSelected(next.sections[Math.max(0, i - 1)]?.name ?? "");
                setSelectedStep(null);
              }}
            >
              Remove section
            </Button>
          </span>
        </aside>
      </div>

      {leaving ? (
        <Sheet
          title="Leave the drum designer?"
          onClose={() => setLeaving(false)}
          footer={
            <>
              <Button onClick={() => setLeaving(false)}>Keep designing</Button>
              <Button
                tier="primary"
                onClick={() => {
                  setLeaving(false);
                  onClose();
                }}
              >
                Leave without placing
              </Button>
            </>
          }
        >
          <p className="m-0 text-body">
            The part has changes that are not on {track?.name ?? "the track"}. Leaving drops them;{" "}
            {status.project.name} stays as it was.
          </p>
        </Sheet>
      ) : null}
    </section>
  );
}
