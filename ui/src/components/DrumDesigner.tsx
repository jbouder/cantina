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
  putGroove,
  putInSection,
  removeSection,
  removeStep,
  renameSection,
  SECTION_KINDS,
  setFill,
  setStep,
  withSection,
} from "../lib/designEdit";
import { type DrumPart, layout, MAX_REPEAT, patternTicks } from "../lib/drumPart";
import { DEFAULT_VELOCITY, isBeatStart, lengthLabel, stepTicks, ticksAt } from "../lib/drums";
import {
  type DesignPlays,
  drumDesignAudition,
  drumDesignPlace,
  drumDesignPlay,
  drumDesignPlays,
  drumDesignSet,
  drumDesignStop,
  type EngineStatus,
  type GrooveSection,
  grooveFileRead,
  type MeterFrame,
  type PatternInfo,
  type PlaceMode,
  patternFileRead,
  RESOLUTIONS,
} from "../lib/engine";
import {
  type MarkKind,
  resizePattern,
  setCells,
  stepMark,
  strokeCells,
  withPatternFeel,
} from "../lib/gridEdit";
import { addPartHit, heardTicks, landing, overdubPart, type PartTake } from "../lib/liveRecord";
import {
  ACCENT,
  addVariation,
  barCount,
  barSteps,
  clearBar,
  copyBar,
  drawVelocity,
  duplicateBar,
  everyVariation,
  GHOST,
  nudgeVelocity,
  passes,
  pasteBar,
  removeVariation,
  shiftSteps,
  strike,
  VARIATIONS,
  variationOf,
  withVariation,
} from "../lib/patternTools";
import { Button } from "./Button";
import { DesignLanes } from "./DesignLanes";
import { DesignPads } from "./DesignPads";
import { DesignPalette } from "./DesignPalette";
import { Field, Toggle } from "./Field";
import { KitWorkbench } from "./KitWorkbench";
import { Lcd } from "./Lcd";
import { Led } from "./Led";
import { SaveGrooveSheet } from "./SaveGrooveSheet";
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

/** A step's mark as the cell shows it: ×3 for a ratchet, f for a flam, 50 for a chance. */
function markLabel(m: { chance?: number; ratchet?: number; flam?: boolean }): string {
  return [
    (m.ratchet ?? 1) > 1 ? `×${m.ratchet}` : "",
    m.flam ? "f" : "",
    (m.chance ?? 1) < 1 ? `${Math.round((m.chance ?? 1) * 100)}` : "",
  ]
    .filter(Boolean)
    .join(" ");
}

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
  // The variation the grid edits (0 is A), the pad the velocity lane
  // draws, the bar the bar tools work on, and a copied bar.
  // In a browser `?variant=<1-3>` opens on a variation, for screenshots.
  const [variant, setVariant] = useState(() =>
    Number(new URLSearchParams(window.location.search).get("variant") ?? 0),
  );
  const [lanePad, setLanePad] = useState<string | null>(null);
  const [bar, setBar] = useState(0);
  const [copied, setCopied] = useState<ReturnType<typeof copyBar> | null>(null);
  const velocityDrag = useRef(false);
  // The side panel, the save sheet, and the name of the groove last
  // opened, to save it back under. In a browser `?palette` opens the palette.
  const [panel, setPanel] = useState<"inspector" | "palette" | "kit">(() => {
    const q = new URLSearchParams(window.location.search);
    return q.has("palette") ? "palette" : q.has("workbench") ? "kit" : "inspector";
  });
  const [saving, setSaving] = useState(false);
  // The place sheet, and what it does: replace or append.
  // In a browser `?place` opens the sheet, for screenshots.
  const [placing, setPlacing] = useState(() =>
    new URLSearchParams(window.location.search).has("place"),
  );
  const [placeMode, setPlaceMode] = useState<PlaceMode>("replace");
  // Playing a part in: the pads strip, recording, the take of the pass
  // under way, the pass it belongs to, and how deep the history was after
  // the last take went in (for Undo take). `?pads` opens the strip.
  const [padsOpen, setPadsOpen] = useState(() =>
    new URLSearchParams(window.location.search).has("pads"),
  );
  const [recording, setRecording] = useState(false);
  const take = useRef<PartTake>(new Map());
  const takePass = useRef<string | null>(null);
  const [takeDepth, setTakeDepth] = useState<number | null>(null);
  const [grooveName, setGrooveName] = useState("");
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
  const variantIndex = Math.min(variant, section?.variations?.length ?? 0);
  const base = section ? variationOf(section, variantIndex) : null;
  const pattern = stroke?.pattern ?? base;
  const length = pattern?.length ?? 0;
  const steps = useMemo(
    () => Array.from({ length }, (_, s) => ({ s, key: `step-${s}` })),
    [length],
  );
  const pads = useMemo(
    () => (pattern ? gridPads(design?.pads ?? [], pattern) : []),
    [design?.pads, pattern],
  );

  // What is under the playhead: the span of the plan it is in, the step
  // of this section's grid, and where it is in the song or the section.
  const tempo = status.project.tempo;
  const ticks = status.transport.playing ? ticksAt(frame.position, tempo) : null;
  const now =
    ticks === null
      ? null
      : (design?.spans.find((s) => ticks >= s.start && ticks < s.start + s.length) ?? null);
  // With variations the section plays a round of passes; the grid
  // lights only on the pass of the variation it shows.
  let step: number | null = null;
  if (ticks !== null && now && section && pattern && now.section === section.name) {
    const pass = patternTicks(pattern);
    const round = passes(section);
    const local = (ticks - now.start) % (pass * round.length);
    if (round[Math.floor(local / pass)] === variantIndex) {
      step = Math.min(pattern.length - 1, Math.floor((local % pass) / stepTicks(pattern)));
    }
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
      setVariant(0);
      setBar(0);
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

  if (!design || !part || !section || !pattern || !base || !plays) return null;

  /** A pattern file from the palette, into section `name`. */
  const takePattern = (path: string, name: string) =>
    patternFileRead(path).then(
      (p) => commit(putInSection(part, name, p)),
      (e) => onError(String(e)),
    );
  /** A whole stock genre: a section of each kind, in the project's meter where it can be. */
  const takeGenre = (genre: string) => {
    const all = status.grooves.filter((g) => g.genre === genre);
    const kinds = [...new Set(all.map((g) => g.section))];
    const picked = kinds.flatMap((kind) => {
      const of = all.filter((g) => g.section === kind);
      const fit = of.find(
        (g) =>
          g.timeSignature.numerator === tempo.numerator &&
          g.timeSignature.denominator === tempo.denominator,
      );
      const g = fit ?? of[0];
      return g ? [[kind, g.path] as const] : [];
    });
    Promise.all(picked.map(([, path]) => patternFileRead(path))).then(
      (patterns) =>
        commit(
          putGroove(
            part,
            picked.map(([kind], i) => [kind, patterns[i] as PatternInfo]),
          ),
        ),
      (e) => onError(String(e)),
    );
  };
  /** One of the user's grooves, in place of the part; its kit when it is installed. */
  const openGroove = (path: string) =>
    grooveFileRead(path).then(
      (g) => {
        const kit = status.kits.some((k) => k.id === g.kit) ? g.kit : part.kit;
        commit({ kit, sections: g.sections, order: g.order });
        setSelected(g.sections[0]?.name ?? "");
        setSelectedStep(null);
        setVariant(0);
        setGrooveName(g.name);
      },
      (e) => onError(String(e)),
    );

  // ----- playing a part in -----

  const outputSeconds = status.latency.outputFrames / Math.max(1, status.sampleRate);
  const heard = status.transport.playing ? heardTicks(frame.position, outputSeconds, tempo) : null;
  const heardSpan =
    heard === null
      ? null
      : (design.spans.find(
          (s) => s.section === section.name && heard >= s.start && heard < s.start + s.length,
        ) ?? null);
  const round = passes(section);
  const heardAt =
    heard !== null && heardSpan ? landing(heard, heardSpan.start, section.pattern, round) : null;
  const recordingVariant = heardAt?.variant ?? 0;

  /** The take of the pass just played, into the part as one step of history. */
  const commitTake = () => {
    const out = overdubPart([section.pattern, ...(section.variations ?? [])], take.current);
    take.current = new Map();
    if (out.size === 0) return;
    commit(
      withSection(part, section.name, (s) => {
        let next = s;
        for (const [variant, p] of out) next = withVariation(next, variant, p);
        return next;
      }),
    );
    setTakeDepth(past.length + 1);
  };

  const hitPad = (pad: string, velocity: number) => {
    drumDesignAudition(pad, velocity).catch((e) => onError(String(e)));
    if (!recording || heard === null || !heardSpan) return;
    const at = landing(heard, heardSpan.start, section.pattern, round);
    // A hit quantized past the end of where the section plays belongs to
    // what comes next: it is left out.
    if (at.pass * patternTicks(section.pattern) >= heardSpan.length) return;
    take.current = addPartHit(take.current, at, pad, velocity);
  };

  const toggleRecord = () => {
    if (recording) {
      commitTake();
      setRecording(false);
      return;
    }
    take.current = new Map();
    takePass.current = null;
    setRecording(true);
    const loop =
      plays.kind === "section" && plays.name === section.name
        ? Promise.resolve(status)
        : drumDesignPlays({ kind: "section", name: section.name });
    report(loop.then((s) => (s.transport.playing || frame.countIn > 0 ? s : drumDesignPlay(true))));
  };

  // Rendered while recording, after the hits it ends were taken: when a
  // pass of the recording ends, what was played in it goes in as a take.
  // (Once: the pass is remembered before the take goes in.)
  // A pass of the recording ends: what was played in it goes in as a take.
  const passKey = heardSpan && heardAt ? `${heardSpan.start}:${heardAt.pass}` : null;
  if (recording && passKey !== null && takePass.current !== passKey) {
    if (takePass.current !== null && take.current.size > 0) queueMicrotask(commitTake);
    takePass.current = passKey;
  }

  /** One step of history: the pattern the grid shows, replaced. */
  const edit = (next: PatternInfo) =>
    commit(withSection(part, section.name, (s) => withVariation(s, variantIndex, next)));
  /** One step of history: every variation of the section changed alike. */
  const editAll = (change: (p: PatternInfo) => PatternInfo) =>
    commit(withSection(part, section.name, (s) => everyVariation(s, change)));
  const numerator = tempo.numerator;
  const kitName =
    design.kitEdit?.kit.name ?? status.kits.find((k) => k.id === part.kit)?.name ?? part.kit;
  const bars = barCount(pattern, numerator);
  const barAt = Math.min(bar, bars - 1);
  const barOptions = Array.from({ length: bars }, (_, b) => ({ b, key: `bar-${b + 1}` }));
  const lane = pads.find((p) => p.id === lanePad) ?? pads[0] ?? null;

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
    const s = paint({ value, last: { row, step: at }, pattern: base }, row, at);
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
    if (s) edit(s.pattern);
  };

  const toggleCell = (row: number, at: number) => {
    const pad = pads[row]?.id;
    if (!pad) return;
    const value = valueAt(pad, at) > 0 ? 0 : DEFAULT_VELOCITY;
    edit(setCells(base, [{ pad, step: at, value }]));
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
      setLanePad(pads[Math.max(0, Math.min(pads.length - 1, row + move[0]))]?.id ?? null);
    } else if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      toggleCell(row, at);
    } else if (!e.metaKey && !e.ctrlKey && !e.altKey) {
      const pad = pads[row]?.id;
      if (!pad) return;
      const key = e.key.toLowerCase();
      const marks: Record<string, MarkKind> = { c: "chance", r: "ratchet", f: "flam" };
      const mark = marks[key];
      let next: PatternInfo | null = null;
      if (mark) next = stepMark(base, [{ pad, step: at }], mark);
      else if (key === "g") next = strike(base, pad, at, GHOST);
      else if (key === "a") next = strike(base, pad, at, ACCENT);
      else if (e.key === "Backspace" || e.key === "Delete") next = strike(base, pad, at, 0);
      else if (e.key === "[") next = shiftSteps(base, -1);
      else if (e.key === "]") next = shiftSteps(base, 1);
      else return;
      e.preventDefault();
      e.stopPropagation();
      if (next) edit(next);
    }
  };
  // ⇧↑ and ⇧↓ make a hit harder or softer, before the arrows move.
  const onCellKeyCapture = (e: KeyboardEvent<HTMLButtonElement>, row: number, at: number) => {
    const pad = pads[row]?.id;
    if (!e.shiftKey || !pad || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    e.stopPropagation();
    edit(nudgeVelocity(base, pad, at, e.key === "ArrowUp" ? 0.1 : -0.1));
  };

  /** The velocity the lane's pointer is at, from the bottom of a cell (soft) to its top (full). */
  const laneValue = (e: PointerEvent<HTMLElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return 1 - (e.clientY - r.top) / Math.max(1, r.height);
  };
  const onLaneDown = (e: PointerEvent<HTMLButtonElement>, at: number) => {
    if (e.button !== 0 || !lane) return;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    velocityDrag.current = true;
    const s = {
      value: 0,
      last: { row: 0, step: at },
      pattern: drawVelocity(base, lane.id, at, laneValue(e)),
    };
    strokeRef.current = s;
    setStroke(s);
  };
  const onLaneMove = (e: PointerEvent<HTMLButtonElement>, at: number) => {
    const s = strokeRef.current;
    if (!velocityDrag.current || !s || !lane) return;
    const next = { ...s, pattern: drawVelocity(s.pattern, lane.id, at, laneValue(e)) };
    strokeRef.current = next;
    setStroke(next);
  };
  const endLane = () => {
    velocityDrag.current = false;
    endStroke();
  };

  const perBar = pattern.stepsPerBeat * tempo.numerator;
  const orderStep = selectedStep === null ? undefined : part.order[selectedStep];
  const barsLong = Math.max(1, Math.round(pattern.length / perBar));
  const lengthOptions = [1, 2, 3, 4, 6, 8].filter((b) => b * perBar <= 128);

  return (
    <section
      aria-label="Drum designer"
      className="grid min-h-0 flex-1 grid-rows-[auto_auto_minmax(0,1fr)] gap-3"
    >
      <header className="@container flex min-w-0 items-center gap-2">
        <span className="caption shrink-0">Drum designer</span>
        <Lcd className="min-w-0 max-w-80 truncate" title={`${track?.name} · ${kitName}`}>
          {track?.name ?? "Drums"} · {kitName}
          {design.kitEdit?.changed ? " (changed)" : ""}
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
          tier="primary"
          disabled={part.order.length === 0}
          onClick={() => {
            if (design.trackClips === 0) report(drumDesignPlace("replace"));
            else setPlacing(true);
          }}
          title={`Put the part on ${track?.name ?? "the track"} as clips, one edit you can undo in the project`}
        >
          Place
        </Button>
        <Button
          tight
          onClick={() => setSaving(true)}
          title="Save the whole part as a groove in your library, for the palette and for jams"
        >
          <span className="@4xl:inline hidden">Save as groove…</span>
          <span className="@4xl:hidden">Save…</span>
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
        onDropPattern={(name, path) => takePattern(path, name)}
      />

      <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_minmax(14rem,18rem)] gap-3">
        <div
          className={`grid min-h-0 gap-2 ${padsOpen ? "grid-rows-[auto_auto_minmax(0,1fr)]" : "grid-rows-[auto_minmax(0,1fr)]"}`}
        >
          <div className="@container flex min-w-0 items-center gap-2">
            <div role="tablist" aria-label="Variations" className="flex shrink-0 gap-0.5">
              {[base, ...(section.variations ?? [])].map((_, i) =>
                i === 0 || i <= (section.variations?.length ?? 0) ? (
                  <Button
                    key={VARIATIONS[i]}
                    role="tab"
                    square
                    aria-selected={i === variantIndex}
                    aria-label={`Variation ${VARIATIONS[i]}`}
                    title={`Edit variation ${VARIATIONS[i]}`}
                    onClick={() => setVariant(i)}
                  >
                    {VARIATIONS[i]}
                  </Button>
                ) : null,
              )}
            </div>
            <Button
              square
              aria-label="Add a variation"
              title={`Add a variation: a copy of ${VARIATIONS[variantIndex]} to change, up to D`}
              disabled={(section.variations?.length ?? 0) >= 3}
              onClick={() => {
                commit(withSection(part, section.name, (s) => addVariation(s, variantIndex)));
                setVariant((section.variations?.length ?? 0) + 1);
              }}
            >
              +
            </Button>
            {variantIndex > 0 ? (
              <Button
                tier="quiet"
                tight
                title={`Remove variation ${VARIATIONS[variantIndex]}`}
                onClick={() => {
                  commit(withSection(part, section.name, (s) => removeVariation(s, variantIndex)));
                  setVariant(variantIndex - 1);
                }}
              >
                <span className="@3xl:inline hidden">Remove {VARIATIONS[variantIndex]}</span>
                <span className="@3xl:hidden">×</span>
              </Button>
            ) : null}
            <span className="flex-1" />
            <Button
              tight
              aria-pressed={padsOpen}
              onClick={() => setPadsOpen(!padsOpen)}
              title="The kit as pads: play them, and record a part in"
            >
              Pads
            </Button>
            {/* The bar tools drop out of a narrow grid; [ and ] still shift. */}
            <span className="@2xl:flex hidden shrink-0 items-center gap-2">
              <label className="flex shrink-0 items-center gap-2">
                <span className="caption @2xl:inline hidden">Bar</span>
                <span className="relative">
                  <select
                    aria-label="Bar the bar tools work on"
                    className="surface-field min-h-8 w-16 appearance-none py-0 pr-6 pl-2"
                    value={barAt}
                    onChange={(e) => setBar(Number(e.target.value))}
                  >
                    {barOptions.map(({ b, key }) => (
                      <option key={key} value={b}>
                        {b + 1}
                      </option>
                    ))}
                  </select>
                  <svg
                    aria-hidden="true"
                    viewBox="0 0 16 16"
                    className="pointer-events-none absolute top-1/2 right-2 size-3 -translate-y-1/2 fill-field-fg"
                  >
                    <path d="M2 5l6 6 6-6z" />
                  </svg>
                </span>
              </label>
              <Button
                tight
                title={`Copy bar ${barAt + 1}, every pad`}
                onClick={() =>
                  setCopied(
                    copyBar(
                      base,
                      pads.map((p) => p.id),
                      barAt,
                      numerator,
                    ),
                  )
                }
              >
                Copy
              </Button>
              <Button
                tight
                disabled={!copied}
                title={`Paste the copied bar over bar ${barAt + 1}`}
                onClick={() => {
                  if (copied) edit(pasteBar(base, copied, barAt, numerator));
                }}
              >
                Paste
              </Button>
              <Button
                tight
                disabled={pattern.length + barSteps(pattern, numerator) > 128}
                title={`Play bar ${barAt + 1} twice: a copy goes in after it`}
                onClick={() => {
                  const next = duplicateBar(base, barAt, numerator);
                  if (next) edit(next);
                }}
              >
                <span className="@3xl:inline hidden">Duplicate</span>
                <span className="@3xl:hidden">Dup</span>
              </Button>
              <Button
                tight
                title={`Clear bar ${barAt + 1}`}
                onClick={() => edit(clearBar(base, barAt, numerator))}
              >
                Clear
              </Button>
              <Button
                square
                aria-label="Shift earlier"
                title="Shift every hit a step earlier, round to the end ([)"
                onClick={() => edit(shiftSteps(base, -1))}
              >
                ‹
              </Button>
              <Button
                square
                aria-label="Shift later"
                title="Shift every hit a step later, round to the start (])"
                onClick={() => edit(shiftSteps(base, 1))}
              >
                ›
              </Button>
            </span>
          </div>
          {padsOpen ? (
            <div className="grid gap-1">
              <div className="flex min-w-0 items-center gap-2">
                <Button
                  aria-pressed={recording}
                  disabled={!status.running}
                  onClick={toggleRecord}
                  title={`Loop ${section.name} and record what you play into it, after a count-in, quantized to the step and overdubbed, a take a pass`}
                >
                  <Led on={recording && status.transport.playing} label="Recording" tone="record" />
                  Record
                </Button>
                <Button
                  tier="quiet"
                  disabled={takeDepth === null || takeDepth !== past.length}
                  onClick={undo}
                  title="Take back the last pass you recorded"
                >
                  Undo take
                </Button>
                <span role="status" className="min-w-0 truncate text-body">
                  {recording
                    ? frame.countIn > 0
                      ? "Counting in…"
                      : status.transport.playing
                        ? `Recording into ${section.name}, ${VARIATIONS[recordingVariant] ?? "A"}`
                        : "Starting…"
                    : "Play the pads, or click here and press A S D F…"}
                </span>
              </div>
              <DesignPads pads={design.pads} onHit={hitPad} />
            </div>
          ) : null}
          <div className="surface-lcd scroll-area min-h-0 p-2">
            {pads.length > 0 ? (
              <table
                ref={grid}
                aria-label={`${section.name} ${VARIATIONS[variantIndex]}: ${pads.length} pads by ${pattern.length} steps. Arrows move, Space or Return toggles a step, G ghosts it, A accents it.`}
                className="border-separate border-spacing-0"
              >
                <tbody>
                  {pads.map((pad, row) => (
                    <tr key={pad.id}>
                      <th
                        scope="row"
                        className="sticky left-0 z-[1] w-28 max-w-28 bg-lcd-bg p-0 pr-2 text-left font-normal"
                      >
                        <button
                          type="button"
                          tabIndex={-1}
                          aria-pressed={lane?.id === pad.id}
                          title={`Draw ${pad.name}'s velocities in the lane below`}
                          onClick={() => setLanePad(pad.id)}
                          className="lcd-text block w-full cursor-pointer truncate border-0 bg-transparent p-0 text-left font-lcd text-lcd-fg text-readout aria-pressed:underline"
                        >
                          {pad.name}
                        </button>
                      </th>
                      {steps.map(({ s, key }) => {
                        const v = valueAt(pad.id, s);
                        const beat = isBeatStart(s, pattern);
                        const now = step === s;
                        const m = pattern.rows
                          .find((r) => r.pad === pad.id)
                          ?.marks?.find((x) => x.step === s);
                        const label = m ? markLabel(m) : "";
                        return (
                          <td key={key} className={`p-0 ${beat ? "pl-2" : "pl-0.5"}`}>
                            <button
                              type="button"
                              data-row={row}
                              data-step={s}
                              tabIndex={row === 0 && s === 0 ? 0 : -1}
                              aria-pressed={v > 0}
                              aria-label={`${pad.name}, step ${s + 1}${v > 0 ? `, ${Math.round(v * 100)}%${label ? `, ${label}` : ""}` : ""}`}
                              onPointerDown={(e) => onCellDown(e, row, s)}
                              onPointerEnter={() => onCellEnter(row, s)}
                              onPointerUp={endStroke}
                              onPointerCancel={endStroke}
                              onClick={(e) => {
                                // A click from the keyboard: pointer clicks were painted.
                                if (e.detail === 0) toggleCell(row, s);
                              }}
                              onKeyDownCapture={(e) => onCellKeyCapture(e, row, s)}
                              onKeyDown={(e) => onCellKey(e, row, s)}
                              onFocus={() => setLanePad(pad.id)}
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
                                    opacity: (0.45 + v * 0.55) * (m?.chance ?? 1),
                                  }}
                                />
                              ) : null}
                              {v > 0 && label ? (
                                <span
                                  aria-hidden="true"
                                  className="absolute top-0 left-0.5 font-bold font-lcd text-label text-lcd-fg leading-none mix-blend-difference"
                                >
                                  {label}
                                </span>
                              ) : null}
                            </button>
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                  {lane ? (
                    <tr>
                      <th
                        scope="row"
                        className="lcd-text sticky left-0 z-[1] w-28 max-w-28 truncate bg-lcd-bg pt-2 pr-2 text-left align-top font-lcd font-normal text-label"
                        title={`${lane.name}'s velocities: drag across to draw them`}
                      >
                        Velocity · {lane.name}
                      </th>
                      {steps.map(({ s, key }) => {
                        const v = valueAt(lane.id, s);
                        const beat = isBeatStart(s, pattern);
                        return (
                          <td key={key} className={`p-0 pt-2 ${beat ? "pl-2" : "pl-0.5"}`}>
                            <button
                              type="button"
                              tabIndex={-1}
                              aria-label={`${lane.name} step ${s + 1} velocity${v > 0 ? `, ${Math.round(v * 100)}%` : ", no hit"}`}
                              onPointerDown={(e) => onLaneDown(e, s)}
                              onPointerMove={(e) => onLaneMove(e, s)}
                              onPointerEnter={(e) => onLaneMove(e, s)}
                              onPointerUp={endLane}
                              onPointerCancel={endLane}
                              className={`relative block h-16 w-8 cursor-ns-resize touch-none border-0 p-0 ${beat ? "bg-tl-lane" : "bg-tl-bg"}`}
                            >
                              {v > 0 ? (
                                <span
                                  aria-hidden="true"
                                  className="absolute inset-x-1 bottom-0 block bg-lcd-fg"
                                  style={{ height: `${Math.round(v * 100)}%` }}
                                />
                              ) : null}
                            </button>
                          </td>
                        );
                      })}
                    </tr>
                  ) : null}
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
        </div>

        <aside
          aria-label={panel === "palette" ? "Palette" : "Inspector"}
          className="surface-panel scroll-area grid min-h-0 content-start gap-3 p-3"
        >
          <div role="tablist" aria-label="Side panel" className="flex gap-0.5">
            <Button
              role="tab"
              aria-selected={panel === "inspector"}
              className="flex-1"
              onClick={() => setPanel("inspector")}
            >
              Inspector
            </Button>
            <Button
              role="tab"
              aria-selected={panel === "palette"}
              className="flex-1"
              onClick={() => setPanel("palette")}
              title="The stock grooves, your grooves and your patterns"
            >
              Palette
            </Button>
            <Button
              role="tab"
              aria-selected={panel === "kit"}
              className="flex-1"
              onClick={() => setPanel("kit")}
              title="The kit: hear each pad, tune it, swap its samples, save it"
            >
              Kit
            </Button>
          </div>
          {panel === "kit" ? (
            <KitWorkbench
              kitId={part.kit}
              kits={status.kits}
              pads={design.pads}
              edit={design.kitEdit}
              onStatus={onStatus}
              onError={onError}
            />
          ) : null}
          {panel === "palette" ? (
            <DesignPalette
              grooves={status.grooves}
              mine={status.library.grooves}
              patterns={status.library.patterns}
              meter={tempo}
              editing={section.name}
              onUsePattern={(path) => takePattern(path, section.name)}
              onUseGenre={takeGenre}
              onOpenGroove={openGroove}
            />
          ) : null}
          {panel === "inspector" && orderStep && selectedStep !== null ? (
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
          {panel === "inspector" ? (
            <>
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
                value={pattern.length % perBar === 0 ? barsLong : ""}
                onChange={(e) => editAll((p) => resizePattern(p, Number(e.target.value) * perBar))}
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
                  onChange={(e) => editAll((p) => ({ ...p, swing: Number(e.target.value) }))}
                  className="w-full"
                />
              </Field>
              {(["timing", "velocity"] as const).map((k) => {
                const h = pattern.humanize ?? { timing: 0, velocity: 0 };
                return (
                  <Field
                    key={k}
                    label={k === "timing" ? "Humanize timing" : "Humanize velocity"}
                    hint={`${Math.round(h[k] * 100)}%`}
                    htmlFor={`${ids}-humanize-${k}`}
                  >
                    <input
                      id={`${ids}-humanize-${k}`}
                      type="range"
                      min={0}
                      max={1}
                      step={0.05}
                      value={h[k]}
                      onChange={(e) =>
                        editAll((p) =>
                          withPatternFeel(p, {
                            ...(p.humanize ?? { timing: 0, velocity: 0 }),
                            [k]: Number(e.target.value),
                          }),
                        )
                      }
                      className="w-full"
                    />
                  </Field>
                );
              })}
              {section.variations?.length ? (
                <Select
                  id={`${ids}-vary`}
                  label="Variations"
                  hint={
                    section.vary === "chain"
                      ? "One after another, a pass each"
                      : "A between each of the others"
                  }
                  value={section.vary ?? "alternate"}
                  onChange={(e) =>
                    commit(
                      withSection(part, section.name, (s) => ({
                        ...s,
                        vary: e.target.value as "alternate" | "chain",
                      })),
                    )
                  }
                >
                  <option value="alternate">
                    Alternate:{" "}
                    {passes({ ...section, vary: "alternate" })
                      .map((i) => VARIATIONS[i])
                      .join(" ")}
                  </option>
                  <option value="chain">
                    Chain:{" "}
                    {passes({ ...section, vary: "chain" })
                      .map((i) => VARIATIONS[i])
                      .join(" ")}
                  </option>
                </Select>
              ) : null}
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
                        commit(
                          setFill(part, section.name, { ...fill, every: Number(e.target.value) }),
                        );
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
            </>
          ) : null}
        </aside>
      </div>

      {placing ? (
        <Sheet
          title={`Place the part on ${track?.name ?? "the track"}`}
          onClose={() => setPlacing(false)}
          footer={
            <>
              <Button onClick={() => setPlacing(false)}>Cancel</Button>
              <Button
                tier="primary"
                onClick={() => {
                  setPlacing(false);
                  report(drumDesignPlace(placeMode));
                }}
              >
                Place
              </Button>
            </>
          }
        >
          <div className="grid gap-3">
            <p className="m-0 text-body">
              The order goes on the track as clips, a pattern for each section, with the kit. It is
              one edit: Undo in {status.project.name} takes all of it back.
            </p>
            <fieldset className="m-0 grid gap-2 border-0 p-0">
              <legend className="caption mb-2">Where</legend>
              <Button
                aria-pressed={placeMode === "replace"}
                onClick={() => setPlaceMode("replace")}
                className="justify-start"
              >
                {design.placed
                  ? "In place of the part placed before"
                  : `In place of the track's ${design.trackClips} clip${design.trackClips === 1 ? "" : "s"}`}
              </Button>
              <Button
                aria-pressed={placeMode === "append"}
                onClick={() => setPlaceMode("append")}
                className="justify-start"
              >
                After the track's last clip
              </Button>
            </fieldset>
          </div>
        </Sheet>
      ) : null}

      {saving ? (
        <SaveGrooveSheet
          name={grooveName}
          mine={status.library.grooves.map((g) => g.name)}
          onClose={() => setSaving(false)}
          onStatus={onStatus}
          onError={onError}
        />
      ) : null}

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
