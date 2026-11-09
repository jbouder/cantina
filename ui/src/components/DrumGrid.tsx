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
import { createPortal } from "react-dom";
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
  brokenOf,
  choosePatternExportPath,
  choosePatternImportPaths,
  type DrumRows,
  drumRowsSet,
  type EngineStatus,
  type KitInfo,
  kitsRescan,
  type LibraryStatus,
  type PatternInfo,
  patternAdd,
  patternAudition,
  patternClipAdd,
  patternExport,
  patternImport,
  patternRemove,
  patternRename,
  patternReplace,
  patternSetLength,
  patternSetResolution,
  patternSetStep,
  patternSetSwing,
  RESOLUTIONS,
  type StockGroove,
  type Tempo,
  type TrackStatus,
  trackSetKit,
} from "../lib/engine";
import {
  type Cells,
  clearRegion,
  clearRow,
  copyRegion,
  doublePattern,
  duplicateRegion,
  halvePattern,
  inRegion,
  pasteCells,
  type Region,
  regionOf,
  setCells,
  shiftRegion,
  strokeCells,
} from "../lib/gridEdit";
import { dropPatternAt, setPatternDrag, usePatternDrag } from "../lib/pattern-drag";
import { DRAG_THRESHOLD } from "../lib/reorder";
import { Button } from "./Button";
import { GrooveBrowser } from "./GrooveBrowser";
import { Led } from "./Led";
import { Select } from "./Select";

/** Grooves in genre order, as the shell sorts them, grouped by genre. */
function groupByGenre(grooves: StockGroove[]): [string, StockGroove[]][] {
  const groups = new Map<string, StockGroove[]>();
  for (const g of grooves) groups.set(g.genre, [...(groups.get(g.genre) ?? []), g]);
  return [...groups];
}

type Props = {
  /** A drum track: `track.kit` is set. */
  track: TrackStatus;
  patterns: PatternInfo[];
  kits: KitInfo[];
  /** The library folders: pattern files to import, and files that will not load. */
  library: LibraryStatus;
  /** The stock grooves, offered in the pattern menu by genre. */
  grooves: StockGroove[];
  /** This track's rows muted or soloed for listening. */
  rows: DrumRows;
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
  startX: number;
  startY: number;
  /** Decided by the first movement: up and down sets the velocity, across paints, ⇧ selects. */
  mode: "undecided" | "velocity" | "paint" | "select";
  /** What a paint stroke writes: a hit, or nothing when it started on one. */
  paintValue: number;
  /** The cells a paint stroke has crossed, by "pad:step". */
  painted: Map<string, { pad: string; step: number; value: number }>;
  /** The last cell the stroke reached, by row and step. */
  last: { row: number; step: number };
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
  library,
  grooves,
  rows,
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
  // `?grooves` opens the groove browser, for screenshots.
  const [browsing, setBrowsing] = useState(() =>
    new URLSearchParams(window.location.search).has("grooves"),
  );
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
  // `?gridselect=top,bottom,from,to` starts with cells selected, for screenshots.
  const [selection, setSelection] = useState<Region | null>(() => {
    const [top, bottom, from, to] = (
      new URLSearchParams(window.location.search).get("gridselect") ?? ""
    )
      .split(",")
      .map(Number);
    return [top, bottom, from, to].every((n) => Number.isInteger(n))
      ? { top: top as number, bottom: bottom as number, from: from as number, to: to as number }
      : null;
  });
  /** Where a ⇧-arrow selection grows from. */
  const anchor = useRef<{ row: number; step: number } | null>(null);
  /** What Copy kept: the grid's own clipboard, not the system's. */
  const clipboard = useRef<Cells | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the fields follow the pattern shown
  useEffect(() => {
    setName(pattern?.name ?? "");
    setSwing(pattern?.swing ?? 0);
    setEditing(false);
  }, [pattern?.id, pattern?.name, pattern?.swing]);

  // A selection belongs to the pattern it was made in.
  const selectedIn = useRef(pattern?.id);
  useEffect(() => {
    if (selectedIn.current !== pattern?.id) setSelection(null);
    selectedIn.current = pattern?.id;
  }, [pattern?.id]);

  const pads = track.kit?.pads ?? [];
  const brokenKits = brokenOf(library, "kit");
  const brokenPatterns = brokenOf(library, "pattern");

  /** Imports pattern files and shows the last one. */
  const importPaths = (paths: string[]) => {
    if (paths.length === 0) return;
    paths
      .reduce<Promise<EngineStatus | null>>(
        (done, path) => done.then(() => patternImport(path, track.id)),
        Promise.resolve(null),
      )
      .then(
        (s) => {
          if (!s) return;
          onStatus(s);
          setChosen(s.project.patterns[s.project.patterns.length - 1]?.id ?? null);
        },
        (e) => onError(String(e)),
      );
  };
  const importFiles = () =>
    choosePatternImportPaths(library.patternsDir).then(importPaths, (e) => onError(String(e)));
  const exportPattern = () => {
    if (!pattern) return;
    choosePatternExportPath(pattern.name, library.patternsDir).then(
      (path) => {
        if (path) patternExport(pattern.id, path).catch((e) => onError(String(e)));
      },
      (e) => onError(String(e)),
    );
  };

  const step = pattern
    ? currentStep(position, tempo, pattern, playing && auditioning !== null)
    : null;

  // ----- cells -----

  const cellValue = (pad: string, s: number) => {
    const painted = drag?.mode === "paint" ? drag.painted.get(`${pad}:${s}`) : undefined;
    if (painted) return painted.value;
    if (drag && drag.mode === "velocity" && drag.pad === pad && drag.step === s) return drag.value;
    return pattern ? velocityAt(pattern, pad, s) : 0;
  };
  const padIds = pads.map((p) => p.id);
  const rowOf = (pad: string) => padIds.indexOf(pad);
  /** Sends a whole new pattern: one gesture, one step of undo. */
  const replace = (next: PatternInfo | null) => {
    if (next) run(patternReplace(next));
  };
  /** The cell under a point, from the buttons' data attributes. */
  const cellAt = (x: number, y: number) => {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-pad][data-step]");
    const pad = el?.dataset.pad;
    const step = Number(el?.dataset.step);
    return pad !== undefined && Number.isFinite(step) ? { pad, row: rowOf(pad), step } : null;
  };
  /** The selection, or the focused cell as a region of one. */
  const target = (): Region | null => {
    if (selection) return selection;
    if (!focused) return null;
    const row = rowOf(focused.pad);
    return row < 0 ? null : regionOf({ row, step: focused.step }, { row, step: focused.step });
  };
  const toggleRow = (pad: string, kind: "muted" | "soloed") => {
    const list = rows[kind];
    const next = list.includes(pad) ? list.filter((p) => p !== pad) : [...list, pad];
    run(drumRowsSet(track.id, { ...rows, [kind]: next }));
  };
  /** A row muted, or left out by a solo. */
  const silent = (pad: string) =>
    rows.muted.includes(pad) || (rows.soloed.length > 0 && !rows.soloed.includes(pad));

  // Copy, Cut and Paste are the system's: with a cell focused they act on
  // the selection (or the cell), kept in the grid's own clipboard, and
  // Paste writes it with its corner at the focused cell.
  const edits = useRef<Record<"copy" | "cut" | "paste", () => boolean>>({
    copy: () => false,
    cut: () => false,
    paste: () => false,
  });
  edits.current = {
    copy: () => {
      const r = target();
      if (!pattern || !r) return false;
      clipboard.current = copyRegion(pattern, padIds, r);
      return true;
    },
    cut: () => {
      const r = target();
      if (!pattern || !r) return false;
      clipboard.current = copyRegion(pattern, padIds, r);
      replace(clearRegion(pattern, padIds, r));
      return true;
    },
    paste: () => {
      if (!pattern || !focused || !clipboard.current) return false;
      replace(pasteCells(pattern, padIds, clipboard.current, rowOf(focused.pad), focused.step));
      return true;
    },
  };
  useEffect(() => {
    const inGrid = () => grid.current?.contains(document.activeElement) ?? false;
    const handlers: [string, (e: Event) => void][] = [
      ["beforecopy", (e) => inGrid() && e.preventDefault()],
      ["beforecut", (e) => inGrid() && e.preventDefault()],
      ["beforepaste", (e) => inGrid() && clipboard.current !== null && e.preventDefault()],
      ["copy", (e) => inGrid() && edits.current.copy() && e.preventDefault()],
      ["cut", (e) => inGrid() && edits.current.cut() && e.preventDefault()],
      ["paste", (e) => inGrid() && edits.current.paste() && e.preventDefault()],
    ];
    for (const [type, handler] of handlers) document.addEventListener(type, handler);
    return () => {
      for (const [type, handler] of handlers) document.removeEventListener(type, handler);
    };
  }, []);

  const onCellDown = (e: PointerEvent<HTMLButtonElement>, pad: string, s: number) => {
    if (e.button !== 0 || !pattern) return;
    const was = velocityAt(pattern, pad, s);
    const row = rowOf(pad);
    const d: Drag = {
      pad,
      step: s,
      startX: e.clientX,
      startY: e.clientY,
      mode: e.shiftKey ? "select" : "undecided",
      from: was > 0 ? was : DEFAULT_VELOCITY,
      value: was,
      moved: false,
      paintValue: was > 0 ? 0 : DEFAULT_VELOCITY,
      painted: new Map(),
      last: { row, step: s },
    };
    if (e.shiftKey) {
      // ⇧-click and drag selects, from the focused cell when there is one.
      const from = focused && !selection ? { row: rowOf(focused.pad), step: focused.step } : null;
      anchor.current = from ?? anchor.current ?? { row, step: s };
      setSelection(regionOf(anchor.current, { row, step: s }));
    }
    dragRef.current = d;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onCellMove = (e: PointerEvent<HTMLButtonElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    let next = d;
    if (d.mode === "undecided") {
      if (Math.abs(dy) >= 4 && Math.abs(dy) > Math.abs(dx)) next = { ...d, mode: "velocity" };
      else if (Math.abs(dx) >= 6) {
        const first = { pad: d.pad, step: d.step, value: d.paintValue };
        next = { ...d, mode: "paint", painted: new Map([[`${d.pad}:${d.step}`, first]]) };
      } else return;
    }
    if (next.mode === "velocity") {
      next = { ...next, value: draggedVelocity(next.from, dy), moved: true };
    } else {
      const cell = cellAt(e.clientX, e.clientY);
      if (cell && cell.row >= 0) {
        if (next.mode === "select") {
          setSelection(regionOf(anchor.current ?? next.last, cell));
        } else {
          const painted = new Map(next.painted);
          for (const c of strokeCells(next.last, cell)) {
            const pad = padIds[c.row];
            if (pad) painted.set(`${pad}:${c.step}`, { pad, step: c.step, value: next.paintValue });
          }
          next = { ...next, painted, last: { row: cell.row, step: cell.step } };
        }
      }
      next = { ...next, moved: true };
    }
    dragRef.current = next;
    setDrag(next);
  };

  const onCellUp = (pad: string, s: number) => {
    const d = dragRef.current;
    dragRef.current = null;
    setDrag(null);
    if (!d || !pattern) return;
    if (d.mode === "select") return;
    if (d.mode === "paint") {
      replace(setCells(pattern, [...d.painted.values()]));
      return;
    }
    setSelection(null);
    anchor.current = null;
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
    const here = { row: padIndex, step: s };
    // ⇧← and ⇧→ (and ⌥⇧↑, ⌥⇧↓) grow the selection from where it started.
    const grow = (to: { row: number; step: number }) => {
      anchor.current ??= here;
      const row = Math.max(0, Math.min(pads.length - 1, to.row));
      const step = Math.max(0, Math.min(pattern.length - 1, to.step));
      setSelection(regionOf(anchor.current, { row, step }));
      focusCell(pads[row]?.id ?? pad, step);
    };
    const region = target();
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (!(e.metaKey || e.ctrlKey)) {
      let done = true;
      if (e.shiftKey && key === "ArrowLeft") grow({ row: padIndex, step: s - 1 });
      else if (e.shiftKey && key === "ArrowRight") grow({ row: padIndex, step: s + 1 });
      else if (e.shiftKey && e.altKey && key === "ArrowUp") grow({ row: padIndex - 1, step: s });
      else if (e.shiftKey && e.altKey && key === "ArrowDown") grow({ row: padIndex + 1, step: s });
      else if (key === "Escape" && selection) {
        setSelection(null);
        anchor.current = null;
      } else if ((key === "Backspace" || key === "Delete") && e.altKey) {
        replace(clearRow(pattern, pad));
      } else if ((key === "Backspace" || key === "Delete") && region) {
        replace(clearRegion(pattern, padIds, region));
      } else if (key === "[" || key === "]") {
        // Shifts the selection, or the focused cell's whole row.
        const r = selection ?? { top: padIndex, bottom: padIndex, from: 0, to: pattern.length - 1 };
        replace(shiftRegion(pattern, padIds, r, key === "]" ? 1 : -1));
      } else if (key === "d" && !e.shiftKey) {
        replace(selection ? duplicateRegion(pattern, padIds, selection) : doublePattern(pattern));
      } else if (key === "h" && !e.shiftKey) {
        replace(halvePattern(pattern));
      } else if (key === "m" && !e.shiftKey) {
        toggleRow(pad, "muted");
      } else if (key === "s" && !e.shiftKey) {
        toggleRow(pad, "soloed");
      } else done = false;
      if (done) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }
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

  // Dragging the place key carries the pattern to the timeline; a press
  // that never passes the threshold is a click and places at the playhead.
  const patternDrag = usePatternDrag();
  const press = useRef<{ x0: number; y0: number; moved: boolean } | null>(null);
  const dragged = useRef(false);
  useEffect(() => {
    if (!patternDrag) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") {
        // Canceled: the release that follows neither drops nor places.
        press.current = null;
        dragged.current = true;
        setPatternDrag(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [patternDrag]);

  return (
    <section aria-label={`${track.name} drum grid`} className="grid gap-2">
      {patternDrag
        ? createPortal(
            <div
              aria-hidden="true"
              style={{ left: patternDrag.x + 12, top: patternDrag.y + 12 }}
              className="surface-lcd pointer-events-none fixed z-50 px-2 py-1 font-lcd text-base"
            >
              <span className="lcd-text">{patternDrag.name}</span>
            </div>,
            document.body,
          )
        : null}
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
          {brokenKits.length > 0 ? (
            <optgroup label="Will not load">
              {brokenKits.map((b) => (
                <option key={b.path} value={`broken:${b.path}`} disabled title={b.reason}>
                  {b.name}: {b.reason}
                </option>
              ))}
            </optgroup>
          ) : null}
        </Select>
        <Button onClick={() => run(kitsRescan())} title="Look for kits installed since launch">
          Rescan
        </Button>
        <Select
          id={`${id}-pattern`}
          label="Pattern"
          value={pattern?.id ?? ""}
          onChange={(e) => {
            const value = e.target.value;
            if (value.startsWith("file:")) importPaths([value.slice(5)]);
            else setChosen(Number(value));
          }}
          disabled={
            patterns.length === 0 &&
            library.patterns.length === 0 &&
            grooves.length === 0 &&
            brokenPatterns.length === 0
          }
        >
          {patterns.length === 0 ? <option value="">No pattern</option> : null}
          {patterns.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
          {library.patterns.length > 0 ? (
            <optgroup label="Import from the library">
              {library.patterns.map((p) => (
                <option key={p.path} value={`file:${p.path}`}>
                  {p.name}
                </option>
              ))}
            </optgroup>
          ) : null}
          {groupByGenre(grooves).map(([genre, list]) => (
            <optgroup key={genre} label={`Add a groove: ${genre}`}>
              {list.map((g) => (
                <option key={g.path} value={`file:${g.path}`}>
                  {g.name} ({g.tempo.min}–{g.tempo.max} bpm)
                </option>
              ))}
            </optgroup>
          ))}
          {brokenPatterns.length > 0 ? (
            <optgroup label="Will not load">
              {brokenPatterns.map((b) => (
                <option key={b.path} value={`broken:${b.path}`} disabled title={b.reason}>
                  {b.name}: {b.reason}
                </option>
              ))}
            </optgroup>
          ) : null}
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
        <Button
          onClick={importFiles}
          title="Add .pattern.json files to the project; files in your Patterns folder are also in the pattern menu"
        >
          Import…
        </Button>
        <Button
          aria-pressed={browsing}
          aria-expanded={browsing}
          variant={browsing ? "transport" : "panel"}
          onClick={() => setBrowsing(!browsing)}
          title="Browse the stock grooves: filter, hear and add them"
        >
          Grooves
        </Button>
        {pattern ? (
          <>
            <Button onClick={exportPattern} title="Write this pattern to a .pattern.json file">
              Export…
            </Button>
            {editing ? (
              <form onSubmit={rename} className="flex items-center gap-2">
                <label htmlFor={`${id}-name`} className="sr-only">
                  Pattern name
                </label>
                <input
                  id={`${id}-name`}
                  // biome-ignore lint/a11y/noAutofocus: the field appears on request, under the pointer
                  autoFocus
                  className="surface-lcd min-h-8 w-40 px-2 font-lcd text-base"
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
              className={`touch-none select-none ${patternDrag ? "cursor-grabbing" : "cursor-grab"}`}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                e.currentTarget.setPointerCapture(e.pointerId);
                press.current = { x0: e.clientX, y0: e.clientY, moved: false };
              }}
              onPointerMove={(e) => {
                const p = press.current;
                if (!p) return;
                const far = Math.hypot(e.clientX - p.x0, e.clientY - p.y0) > DRAG_THRESHOLD;
                if (!p.moved && !far) return;
                p.moved = true;
                setPatternDrag({ id: pattern.id, name: pattern.name, x: e.clientX, y: e.clientY });
              }}
              onPointerUp={(e) => {
                const p = press.current;
                press.current = null;
                if (!p?.moved) return;
                // A drag ends in a click on the key; that click is not a place.
                dragged.current = true;
                setPatternDrag(null);
                dropPatternAt(pattern.id, e.clientX, e.clientY);
              }}
              onPointerCancel={() => {
                press.current = null;
                setPatternDrag(null);
              }}
              onClick={() => {
                if (dragged.current) {
                  dragged.current = false;
                  return;
                }
                run(patternClipAdd(track.id, pattern.id, Math.round(ticksAt(position, tempo))));
              }}
              title="Place the pattern on this track at the playhead, or drag it onto the timeline; drag the clip's right edge to loop it for more bars"
            >
              Place at playhead
            </Button>
          </>
        ) : null}
      </div>
      {browsing ? (
        <GrooveBrowser
          track={track.id}
          grooves={grooves}
          audition={audition}
          onStatus={onStatus}
          onError={onError}
          onAdded={setChosen}
        />
      ) : null}
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
              className="surface-lcd min-h-8 w-20 px-2 text-center font-lcd text-base tabular-nums"
            />
            <span className="text-ink-dim">{lengthLabel(pattern, tempo)}</span>
            <Button
              disabled={pattern.length * 2 > 128}
              onClick={() => replace(doublePattern(pattern))}
              title="Double the pattern: play it twice in a row, twice as long (D with no selection)"
              aria-label="Double the pattern"
              className="normal-case!"
            >
              ×2
            </Button>
            <Button
              disabled={pattern.length < 2}
              onClick={() => replace(halvePattern(pattern))}
              title="Halve the pattern: keep the first half (H)"
              aria-label="Halve the pattern"
              className="normal-case!"
            >
              ½
            </Button>
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
            aria-label={`${pattern.name}: ${pads.length} pads by ${pattern.length} steps. Space toggles a step, Shift with the up and down arrows sets its velocity, Shift with the left and right arrows selects.${selection ? ` ${selection.bottom - selection.top + 1} by ${selection.to - selection.from + 1} selected.` : ""}`}
            className="border-separate border-spacing-0"
          >
            <tbody>
              {pads.map((pad, padIndex) => (
                <tr key={pad.id}>
                  <th
                    scope="row"
                    className={`lcd-text w-24 max-w-24 truncate pr-2 text-left font-lcd font-normal text-base ${silent(pad.id) ? "opacity-40" : ""}`}
                    title={pad.name}
                  >
                    {pad.name}
                  </th>
                  <td className="p-0 pr-1">
                    <span className="flex gap-0.5">
                      <button
                        type="button"
                        tabIndex={-1}
                        aria-pressed={rows.muted.includes(pad.id)}
                        aria-label={`Mute ${pad.name} (M)`}
                        title={`Mute ${pad.name} while you listen (M)`}
                        onClick={() => toggleRow(pad.id, "muted")}
                        className={`h-6 w-5 border-0 p-0 font-bold font-lcd text-xs ${rows.muted.includes(pad.id) ? "bg-lcd-fg text-lcd-bg" : "bg-tl-lane text-lcd-fg"}`}
                      >
                        M
                      </button>
                      <button
                        type="button"
                        tabIndex={-1}
                        aria-pressed={rows.soloed.includes(pad.id)}
                        aria-label={`Solo ${pad.name} (S)`}
                        title={`Hear ${pad.name} alone while you listen (S)`}
                        onClick={() => toggleRow(pad.id, "soloed")}
                        className={`h-6 w-5 border-0 p-0 font-bold font-lcd text-xs ${rows.soloed.includes(pad.id) ? "bg-lcd-fg text-lcd-bg" : "bg-tl-lane text-lcd-fg"}`}
                      >
                        S
                      </button>
                      <button
                        type="button"
                        tabIndex={-1}
                        aria-label={`Clear ${pad.name} (⌥⌫)`}
                        title={`Clear every step of ${pad.name} (⌥⌫)`}
                        onClick={() => replace(clearRow(pattern, pad.id))}
                        className="h-6 w-5 border-0 bg-tl-lane p-0 font-lcd text-lcd-fg text-xs"
                      >
                        ×
                      </button>
                    </span>
                  </td>
                  {steps.map(({ s, key }) => {
                    const v = cellValue(pad.id, s);
                    const beat = isBeatStart(s, pattern);
                    const now = step === s;
                    const picked = inRegion(selection, padIndex, s);
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
                          } ${now ? "outline outline-1 outline-tl-playhead" : ""} ${
                            picked ? "outline-2 outline-focus outline-solid -outline-offset-2" : ""
                          } ${silent(pad.id) ? "opacity-40" : ""}`}
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
