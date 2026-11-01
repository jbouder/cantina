import { type DragEvent, useCallback, useEffect, useId, useRef, useState } from "react";
import {
  chainAdd,
  chainMove,
  chainRemove,
  chainSetParam,
  chainSetSpec,
  chooseImportPaths,
  type EngineStatus,
  editorClose,
  editorOpen,
  type IrInfo,
  irsRescan,
  ms,
  type PedalInfo,
  type PedalKindInfo,
  pedalImport,
} from "../lib/engine";
import { Button } from "./Button";
import { Fader } from "./Fader";
import { Led } from "./Led";
import { PedalReadout } from "./PedalReadout";
import { Select } from "./Select";

type Props = {
  /** The track whose chain this is. */
  track: number;
  chain: PedalInfo[];
  sampleRate: number;
  /** The stock pedals the picker offers. */
  kinds: PedalKindInfo[];
  /** The latest frame's pedal readouts by pedal id (see readouts.ts). */
  readouts: Map<number, number[]>;
  /** The cabinet impulse responses installed. */
  irs: IrInfo[];
  /** Opens the plugin browser for this track. */
  onBrowsePlugins: () => void;
  /** Opens the designer on a designed pedal, or starts a new design. */
  onDesign: (pedal: number | null) => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** The kind a pedal in a chain was added as, if it is a stock one. */
function kindOf(kinds: PedalKindInfo[], spec: string) {
  const base = spec.startsWith("cab:") ? "cab" : spec;
  return kinds.find((k) => k.spec === base);
}

/** A small tag in the frame's lettering: where a pedal comes from. */
function Badge({ children, title }: { children: string; title: string }) {
  return (
    <span
      title={title}
      className="surface-lcd shrink-0 px-1 py-0.5 font-lcd text-[11px] uppercase leading-none tracking-[0.08em]"
    >
      <span className="lcd-text">{children}</span>
    </span>
  );
}

/**
 * The pedals in order, each with its knobs. Reorder by dragging a card
 * or with the arrow keys on its grip; add a stock pedal from the picker
 * at the end, or a third-party plugin from the browser.
 */
export function PedalChain({
  track,
  chain,
  sampleRate,
  kinds,
  readouts,
  irs,
  onBrowsePlugins,
  onDesign,
  onStatus,
  onError,
}: Props) {
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const [kind, setKind] = useState(kinds[0]?.spec ?? "gain");
  const stock = kinds.filter((k) => k.stock);
  const library = kinds.filter((k) => !k.stock);
  const chosen = kinds.find((k) => k.spec === kind);

  const importFiles = () => {
    chooseImportPaths().then(
      (paths) => {
        for (const path of paths) run(pedalImport(path, track));
      },
      (e) => onError(String(e)),
    );
  };

  const run = useCallback(
    (action: Promise<EngineStatus>) => {
      action.then(onStatus, (e) => onError(String(e)));
    },
    [onStatus, onError],
  );

  const onDrop = (e: DragEvent, index: number) => {
    e.preventDefault();
    if (dragging !== null) run(chainMove(dragging, index));
    setDragging(null);
    setOver(null);
  };

  return (
    <div className="grid gap-2">
      <ol className="m-0 flex list-none gap-2 overflow-x-auto p-0 pb-1">
        {chain.map((pedal, index) => (
          <li
            key={pedal.id}
            onDragOver={(e) => {
              e.preventDefault();
              setOver(index);
            }}
            onDragLeave={() => setOver((o) => (o === index ? null : o))}
            onDrop={(e) => onDrop(e, index)}
            className={over === index && dragging !== pedal.id ? "outline-2 outline-focus" : ""}
          >
            <PedalCard
              pedal={pedal}
              kind={kindOf(kinds, pedal.spec)}
              readouts={readouts.get(pedal.id)}
              irs={irs}
              sampleRate={sampleRate}
              index={index}
              count={chain.length}
              dragging={dragging === pedal.id}
              onDragStart={() => setDragging(pedal.id)}
              onDragEnd={() => {
                setDragging(null);
                setOver(null);
              }}
              onMove={(to) => run(chainMove(pedal.id, to))}
              onRemove={() => run(chainRemove(pedal.id))}
              onEditor={(open) => run(open ? editorOpen(pedal.id) : editorClose(pedal.id))}
              onSpec={(spec) => run(chainSetSpec(pedal.id, spec))}
              onRescanIrs={() => run(irsRescan())}
              onDesign={() => onDesign(pedal.id)}
              onError={onError}
            />
          </li>
        ))}
        {chain.length === 0 ? (
          <li className="self-center text-ink-dim text-sm">
            No pedals: the signal goes straight through.
          </li>
        ) : null}
      </ol>
      <div className="flex items-end gap-2">
        <Select
          id={`pedal-kind-${track}`}
          label="Add a pedal"
          value={kind}
          onChange={(e) => setKind(e.target.value)}
        >
          <optgroup label="Stock">
            {stock.map((k) => (
              <option key={k.spec} value={k.spec}>
                {k.name}
              </option>
            ))}
          </optgroup>
          {library.length > 0 ? (
            <optgroup label="Your pedals">
              {library.map((k) => (
                <option key={k.spec} value={k.spec}>
                  {k.name}
                </option>
              ))}
            </optgroup>
          ) : null}
        </Select>
        {chosen ? (
          <span className="flex min-h-11 items-center">
            <Badge
              title={
                !chosen.stock
                  ? "From your library, in the pedal format"
                  : chosen.designable
                    ? "Ships with Cantina, written in the pedal format: the designer can open it"
                    : "Ships with Cantina, built in"
              }
            >
              {!chosen.stock ? "yours" : chosen.designable ? "stock · json" : "stock"}
            </Badge>
          </span>
        ) : null}
        <Button onClick={() => run(chainAdd(track, kind))}>Add</Button>
        <Button aria-haspopup="dialog" onClick={onBrowsePlugins}>
          Plugins…
        </Button>
        <Button
          aria-haspopup="dialog"
          onClick={() => onDesign(null)}
          title="Start a new pedal at the end of the chain and open the designer on it"
        >
          Design…
        </Button>
        <Button
          aria-haspopup="dialog"
          onClick={importFiles}
          title="Copy a .pedal.json file into your library and add it to this chain; dropping one on the window does the same"
        >
          Import…
        </Button>
      </div>
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
  dragging: boolean;
  onDragStart: () => void;
  onDragEnd: () => void;
  onMove: (to: number) => void;
  onRemove: () => void;
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
  dragging,
  onDragStart,
  onDragEnd,
  onMove,
  onRemove,
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
  // A pedal with many knobs (the EQ) gets a double-width card.
  const wide = pedal.params.length > 8;
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
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = "move";
        onDragStart();
      }}
      onDragEnd={onDragEnd}
      className={`surface-panel grid shrink-0 gap-2 p-3 ${wide ? "w-96" : "w-48"} ${dragging ? "opacity-50" : ""}`}
    >
      <header className="flex items-center gap-1">
        <button
          type="button"
          aria-label={`${pedal.name}, position ${index + 1} of ${count}. Use the arrow keys to move it.`}
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft" && index > 0) onMove(index - 1);
            if (e.key === "ArrowRight" && index < count - 1) onMove(index + 1);
          }}
          className="surface-pbutton grid size-11 shrink-0 cursor-grab place-items-center border-0 p-0"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-4 fill-current">
            <path d="M5 3h2v2H5zM9 3h2v2H9zM5 7h2v2H5zM9 7h2v2H9zM5 11h2v2H5zM9 11h2v2H9z" />
          </svg>
        </button>
        <h3 className="m-0 flex-1 truncate font-bold text-[11px] uppercase tracking-[0.1em]">
          {pedal.name}
          {kind?.stock ? (
            <span className="mt-0.5 block">
              <Badge title={kind.designable ? "Stock, in the pedal format" : "Stock, built in"}>
                {kind.designable ? "stock · json" : "stock"}
              </Badge>
            </span>
          ) : designed ? (
            <span className="mt-0.5 block">
              <Badge title="Designed here, in the pedal format">yours</Badge>
            </span>
          ) : null}
          {pedal.latencyFrames > 0 ? (
            <small
              className="block font-normal text-ink-dim normal-case tracking-normal"
              title="Delay this pedal adds; other tracks are delayed to match"
            >
              {ms(pedal.latencyFrames, sampleRate)} latency
            </small>
          ) : null}
        </h3>
        <Button aria-label={`Remove ${pedal.name}`} onClick={onRemove} className="px-2">
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </Button>
      </header>
      {cabinetIr !== null ? (
        <div className="grid gap-1">
          <label htmlFor={irId} className="font-bold text-[11px] uppercase tracking-[0.08em]">
            Impulse
          </label>
          <span className="relative">
            <select
              id={irId}
              className="surface-lcd min-h-11 w-full appearance-none truncate py-0 pr-8 pl-2 font-lcd text-base"
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
              className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 fill-lcd-fg"
            >
              <path d="M2 5l6 6 6-6z" />
            </svg>
          </span>
          <Button
            onClick={onRescanIrs}
            title="Look for impulse responses added to ~/Music/Cantina/IRs"
          >
            Rescan
          </Button>
        </div>
      ) : null}
      <PedalReadout pedal={pedal} values={readouts} />
      {designed ? (
        <Button aria-haspopup="dialog" onClick={onDesign} title="Open this pedal in the designer">
          Edit…
        </Button>
      ) : null}
      {pedal.hasEditor ? (
        <Button
          aria-pressed={pedal.editorOpen}
          onClick={() => onEditor(!pedal.editorOpen)}
          variant={pedal.editorOpen ? "transport" : "panel"}
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
      <div className={wide ? "grid grid-cols-2 gap-x-4 gap-y-2" : "contents"}>
        {pedal.params.map((param, i) => (
          <Fader
            key={`${String(i)}-${param.name}`}
            param={param}
            value={values[i] ?? param.value}
            onChange={(v) => setParam(i, v)}
          />
        ))}
      </div>
    </article>
  );
}
