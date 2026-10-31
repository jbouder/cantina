import { type DragEvent, useCallback, useRef, useState } from "react";
import {
  chainAdd,
  chainMove,
  chainRemove,
  chainSetParam,
  type EngineStatus,
  type PedalInfo,
} from "../lib/engine";
import { Button } from "./Button";
import { Fader } from "./Fader";
import { Select } from "./Select";

type Props = {
  /** The track whose chain this is. */
  track: number;
  chain: PedalInfo[];
  kinds: string[];
  /** Opens the plugin browser for this track. */
  onBrowsePlugins: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

function titleCase(spec: string) {
  return spec.charAt(0).toUpperCase() + spec.slice(1);
}

/**
 * The pedals in order, each with its knobs. Reorder by dragging a card
 * or with the arrow keys on its grip; add a stock pedal from the picker
 * at the end, or a third-party plugin from the browser.
 */
export function PedalChain({ track, chain, kinds, onBrowsePlugins, onStatus, onError }: Props) {
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const [kind, setKind] = useState(kinds[0] ?? "gain");

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
          {kinds.map((k) => (
            <option key={k} value={k}>
              {titleCase(k)}
            </option>
          ))}
        </Select>
        <Button onClick={() => run(chainAdd(track, kind))}>Add</Button>
        <Button aria-haspopup="dialog" onClick={onBrowsePlugins}>
          Plugins…
        </Button>
      </div>
    </div>
  );
}

type CardProps = {
  pedal: PedalInfo;
  index: number;
  count: number;
  dragging: boolean;
  onDragStart: () => void;
  onDragEnd: () => void;
  onMove: (to: number) => void;
  onRemove: () => void;
  onError: (message: string) => void;
};

function PedalCard({
  pedal,
  index,
  count,
  dragging,
  onDragStart,
  onDragEnd,
  onMove,
  onRemove,
  onError,
}: CardProps) {
  // Knob values live here between status pushes, since set_param is
  // not echoed back.
  const [values, setValues] = useState(() => pedal.params.map((p) => p.value));
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
      className={`surface-panel grid w-48 shrink-0 gap-2 p-3 ${dragging ? "opacity-50" : ""}`}
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
        </h3>
        <Button aria-label={`Remove ${pedal.name}`} onClick={onRemove} className="px-2">
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </Button>
      </header>
      {pedal.params.map((param, i) => (
        <Fader
          key={param.name}
          param={param}
          value={values[i] ?? param.value}
          onChange={(v) => setParam(i, v)}
        />
      ))}
    </article>
  );
}
