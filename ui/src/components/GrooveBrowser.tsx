import { type KeyboardEvent, type PointerEvent, useId, useMemo, useRef, useState } from "react";
import {
  type AuditionStatus,
  type EngineStatus,
  type GrooveSection,
  grooveAudition,
  patternImport,
  type StockGroove,
} from "../lib/engine";
import { FEELS, filterGrooves, meterOf } from "../lib/grooves";
import { dropPatternAt, overPatternDropTarget, setPatternDrag } from "../lib/pattern-drag";
import { DRAG_THRESHOLD } from "../lib/reorder";
import { Button } from "./Button";
import { Led } from "./Led";
import { Select } from "./Select";

type Props = {
  /** The drum track grooves are heard on and added for. */
  track: number;
  grooves: StockGroove[];
  audition: AuditionStatus | null;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
  /** A groove was added to the project as this pattern. */
  onAdded: (pattern: number) => void;
};

const SECTIONS: GrooveSection[] = ["intro", "verse", "chorus", "bridge", "fill", "ending"];

/**
 * The stock groove library in the Beats tab: filter by genre, section and
 * time signature, search, loop a groove on the track's kit at the
 * project's tempo to hear it, and add it, with a button or by dragging
 * it onto the timeline. The list is one tab stop: the arrow keys move
 * through it, Space loops the groove, Return adds it, Escape stops.
 */
export function GrooveBrowser({ track, grooves, audition, onStatus, onError, onAdded }: Props) {
  const id = useId();
  const [genre, setGenre] = useState("");
  const [section, setSection] = useState("");
  const [meter, setMeter] = useState("");
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const press = useRef<{ x0: number; y0: number; groove: StockGroove; moved: boolean } | null>(
    null,
  );

  const genres = useMemo(() => [...new Set(grooves.map((g) => g.genre))], [grooves]);
  const meters = useMemo(() => [...new Set(grooves.map(meterOf))].sort(), [grooves]);
  const shown = filterGrooves(grooves, genre, section, meter, query);
  const current = shown[Math.min(active, shown.length - 1)];
  const looping = audition?.track === track ? audition.groove : null;

  const fail = (e: unknown) => onError(String(e));
  const loop = (g: StockGroove) =>
    grooveAudition(looping === g.path ? null : track, looping === g.path ? null : g.path).then(
      onStatus,
      fail,
    );
  const stop = () => {
    if (looping) grooveAudition(null, null).then(onStatus, fail);
  };
  /** Adds the groove; resolves to the new pattern's id. */
  const add = (g: StockGroove) =>
    patternImport(g.path, track).then((status) => {
      onStatus(status);
      const added = status.project.patterns[status.project.patterns.length - 1];
      if (added) onAdded(added.id);
      return added?.id ?? null;
    });

  const move = (to: number) => {
    const next = Math.max(0, Math.min(shown.length - 1, to));
    setActive(next);
    list.current
      ?.querySelector<HTMLElement>(`[data-index="${next}"]`)
      ?.scrollIntoView({ block: "nearest" });
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    switch (e.key) {
      case "ArrowDown":
        move(active + 1);
        break;
      case "ArrowUp":
        move(active - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(shown.length - 1);
        break;
      case "PageDown":
        move(active + 8);
        break;
      case "PageUp":
        move(active - 8);
        break;
      case " ":
        if (current) loop(current);
        break;
      case "Enter":
        if (current) add(current).catch(fail);
        break;
      case "Escape":
        stop();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  // A row dragged past the threshold carries the groove to the timeline,
  // which adds it and places it where it lands.
  const down = (e: PointerEvent<HTMLDivElement>, g: StockGroove) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    press.current = { x0: e.clientX, y0: e.clientY, groove: g, moved: false };
  };
  const moveDrag = (e: PointerEvent<HTMLDivElement>) => {
    const p = press.current;
    if (!p) return;
    const far = Math.hypot(e.clientX - p.x0, e.clientY - p.y0) > DRAG_THRESHOLD;
    if (!p.moved && !far) return;
    p.moved = true;
    setPatternDrag({ id: -1, name: p.groove.name, x: e.clientX, y: e.clientY });
  };
  const cancel = () => {
    press.current = null;
    setPatternDrag(null);
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    const p = press.current;
    press.current = null;
    if (!p?.moved) return;
    setPatternDrag(null);
    const { clientX: x, clientY: y } = e;
    if (!overPatternDropTarget(x, y)) return;
    add(p.groove).then((added) => {
      if (added !== null) dropPatternAt(added, x, y);
    }, fail);
  };

  return (
    <section aria-label="Grooves" className="grid gap-2">
      <div className="flex flex-wrap items-end gap-x-4 gap-y-2">
        <label htmlFor={`${id}-search`} className="sr-only">
          Search grooves
        </label>
        <input
          id={`${id}-search`}
          type="search"
          placeholder="Search grooves"
          autoComplete="off"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          className="surface-field min-h-8 w-48 px-3 "
        />
        <Select
          id={`${id}-genre`}
          label="Genre"
          value={genre}
          onChange={(e) => {
            setGenre(e.target.value);
            setActive(0);
          }}
        >
          <option value="">All genres</option>
          {genres.map((g) => (
            <option key={g} value={g}>
              {g}
            </option>
          ))}
        </Select>
        <Select
          id={`${id}-section`}
          label="Section"
          value={section}
          onChange={(e) => {
            setSection(e.target.value);
            setActive(0);
          }}
          className="min-w-32"
        >
          <option value="">Any</option>
          {SECTIONS.map((s) => (
            <option key={s} value={s}>
              {s[0]?.toUpperCase()}
              {s.slice(1)}
            </option>
          ))}
        </Select>
        <Select
          id={`${id}-meter`}
          label="Time"
          value={meter}
          onChange={(e) => {
            setMeter(e.target.value);
            setActive(0);
          }}
          className="min-w-24"
        >
          <option value="">Any</option>
          {meters.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </Select>
        {looping ? (
          <Button onClick={stop} title="Stop looping the groove (Esc)">
            <Led on label="Looping" />
            Stop
          </Button>
        ) : null}
      </div>
      <div
        ref={list}
        role="listbox"
        tabIndex={0}
        aria-label={`Grooves: ${shown.length} of ${grooves.length}`}
        aria-activedescendant={current ? `${id}-g-${shown.indexOf(current)}` : undefined}
        onKeyDown={onKey}
        className="surface-lcd max-h-56 overflow-y-auto p-1"
      >
        {shown.length === 0 ? (
          <p className="m-0 px-2 py-1 font-lcd text-readout text-lcd-dim">
            <span className="lcd-text">No grooves match.</span>
          </p>
        ) : null}
        {shown.map((g, i) => {
          const isLooping = looping === g.path;
          const isActive = current === g;
          return (
            // biome-ignore lint/a11y/useKeyWithClickEvents: the listbox handles the keys for its options
            <div
              key={g.path}
              id={`${id}-g-${i}`}
              data-index={i}
              role="option"
              aria-selected={isActive}
              tabIndex={-1}
              onPointerDown={(e) => down(e, g)}
              onPointerMove={moveDrag}
              onPointerUp={up}
              onPointerCancel={cancel}
              onClick={() => setActive(i)}
              onDoubleClick={() => add(g).catch(fail)}
              className={`grid cursor-grab grid-cols-[auto_1fr_auto_auto] items-center gap-2 rounded px-2 py-0.5 font-lcd text-readout ${isActive ? "outline-2 outline-focus outline-solid -outline-offset-2" : ""}`}
            >
              <Led on={isLooping} label={isLooping ? "Looping" : "Not looping"} />
              <span className="lcd-text truncate">{g.name}</span>
              <span className="lcd-text text-body">
                {g.tempo.min}–{g.tempo.max} bpm · {FEELS[g.feel]} · {meterOf(g)}
              </span>
              <span className="flex gap-1">
                <Button
                  tabIndex={-1}
                  aria-pressed={isLooping}
                  aria-label={`Loop ${g.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    setActive(i);
                    loop(g);
                  }}
                  className="min-h-7 px-2"
                >
                  Loop
                </Button>
                <Button
                  tabIndex={-1}
                  aria-label={`Add ${g.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    add(g).catch(fail);
                  }}
                  className="min-h-7 px-2"
                >
                  Add
                </Button>
              </span>
            </div>
          );
        })}
      </div>
    </section>
  );
}
