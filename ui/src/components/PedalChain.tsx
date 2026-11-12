import { type PointerEvent, useCallback, useEffect, useId, useRef, useState } from "react";
import {
  chainMove,
  chainRemove,
  chainSetParam,
  chainSetSpec,
  type EngineStatus,
  editorClose,
  editorOpen,
  type IrInfo,
  irsRescan,
  ms,
  type PedalInfo,
  type PedalKindInfo,
  pedalSetBypass,
} from "../lib/engine";
import { DRAG_THRESHOLD, dropIndex } from "../lib/reorder";
import { Button } from "./Button";
import { Fader } from "./Fader";
import { Led } from "./Led";
import { PedalReadout } from "./PedalReadout";

type Props = {
  chain: PedalInfo[];
  sampleRate: number;
  /** The stock pedals the picker offers. */
  kinds: PedalKindInfo[];
  /** The latest frame's pedal readouts by pedal id (see readouts.ts). */
  readouts: Map<number, number[]>;
  /** The cabinet impulse responses installed. */
  irs: IrInfo[];
  /** Opens the picker, which adds at the end of this chain. */
  onAdd: () => void;
  /** Opens the designer on a designed pedal. */
  onDesign: (pedal: number) => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The kind a pedal in a chain was added as, if it is a stock one. */
function kindOf(kinds: PedalKindInfo[], spec: string) {
  const base = spec.startsWith("cab:") ? "cab" : spec;
  return kinds.find((k) => k.spec === base);
}

/** Where a pedal comes from, for its name's tooltip. */
function origin(kind: PedalKindInfo | undefined, designed: boolean): string {
  if (kind?.stock)
    return kind.designable ? "Stock pedal, in the pedal format" : "Stock pedal, built in";
  return designed ? "Your pedal, designed here" : "Plugin";
}

/** A card being dragged by its header. */
type Drag = {
  id: number;
  from: number;
  /** Where the press started, and how far the pointer has gone since. */
  x0: number;
  dx: number;
  /** Past the threshold: a drag, not a click. */
  moved: boolean;
  /** Where it would land if let go now. */
  over: number;
};

/** The pointer handlers that make a card's header its grip. */
export type Grip = {
  onPointerDown: (e: PointerEvent<HTMLElement>) => void;
  onPointerMove: (e: PointerEvent<HTMLElement>) => void;
  onPointerUp: (e: PointerEvent<HTMLElement>) => void;
  onPointerCancel: () => void;
};

/**
 * The pedals in order, each with its knobs. Reorder by dragging a card
 * by its header or with the arrow keys on its name; the free slot at the
 * end opens the picker.
 *
 * Dragging uses pointer events, not HTML drag and drop: on macOS the
 * shell's file-drop handler claims every drop, so the webview never
 * fires `drop` (see #158).
 */
export function PedalChain({
  chain,
  sampleRate,
  kinds,
  readouts,
  irs,
  onAdd,
  onDesign,
  onStatus,
  onError,
}: Props) {
  const [drag, setDragState] = useState<Drag | null>(null);
  // The handlers read the latest drag without waiting for a render.
  const dragRef = useRef<Drag | null>(null);
  const setDrag = useCallback((d: Drag | null) => {
    dragRef.current = d;
    setDragState(d);
  }, []);
  const items = useRef<(HTMLLIElement | null)[]>([]);

  const run = useCallback(
    (action: Promise<EngineStatus>) => {
      action.then(onStatus, (e) => onError(String(e)));
    },
    [onStatus, onError],
  );

  // Escape puts a dragged card back.
  const dragging = drag?.moved ?? false;
  useEffect(() => {
    if (!dragging) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setDrag(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dragging, setDrag]);

  const grip = (id: number, index: number): Grip => ({
    onPointerDown: (e) => {
      // The header's own keys (on/off, remove) are clicks, not grips.
      if (e.button !== 0 || (e.target as Element).closest("[data-no-grip]")) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      setDrag({ id, from: index, x0: e.clientX, dx: 0, moved: false, over: index });
    },
    onPointerMove: (e) => {
      const d = dragRef.current;
      if (d?.id !== id) return;
      const dx = e.clientX - d.x0;
      const moved = d.moved || Math.abs(dx) > DRAG_THRESHOLD;
      const spans = items.current.slice(0, chain.length).map((el) => {
        const r = el?.getBoundingClientRect();
        return { left: r?.left ?? 0, right: r?.right ?? 0 };
      });
      const over = moved ? dropIndex(e.clientX, spans) : d.from;
      setDrag({ ...d, dx, moved, over: over < 0 ? d.from : over });
    },
    onPointerUp: () => {
      const d = dragRef.current;
      if (d?.id !== id) return;
      setDrag(null);
      if (d.moved && d.over !== d.from) run(chainMove(d.id, d.over));
    },
    onPointerCancel: () => setDrag(null),
  });

  return (
    <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-2">
      <ol className="scroll-area m-0 flex min-h-0 min-w-0 flex-1 list-none items-stretch gap-2 p-0 pb-1">
        {chain.map((pedal, index) => (
          <li
            key={pedal.id}
            ref={(el) => {
              items.current[index] = el;
            }}
            className={`flex min-h-0 ${drag?.moved && drag.over === index && drag.id !== pedal.id ? "outline-2 outline-focus" : ""}`}
          >
            <PedalCard
              pedal={pedal}
              kind={kindOf(kinds, pedal.spec)}
              readouts={readouts.get(pedal.id)}
              irs={irs}
              sampleRate={sampleRate}
              index={index}
              count={chain.length}
              offset={drag?.moved && drag.id === pedal.id ? drag.dx : null}
              grip={grip(pedal.id, index)}
              onMove={(to) => run(chainMove(pedal.id, to))}
              onRemove={() => run(chainRemove(pedal.id))}
              onBypass={() => run(pedalSetBypass(pedal.id, !pedal.bypassed))}
              onEditor={(open) => run(open ? editorOpen(pedal.id) : editorClose(pedal.id))}
              onSpec={(spec) => run(chainSetSpec(pedal.id, spec))}
              onRescanIrs={() => run(irsRescan())}
              onDesign={() => onDesign(pedal.id)}
              onError={onError}
            />
          </li>
        ))}
        {chain.length === 0 ? (
          <li className="self-center text-ink-dim text-body">
            No pedals: the signal goes straight through.
          </li>
        ) : null}
        {/* Right after the last pedal, as wide as a pedal, so it scrolls
            with the chain. */}
        <li className="flex min-h-0 shrink-0">
          <button
            type="button"
            aria-haspopup="dialog"
            onClick={onAdd}
            title="Add a stock pedal, one of yours, a plugin, a new design or a file"
            className="grid min-h-24 w-64 cursor-pointer place-content-center justify-items-center gap-1 rounded-control border-2 border-frame-hi border-dashed bg-transparent p-2 text-ink-dim hover:border-ink hover:text-ink"
          >
            <span aria-hidden="true" className="font-lcd text-display leading-none">
              +
            </span>
            <span className="font-bold text-label">Add a pedal</span>
          </button>
        </li>
      </ol>
    </div>
  );
}

type CardProps = {
  pedal: PedalInfo;
  /** The stock kind it was added as, if any. */
  kind: PedalKindInfo | undefined;
  readouts: number[] | undefined;
  irs: IrInfo[];
  sampleRate: number;
  index: number;
  count: number;
  /** How far the card is dragged from its place, or null when it is not. */
  offset: number | null;
  grip: Grip;
  onMove: (to: number) => void;
  onRemove: () => void;
  /** Turns the pedal off (bypassed) or back on. */
  onBypass: () => void;
  /** Opens or closes the plugin's own editor window. */
  onEditor: (open: boolean) => void;
  /** Swaps the pedal's spec: a cabinet's impulse response. */
  onSpec: (spec: string) => void;
  onRescanIrs: () => void;
  /** Opens the designer on this pedal. */
  onDesign: () => void;
  onError: (message: string) => void;
};

function PedalCard({
  pedal,
  kind,
  readouts,
  irs,
  sampleRate,
  index,
  count,
  offset,
  grip,
  onMove,
  onRemove,
  onBypass,
  onEditor,
  onSpec,
  onRescanIrs,
  onDesign,
  onError,
}: CardProps) {
  const designed = pedal.spec.startsWith("pedal:");
  // A cabinet picks its impulse response; the choice lives in its spec.
  const cabinetIr =
    pedal.spec === "cab" ? "" : pedal.spec.startsWith("cab:") ? pedal.spec.slice(4) : null;
  const irId = useId();
  // Knob values live here between status pushes, since set_param is
  // not echoed back. A push does carry them, and may bring values a
  // plugin changed on its own (its editor, a preset), so it wins.
  const [values, setValues] = useState(() => pedal.params.map((p) => p.value));
  useEffect(() => {
    setValues(pedal.params.map((p) => p.value));
  }, [pedal.params]);
  // A pedal with many knobs gets a double-width card, so none is tall.
  const wide = pedal.params.length > 6;
  const pending = useRef<Map<number, number>>(new Map());
  const scheduled = useRef(false);

  // A drag sends at most one value per parameter per frame.
  const setParam = (param: number, value: number) => {
    setValues((v) => v.map((x, i) => (i === param ? value : x)));
    pending.current.set(param, value);
    if (scheduled.current) return;
    scheduled.current = true;
    requestAnimationFrame(() => {
      scheduled.current = false;
      const batch = Array.from(pending.current.entries());
      pending.current.clear();
      for (const [p, v] of batch) {
        chainSetParam(pedal.id, p, v).catch((e) => onError(String(e)));
      }
    });
  };

  return (
    <article
      aria-label={`${pedal.name} pedal`}
      style={offset === null ? undefined : { transform: `translateX(${offset}px)` }}
      className={`surface-panel scroll-area grid max-h-full shrink-0 content-start gap-1 p-2 [scrollbar-gutter:auto] ${wide ? "w-128" : "w-64"} ${offset === null ? "" : "relative z-10"}`}
    >
      <header
        {...grip}
        className={`flex min-w-0 touch-none select-none items-center gap-1 ${offset === null ? "cursor-grab" : "cursor-grabbing"}`}
      >
        <button
          type="button"
          data-no-grip
          aria-pressed={!pedal.bypassed}
          aria-label={`${pedal.name} on`}
          title={pedal.bypassed ? "Off: the signal goes round it. Turn it on" : "On. Turn it off"}
          onClick={onBypass}
          className="grid size-6 shrink-0 cursor-pointer place-items-center rounded-control border-0 bg-transparent p-0"
        >
          <Led on={!pedal.bypassed} label={pedal.name} />
        </button>
        <h3 className="m-0 min-w-0 flex-1 font-bold text-body">
          <span className="flex items-center gap-1">
            <button
              type="button"
              aria-label={`${pedal.name}, position ${index + 1} of ${count}. Drag the card's header or use the arrow keys to move it.`}
              title={`${origin(kind, designed)}. Drag to reorder; the arrow keys move it too`}
              onKeyDown={(e) => {
                if (e.key === "ArrowLeft" && index > 0) onMove(index - 1);
                if (e.key === "ArrowRight" && index < count - 1) onMove(index + 1);
              }}
              className="min-h-6 min-w-0 flex-1 cursor-[inherit] truncate border-0 bg-transparent p-0 text-left text-inherit"
            >
              {pedal.name}
            </button>
          </span>
          {pedal.latencyFrames > 0 ? (
            <small
              className="hint block text-ink-dim"
              title="Delay this pedal adds; other tracks are delayed to match"
            >
              {ms(pedal.latencyFrames, sampleRate)} latency
            </small>
          ) : null}
        </h3>
        <button
          type="button"
          data-no-grip
          aria-label={`Remove ${pedal.name}`}
          title="Remove"
          onClick={onRemove}
          className="grid size-6 shrink-0 cursor-pointer place-items-center rounded-control border-0 bg-transparent p-0 text-ink-dim hover:text-ink"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-2.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </button>
      </header>
      <div className={`grid min-w-0 content-start gap-1 ${pedal.bypassed ? "opacity-50" : ""}`}>
        {cabinetIr !== null ? (
          <div className="grid min-w-0 grid-cols-[5.5rem_minmax(0,1fr)_auto] items-center gap-2">
            <label htmlFor={irId} className="caption">
              Impulse
            </label>
            <span className="relative min-w-0">
              <select
                id={irId}
                className="surface-field min-h-6 w-full appearance-none truncate py-0 pr-6 pl-2"
                value={cabinetIr}
                onChange={(e) => onSpec(`cab:${e.target.value}`)}
              >
                {cabinetIr === "" || !irs.some((ir) => ir.id === cabinetIr) ? (
                  <option value={cabinetIr}>
                    {cabinetIr === "" ? "None" : `${cabinetIr} (not installed)`}
                  </option>
                ) : null}
                {irs.map((ir) => (
                  <option key={ir.id} value={ir.id}>
                    {ir.name}
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
            <Button
              tier="quiet"
              square
              aria-label="Rescan impulse responses"
              onClick={onRescanIrs}
              title="Look for impulse responses added to ~/Music/Cantina/IRs"
              className="min-h-6"
            >
              <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
                <path d="M8 2a6 6 0 0 1 5.2 3H11v1.5h4.5V2H14v1.7A7.5 7.5 0 0 0 .6 6.7l1.5.3A6 6 0 0 1 8 2zm5.9 7A6 6 0 0 1 2.8 11H5V9.5H.5V14H2v-1.7a7.5 7.5 0 0 0 13.4-3z" />
              </svg>
            </Button>
          </div>
        ) : null}
        <PedalReadout pedal={pedal} values={readouts} />
        {designed ? (
          <Button
            aria-haspopup="dialog"
            onClick={onDesign}
            title="Open this pedal in the designer"
            className="min-h-6"
          >
            Edit…
          </Button>
        ) : null}
        {pedal.hasEditor ? (
          <Button
            aria-pressed={pedal.editorOpen}
            onClick={() => onEditor(!pedal.editorOpen)}
            className="min-h-6"
            title={
              pedal.editorOpen
                ? "Close the plugin's own window"
                : "Open the plugin's own window; the knobs below follow it"
            }
          >
            <Led on={pedal.editorOpen} label="Editor open" />
            {pedal.editorOpen ? "Editor open" : "Edit…"}
          </Button>
        ) : null}
        <div className={wide ? "grid min-w-0 grid-cols-2 gap-x-4 gap-y-1" : "contents"}>
          {pedal.params.map((param, i) => (
            <Fader
              key={`${String(i)}-${param.name}`}
              param={param}
              value={values[i] ?? param.value}
              onChange={(v) => setParam(i, v)}
            />
          ))}
        </div>
      </div>
    </article>
  );
}
