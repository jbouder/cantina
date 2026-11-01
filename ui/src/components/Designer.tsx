import {
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import {
  chainSetParam,
  chooseExportPath,
  designerSave,
  designerUpdate,
  type EngineStatus,
  inTauri,
  type ParamInfo,
  type PedalInfo,
  type PedalKindInfo,
  pedalCheck,
  pedalDefinition,
  pedalExport,
  pedalStockDefinition,
} from "../lib/engine";
import {
  addNode,
  type Binding,
  connect,
  disconnect,
  type FieldInfo,
  fieldsOf,
  freshParamId,
  isBinding,
  KINDS,
  kindOf,
  moveNode,
  type NodeDef,
  type ParamDef,
  type PedalDef,
  type Problem,
  paramRange,
  problemsByNode,
  removeNode,
  removeParam,
  renameNode,
  renameParam,
  setField,
  type Value,
  valuesFor,
} from "../lib/pedal";
import { Button } from "./Button";
import { Fader } from "./Fader";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { TextInput } from "./TextInput";

type Props = {
  /** The designed pedal in a chain, as the status describes it. */
  pedal: PedalInfo;
  /** The picker's kinds: the designable stock ones are starting points. */
  kinds: PedalKindInfo[];
  onClose: () => void;
  onStatus: (status: EngineStatus) => void;
  onError: (message: string) => void;
};

/** Node card geometry on the canvas, in CSS pixels. */
const CARD_W = 160;
const PORT_Y = 22;
const BIND_Y = 44;
const LABEL = "font-bold text-[11px] uppercase tracking-[0.08em]";
const params = inTauri ? null : new URLSearchParams(window.location.search);

type Selection = { type: "node"; id: string } | { type: "edge"; from: string; to: string } | null;

/**
 * The pedal designer: a node graph on a canvas, a side panel for the
 * selected node's fields and the pedal's parameters, and the pedal
 * itself running in the chain the whole time. Every edit is sent to
 * the engine; a definition with problems is shown inline and the last
 * valid pedal keeps playing.
 */
export function Designer({ pedal, kinds, onClose, onStatus, onError }: Props) {
  const titleId = useId();
  const [def, setDef] = useState<PedalDef | null>(null);
  const [problems, setProblems] = useState<Problem[]>([]);
  const [selected, setSelected] = useState<Selection>(() => {
    const id = params?.get("select");
    return id ? { type: "node", id } : null;
  });
  /** An output port waiting for an input to connect to. */
  const [pending, setPending] = useState<string | null>(null);
  const [adding, setAdding] = useState("gain");
  const [startFrom, setStartFrom] = useState("");
  // Parameter values by id, so a reshaped parameter list keeps them.
  const values = useRef<Map<string, number>>(new Map());
  const panel = useRef<HTMLDivElement>(null);

  // Load once for this pedal; later statuses come through `pedal`.
  // biome-ignore lint/correctness/useExhaustiveDependencies: loads on open
  useEffect(() => {
    pedalDefinition(pedal.id).then(
      (loaded) => {
        values.current = new Map(
          loaded.params.map((p, i) => [p.id, pedal.params[i]?.value ?? p.default]),
        );
        setDef(loaded);
        // A design left with problems shows them straight away.
        pedalCheck(loaded).then(setProblems, (e) => onError(String(e)));
      },
      (e) => onError(String(e)),
    );
  }, [pedal.id]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    panel.current?.querySelector<HTMLElement>("input, button, select")?.focus();
    const onKey = (e: KeyboardEvent | globalThis.KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      opener?.focus();
    };
  }, [onClose]);

  const commit = useCallback(
    (next: PedalDef) => {
      setDef(next);
      designerUpdate(pedal.id, next, valuesFor(next, values.current)).then(
        (result) => {
          setProblems(result.problems);
          if (result.problems.length === 0) onStatus(result.status);
        },
        (e) => onError(String(e)),
      );
    },
    [pedal.id, onStatus, onError],
  );

  if (!def) {
    return (
      <div className="absolute inset-0 z-10 flex items-center justify-center bg-scrim">
        <Lcd className="text-base">Loading the design…</Lcd>
      </div>
    );
  }

  const byNode = problemsByNode(problems);
  const designable = kinds.filter((k) => k.designable);
  const selectedNode =
    selected?.type === "node" ? def.nodes.find((n) => n.id === selected.id) : undefined;

  const load = () => {
    if (!startFrom) return;
    pedalStockDefinition(startFrom).then(
      (stock) => {
        setSelected(null);
        commit(stock);
      },
      (e) => onError(String(e)),
    );
  };

  const save = () => {
    designerSave(pedal.id).then(onStatus, (e) => onError(String(e)));
  };

  const exportFile = () => {
    chooseExportPath(def.name).then(
      (path) => {
        if (path) pedalExport(pedal.id, path).catch((e) => onError(String(e)));
      },
      (e) => onError(String(e)),
    );
  };

  const add = () => {
    const [next, id] = addNode(def, adding);
    setSelected({ type: "node", id });
    commit(next);
  };

  const remove = () => {
    if (selected?.type === "node") {
      const kind = kindOf(def.nodes.find((n) => n.id === selected.id)?.kind ?? "");
      if (kind?.kind === "input" || kind?.kind === "output") return;
      setSelected(null);
      commit(removeNode(def, selected.id));
    } else if (selected?.type === "edge") {
      setSelected(null);
      commit(disconnect(def, selected.from, selected.to));
    }
  };

  const onPortIn = (id: string) => {
    if (pending === null) return;
    setPending(null);
    setSelected({ type: "edge", from: pending, to: id });
    commit(connect(def, pending, id));
  };

  return (
    <div className="absolute inset-0 z-10 flex bg-scrim p-3">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="surface-panel relative flex min-h-0 w-full flex-col overflow-hidden"
      >
        <header className="surface-titlebar flex min-h-[30px] flex-wrap items-center gap-2 py-1 pr-1 pl-3">
          <h2 id={titleId} className="title-text m-0 font-normal text-[1.05rem]">
            Pedal designer
          </h2>
          <span className="title-text text-sm">· {def.name} · running in the chain</span>
          <span className="flex-1" />
          <Button
            onClick={save}
            title="Save to your library under a name made from the pedal's, and keep the project's own copy"
          >
            {pedal.spec.startsWith("pedal:draft-") ? "Save to library" : "Save"}
          </Button>
          <Button onClick={exportFile} title="Write the definition to a .pedal.json file to share">
            Export…
          </Button>
          <Button onClick={onClose} aria-label="Close the designer" className="px-2">
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
              <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
            </svg>
          </Button>
        </header>
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_22rem] grid-rows-[minmax(0,1fr)] gap-3 bg-sheet-bg p-3 text-sheet-ink">
          <section aria-label="Graph" className="flex min-h-0 min-w-0 flex-col gap-2">
            <div className="flex flex-wrap items-end gap-2">
              <Select
                id={`${titleId}-add`}
                label="Add a node"
                value={adding}
                onChange={(e) => setAdding(e.target.value)}
                className="min-w-36"
              >
                {KINDS.filter((k) => k.kind !== "input" && k.kind !== "output").map((k) => (
                  <option key={k.kind} value={k.kind}>
                    {k.label}
                  </option>
                ))}
              </Select>
              <Button onClick={add}>Add</Button>
              <Button
                onClick={remove}
                disabled={
                  !selected ||
                  (selected.type === "node" && (selected.id === "in" || selected.id === "out"))
                }
                title="Remove the selected node or wire (Delete)"
              >
                Remove
              </Button>
              <p className="m-0 text-ink-dim text-xs">
                {pending
                  ? `Wiring from ${pending}: click a node's input, or its output again to stop.`
                  : "Drag nodes to arrange them; click an output then an input to wire them."}
              </p>
            </div>
            <Canvas
              def={def}
              problems={byNode}
              selected={selected}
              pending={pending}
              onSelect={setSelected}
              onMove={(id, pos) => commit(moveNode(def, id, pos))}
              onPortOut={(id) => setPending((p) => (p === id ? null : id))}
              onPortIn={onPortIn}
              onDelete={remove}
            />
          </section>
          <aside
            ref={panel}
            className="surface-lcd grid min-h-0 content-start gap-3 overflow-y-auto p-3"
          >
            <TextInput
              id={`${titleId}-name`}
              label="Name"
              value={def.name}
              onChange={(e) => commit({ ...def, name: e.target.value })}
              className="min-w-0"
            />
            <div className="flex items-end gap-2">
              <Select
                id={`${titleId}-start`}
                label="Start from"
                value={startFrom}
                onChange={(e) => setStartFrom(e.target.value)}
                className="min-w-0"
              >
                <option value="">Choose a stock pedal…</option>
                {designable.map((k) => (
                  <option key={k.spec} value={k.spec}>
                    {k.name}
                  </option>
                ))}
              </Select>
              <Button
                onClick={load}
                disabled={!startFrom}
                title="Replaces the graph and parameters with the stock pedal's"
              >
                Load
              </Button>
            </div>
            {problems.length > 0 ? (
              <Lcd role="alert" error className="text-sm">
                <span className="lcd-text block">The last valid pedal keeps playing. To fix:</span>
                {problems.map((p) => (
                  <span
                    key={`${p.node ?? ""}:${p.field ?? ""}:${p.message}`}
                    className="lcd-text block"
                  >
                    {p.node ? `${p.node}${p.field ? `, ${p.field}` : ""}: ` : ""}
                    {p.message}
                  </span>
                ))}
              </Lcd>
            ) : null}
            {selectedNode ? (
              <NodePanel
                def={def}
                node={selectedNode}
                problems={byNode.get(selectedNode.id) ?? []}
                onRename={(to) => {
                  setSelected({ type: "node", id: to });
                  commit(renameNode(def, selectedNode.id, to));
                }}
                onField={(field, value) => commit(setField(def, selectedNode.id, field, value))}
              />
            ) : selected?.type === "edge" ? (
              <section aria-labelledby={`${titleId}-edge`} className="grid gap-2">
                <h3 id={`${titleId}-edge`} className={`m-0 ${LABEL}`}>
                  Wire {selected.from} → {selected.to}
                </h3>
                <Button onClick={remove}>Remove wire</Button>
              </section>
            ) : (
              <p className="m-0 text-ink-dim text-xs">
                Select a node to edit its fields. A field can be a constant, follow a parameter
                (with a range mapped onto it), or follow another node's signal.
              </p>
            )}
            <ParamsPanel
              def={def}
              live={pedal}
              onChange={commit}
              onValue={(id, index, value) => {
                values.current.set(id, value);
                chainSetParam(pedal.id, index, value).catch((e) => onError(String(e)));
              }}
            />
          </aside>
        </div>
      </div>
    </div>
  );
}

// ----- the canvas -----

type CanvasProps = {
  def: PedalDef;
  problems: Map<string, Problem[]>;
  selected: Selection;
  pending: string | null;
  onSelect: (selection: Selection) => void;
  onMove: (id: string, pos: [number, number]) => void;
  onPortOut: (id: string) => void;
  onPortIn: (id: string) => void;
  onDelete: () => void;
};

function posOf(node: NodeDef | undefined): [number, number] {
  return node?.pos ?? [0, 0];
}

/** A cubic from an output port to an input port. */
function wire(from: [number, number], to: [number, number], fromY: number, toY: number) {
  const x1 = from[0] + CARD_W;
  const y1 = from[1] + fromY;
  const x2 = to[0];
  const y2 = to[1] + toY;
  const bend = Math.max(40, Math.abs(x2 - x1) / 2);
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}

function Canvas({
  def,
  problems,
  selected,
  pending,
  onSelect,
  onMove,
  onPortOut,
  onPortIn,
  onDelete,
}: CanvasProps) {
  const [dragging, setDragging] = useState<{ id: string; pos: [number, number] } | null>(null);
  const drag = useRef<{
    id: string;
    start: [number, number];
    origin: [number, number];
    moved: boolean;
  } | null>(null);
  const width = Math.max(600, ...def.nodes.map((n) => posOf(n)[0] + CARD_W + 60));
  const height = Math.max(360, ...def.nodes.map((n) => posOf(n)[1] + 160));
  const at = (id: string): [number, number] =>
    dragging?.id === id ? dragging.pos : posOf(def.nodes.find((n) => n.id === id));

  const onPointerDown = (e: PointerEvent<HTMLButtonElement>, node: NodeDef) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = {
      id: node.id,
      start: [e.clientX, e.clientY],
      origin: posOf(node),
      moved: false,
    };
  };
  const onPointerMove = (e: PointerEvent<HTMLButtonElement>) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.start[0];
    const dy = e.clientY - d.start[1];
    if (!d.moved && Math.hypot(dx, dy) < 3) return;
    d.moved = true;
    setDragging({ id: d.id, pos: [Math.max(0, d.origin[0] + dx), Math.max(0, d.origin[1] + dy)] });
  };
  const onPointerUp = (e: PointerEvent<HTMLButtonElement>) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    if (d.moved && dragging) {
      setDragging(null);
      onMove(d.id, dragging.pos);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLButtonElement>, node: NodeDef) => {
    const step = e.shiftKey ? 1 : 10;
    const [x, y] = posOf(node);
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [Math.max(0, x - step), y],
      ArrowRight: [x + step, y],
      ArrowUp: [x, Math.max(0, y - step)],
      ArrowDown: [x, y + step],
    };
    const to = moves[e.key];
    if (to) {
      e.preventDefault();
      onMove(node.id, to);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      onDelete();
    }
  };

  return (
    <div className="relative min-h-0 flex-1 overflow-auto bg-tl-bg">
      <div className="relative" style={{ width, height }}>
        <svg
          aria-label="Wires"
          className="absolute inset-0"
          width={width}
          height={height}
          viewBox={`0 0 ${width} ${height}`}
        >
          {def.edges.map(([from, to]) => {
            const d = wire(at(from), at(to), PORT_Y, PORT_Y);
            const isSelected =
              selected?.type === "edge" && selected.from === from && selected.to === to;
            return (
              <g key={`${from}->${to}`}>
                <path
                  d={d}
                  fill="none"
                  className={isSelected ? "stroke-focus" : "stroke-glyph"}
                  strokeWidth={2}
                />
                {/* biome-ignore lint/a11y/useSemanticElements: an SVG path cannot be a button; the role and key handler make the wire selectable from the keyboard */}
                <path
                  d={d}
                  fill="none"
                  stroke="transparent"
                  strokeWidth={12}
                  role="button"
                  tabIndex={0}
                  aria-label={`Wire from ${from} to ${to}`}
                  className="pointer-events-auto cursor-pointer"
                  onClick={() => onSelect({ type: "edge", from, to })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onSelect({ type: "edge", from, to });
                    }
                  }}
                />
              </g>
            );
          })}
          {def.nodes.flatMap((node) =>
            Object.entries(fieldsOf(node))
              .filter(([, v]) => isBinding(v) && v.node && !v.param)
              .map(([field, v]) => {
                const source = (v as Binding).node as string;
                if (!def.nodes.some((n) => n.id === source)) return null;
                const d = wire(at(source), at(node.id), PORT_Y, BIND_Y);
                const end = at(node.id);
                return (
                  <g key={`${source}~${node.id}.${field}`}>
                    <path
                      d={d}
                      fill="none"
                      className="stroke-lcd-fg"
                      strokeWidth={1.5}
                      strokeDasharray="5 4"
                    />
                    <text
                      x={end[0] - 6}
                      y={end[1] + BIND_Y + 4}
                      textAnchor="end"
                      className="fill-lcd-fg font-lcd text-[12px]"
                    >
                      {field}
                    </text>
                  </g>
                );
              }),
          )}
        </svg>
        {def.nodes.map((node) => {
          const kind = kindOf(node.kind);
          const [x, y] = at(node.id);
          const isSelected = selected?.type === "node" && selected.id === node.id;
          const broken = (problems.get(node.id)?.length ?? 0) > 0;
          const summary = Object.entries(fieldsOf(node)).slice(0, 3);
          return (
            <div
              key={node.id}
              className={`surface-panel absolute grid gap-1 p-1 ${isSelected ? "outline-2 outline-focus" : broken ? "outline-2 outline-lcd-error" : ""}`}
              style={{ left: x, top: y, width: CARD_W }}
            >
              <button
                type="button"
                aria-pressed={isSelected}
                aria-label={`${kind?.label ?? node.kind} ${node.id}${broken ? ", has a problem" : ""}. Arrow keys move it; Delete removes it.`}
                onClick={() => onSelect({ type: "node", id: node.id })}
                onPointerDown={(e) => onPointerDown(e, node)}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                onKeyDown={(e) => onKey(e, node)}
                className="surface-pbutton min-h-9 cursor-grab touch-none truncate border-0 px-2 text-left font-bold text-[11px] uppercase tracking-[0.08em]"
              >
                {kind?.label ?? node.kind}
                <span className="font-normal text-ink-dim normal-case tracking-normal">
                  {" "}
                  {node.id}
                </span>
              </button>
              {kind && !kind.source ? (
                <button
                  type="button"
                  aria-label={pending ? `Wire ${pending} into ${node.id}` : `Input of ${node.id}`}
                  disabled={!pending}
                  onClick={() => onPortIn(node.id)}
                  className={`absolute size-5 rounded-full border-2 border-frame-edge ${pending ? "bg-focus" : "bg-silver"}`}
                  style={{ left: -10, top: PORT_Y - 10 }}
                />
              ) : null}
              {kind && !kind.control && kind.kind !== "output" ? (
                <button
                  type="button"
                  aria-pressed={pending === node.id}
                  aria-label={`Output of ${node.id}: start a wire`}
                  onClick={() => onPortOut(node.id)}
                  className={`absolute size-5 rounded-full border-2 border-frame-edge ${pending === node.id ? "bg-focus" : "bg-silver"}`}
                  style={{ right: -10, top: PORT_Y - 10 }}
                />
              ) : null}
              {kind?.control ? (
                <span
                  aria-hidden="true"
                  className="absolute size-5 rounded-full border-2 border-lcd-fg border-dashed bg-lcd-bg"
                  style={{ right: -10, top: PORT_Y - 10 }}
                />
              ) : null}
              {summary.length > 0 ? (
                <ul className="surface-lcd m-0 list-none px-1 py-0.5">
                  {summary.map(([field, value]) => (
                    <li
                      key={field}
                      className="lcd-text truncate font-lcd text-[13px] leading-tight"
                    >
                      {field} {describe(value)}
                    </li>
                  ))}
                </ul>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** A field value in a few characters for a node card. */
function describe(value: Value): string {
  if (isBinding(value)) {
    if (value.param !== undefined) return `← ${value.param}`;
    return `~ ${value.node ?? "?"}`;
  }
  if (typeof value === "boolean") return value ? "on" : "off";
  return String(value);
}

// ----- the side panel -----

type NodePanelProps = {
  def: PedalDef;
  node: NodeDef;
  problems: Problem[];
  onRename: (to: string) => void;
  onField: (field: string, value: Value | undefined) => void;
};

function NodePanel({ def, node, problems, onRename, onField }: NodePanelProps) {
  const id = useId();
  const kind = kindOf(node.kind);
  const [name, setName] = useState(node.id);
  useEffect(() => setName(node.id), [node.id]);
  const fields = fieldsOf(node);
  return (
    <section aria-labelledby={`${id}-title`} className="grid gap-2">
      <h3 id={`${id}-title`} className={`m-0 ${LABEL}`}>
        {kind?.label ?? node.kind}
      </h3>
      <p className="m-0 text-ink-dim text-xs">{kind?.summary}</p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const to = name.trim();
          if (to && to !== node.id && !def.nodes.some((n) => n.id === to)) onRename(to);
          else setName(node.id);
        }}
      >
        <TextInput
          id={`${id}-id`}
          label="Id"
          hint="Press Enter to rename"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="min-w-0"
        />
      </form>
      {kind?.fields.map((field) => (
        <FieldEditor
          key={field.name}
          def={def}
          node={node}
          field={field}
          value={fields[field.name]}
          problems={problems.filter((p) => p.field === field.name)}
          onChange={(value) => onField(field.name, value)}
        />
      ))}
      {Object.keys(fields)
        .filter((name) => !kind?.fields.some((f) => f.name === name))
        .map((name) => (
          <div key={name} className="flex items-center justify-between gap-2">
            <span className="lcd-text font-lcd text-base text-lcd-error">
              {name}: no such field on a {node.kind}
            </span>
            <Button onClick={() => onField(name, undefined)} className="min-h-9 px-2">
              Drop
            </Button>
          </div>
        ))}
    </section>
  );
}

type FieldEditorProps = {
  def: PedalDef;
  node: NodeDef;
  field: FieldInfo;
  value: Value | undefined;
  problems: Problem[];
  onChange: (value: Value | undefined) => void;
};

type Source = "default" | "constant" | "param" | "signal";

function sourceOf(value: Value | undefined): Source {
  if (value === undefined) return "default";
  if (isBinding(value)) return value.param !== undefined ? "param" : "signal";
  return "constant";
}

/** A field's default, as its constant. */
function defaultOf(field: FieldInfo): Value {
  switch (field.kind.type) {
    case "number":
      return field.kind.default;
    case "name":
      return field.kind.options[field.kind.default] ?? "";
    case "bool":
      return field.kind.default;
  }
}

function FieldEditor({ def, node, field, value, problems, onChange }: FieldEditorProps) {
  const id = useId();
  const source = sourceOf(value);
  const binding = isBinding(value) ? value : null;
  const others = def.nodes.filter((n) => n.id !== node.id);
  const kind = field.kind;

  const setSource = (next: Source) => {
    if (next === source) return;
    switch (next) {
      case "default":
        onChange(undefined);
        break;
      case "constant":
        onChange(defaultOf(field));
        break;
      case "param": {
        const first = def.params[0]?.id ?? "";
        if (kind.type === "number") onChange({ param: first, map: [kind.min, kind.max] });
        else onChange({ param: first });
        break;
      }
      case "signal": {
        const lfo = others.find((n) => n.kind === "lfo") ?? others[0];
        if (kind.type === "number") onChange({ node: lfo?.id ?? "", map: [kind.min, kind.max] });
        break;
      }
    }
  };

  const paramLabel = (p: ParamDef) => `${p.label || p.id} (${p.id})`;

  return (
    <fieldset className="m-0 grid gap-1 border-0 p-0">
      <div className="flex items-baseline justify-between gap-2">
        <legend className={`${LABEL} float-left p-0`} title={field.summary}>
          {field.label}
          {field.modulatable ? (
            <span className="font-normal text-ink-dim normal-case tracking-normal">
              {" "}
              · modulatable
            </span>
          ) : null}
        </legend>
        <span className="relative">
          <select
            id={`${id}-source`}
            aria-label={`${field.label} source`}
            value={source}
            onChange={(e) => setSource(e.target.value as Source)}
            className="surface-lcd min-h-9 appearance-none py-0 pr-7 pl-2 font-lcd text-base"
          >
            <option value="default">Default</option>
            <option value="constant">Constant</option>
            <option value="param">Parameter</option>
            {field.modulatable ? <option value="signal">Signal</option> : null}
          </select>
          <svg
            aria-hidden="true"
            viewBox="0 0 16 16"
            className="pointer-events-none absolute top-1/2 right-2 size-3 -translate-y-1/2 fill-lcd-fg"
          >
            <path d="M2 5l6 6 6-6z" />
          </svg>
        </span>
      </div>
      {source === "constant" && kind.type === "number" ? (
        <Fader
          param={fieldAsParam(field, typeof value === "number" ? value : kind.default)}
          value={typeof value === "number" ? value : kind.default}
          onChange={onChange}
        />
      ) : null}
      {source === "constant" && kind.type === "name" ? (
        <Fader
          param={fieldAsParam(field, kind.options.indexOf(String(value)))}
          value={Math.max(0, kind.options.indexOf(String(value)))}
          onChange={(i) => onChange(kind.options[i] ?? kind.options[0] ?? "")}
        />
      ) : null}
      {source === "constant" && kind.type === "bool" ? (
        <Fader
          param={fieldAsParam(field, value === true || value === 1 ? 1 : 0)}
          value={value === true || value === 1 ? 1 : 0}
          onChange={(v) => onChange(v >= 0.5)}
        />
      ) : null}
      {source === "param" && binding ? (
        <div className="grid gap-1">
          <Select
            id={`${id}-param`}
            label="Follows"
            value={binding.param ?? ""}
            onChange={(e) => onChange({ ...binding, param: e.target.value })}
            className="min-w-0"
          >
            {def.params.length === 0 ? <option value="">Add a parameter first</option> : null}
            {def.params.map((p) => (
              <option key={p.id} value={p.id}>
                {paramLabel(p)}
              </option>
            ))}
          </Select>
          {kind.type === "number" ? (
            <MapEditor id={id} binding={binding} field={field} onChange={onChange} />
          ) : (
            <TableEditor
              id={id}
              binding={binding}
              field={field}
              param={def.params.find((p) => p.id === binding.param)}
              onChange={onChange}
            />
          )}
        </div>
      ) : null}
      {source === "signal" && binding ? (
        <div className="grid gap-1">
          <Select
            id={`${id}-node`}
            label="Follows"
            hint="Sample by sample; an lfo is the usual source"
            value={binding.node ?? ""}
            onChange={(e) => onChange({ ...binding, node: e.target.value })}
            className="min-w-0"
          >
            {others.map((n) => (
              <option key={n.id} value={n.id}>
                {n.id} ({n.kind})
              </option>
            ))}
          </Select>
          {kind.type === "number" ? (
            <MapEditor id={id} binding={binding} field={field} onChange={onChange} />
          ) : null}
        </div>
      ) : null}
      {problems.map((p) => (
        <Lcd key={p.message} error className="text-sm">
          {p.message}
        </Lcd>
      ))}
    </fieldset>
  );
}

/** A field described as a parameter, so a Fader can edit its constant. */
function fieldAsParam(field: FieldInfo, value: number): ParamInfo {
  switch (field.kind.type) {
    case "number":
      return {
        name: field.label,
        unit: field.kind.unit === "×" ? "" : field.kind.unit,
        min: field.kind.min,
        max: field.kind.max,
        default: field.kind.default,
        value,
        toggle: false,
        choices: [],
      };
    case "name":
      return {
        name: field.label,
        unit: "",
        min: 0,
        max: field.kind.options.length - 1,
        default: field.kind.default,
        value,
        toggle: false,
        choices: [...field.kind.options],
      };
    case "bool":
      return {
        name: field.label,
        unit: "",
        min: 0,
        max: 1,
        default: field.kind.default ? 1 : 0,
        value,
        toggle: true,
        choices: [],
      };
  }
}

type MapEditorProps = {
  id: string;
  binding: Binding;
  field: FieldInfo;
  onChange: (value: Binding) => void;
};

/** The ends of a map and whether it is exponential. */
function MapEditor({ id, binding, field, onChange }: MapEditorProps) {
  const kind = field.kind.type === "number" ? field.kind : null;
  const [lo, hi] = binding.map ?? [kind?.min ?? 0, kind?.max ?? 1];
  const mapped = binding.map !== undefined;
  return (
    <div className="grid gap-1">
      <label className="flex items-center justify-between gap-2 text-sm">
        <span>Map the range</span>
        <input
          type="checkbox"
          checked={mapped}
          onChange={(e) => {
            const next = { ...binding };
            if (e.target.checked) next.map = [kind?.min ?? 0, kind?.max ?? 1];
            else {
              delete next.map;
              delete next.log;
            }
            onChange(next);
          }}
          className="size-5 accent-lcd-fg"
        />
      </label>
      {mapped ? (
        <div className="grid grid-cols-[1fr_1fr_auto] items-end gap-2">
          <NumberField
            id={`${id}-lo`}
            label="From"
            value={lo}
            onChange={(v) => onChange({ ...binding, map: [v, hi] })}
          />
          <NumberField
            id={`${id}-hi`}
            label="To"
            value={hi}
            onChange={(v) => onChange({ ...binding, map: [lo, v] })}
          />
          <label className="grid justify-items-center gap-1 text-xs">
            Log
            <input
              type="checkbox"
              checked={binding.log ?? false}
              onChange={(e) => {
                const next = { ...binding };
                if (e.target.checked) next.log = true;
                else delete next.log;
                onChange(next);
              }}
              className="size-5 accent-lcd-fg"
            />
          </label>
        </div>
      ) : null}
    </div>
  );
}

type TableEditorProps = {
  id: string;
  binding: Binding;
  field: FieldInfo;
  param: ParamDef | undefined;
  onChange: (value: Binding) => void;
};

/** The field's value for each index of a choice or toggle parameter. */
function TableEditor({ id, binding, field, param, onChange }: TableEditorProps) {
  const entries = param?.choices?.length ? param.choices : param?.toggle ? ["Off", "On"] : null;
  if (!entries) {
    return (
      <p className="m-0 text-ink-dim text-xs">
        A continuous parameter is used as an index; a choice or toggle lets each value be picked.
      </p>
    );
  }
  const options =
    field.kind.type === "name"
      ? field.kind.options
      : field.kind.type === "bool"
        ? ["false", "true"]
        : [];
  const current = binding.values ?? [];
  const set = (i: number, v: string) => {
    const values = entries.map((_, j) => {
      const was = current[j];
      const chosen = j === i ? v : was === undefined ? (options[0] ?? "") : String(was);
      return field.kind.type === "bool" ? chosen === "true" : chosen;
    });
    onChange({ ...binding, values });
  };
  return (
    <div className="grid gap-1">
      {entries.map((entry, i) => (
        <Select
          key={entry}
          id={`${id}-value-${String(i)}`}
          label={entry}
          value={String(current[i] ?? options[0] ?? "")}
          onChange={(e) => set(i, e.target.value)}
          className="min-w-0"
        >
          {options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </Select>
      ))}
    </div>
  );
}

type NumberFieldProps = {
  id: string;
  label: string;
  value: number;
  onChange: (value: number) => void;
};

function NumberField({ id, label, value, onChange }: NumberFieldProps) {
  return (
    <label htmlFor={id} className="grid gap-1 text-xs">
      {label}
      <input
        id={id}
        type="number"
        step="any"
        value={value}
        onChange={(e) => {
          const v = Number(e.target.value);
          if (Number.isFinite(v)) onChange(v);
        }}
        className="surface-lcd min-h-9 w-full min-w-0 px-1 font-lcd text-base"
      />
    </label>
  );
}

// ----- parameters -----

type ParamsPanelProps = {
  def: PedalDef;
  /** The pedal as it runs, for the knobs. */
  live: PedalInfo;
  onChange: (def: PedalDef) => void;
  /** A knob moved: the parameter's id, its index and the value. */
  onValue: (id: string, index: number, value: number) => void;
};

const UNITS = ["", "dB", "Hz", "s", "ms", "%"];

function ParamsPanel({ def, live, onChange, onValue }: ParamsPanelProps) {
  const id = useId();
  const [knobs, setKnobs] = useState(() => live.params.map((p) => p.value));
  useEffect(() => {
    setKnobs(live.params.map((p) => p.value));
  }, [live.params]);
  const update = (index: number, patch: Partial<ParamDef>) =>
    onChange({
      ...def,
      params: def.params.map((p, i) => (i === index ? { ...p, ...patch } : p)),
    });
  const add = () => {
    const pid = freshParamId(def);
    onChange({
      ...def,
      params: [
        ...def.params,
        { id: pid, label: `Param ${def.params.length + 1}`, min: 0, max: 1, default: 0.5 },
      ],
    });
  };
  return (
    <section aria-labelledby={`${id}-title`} className="grid gap-2">
      <div className="flex items-center justify-between gap-2">
        <h3 id={`${id}-title`} className={`m-0 ${LABEL}`}>
          Parameters
        </h3>
        <Button onClick={add} className="min-h-9 px-2">
          Add
        </Button>
      </div>
      {def.params.length === 0 ? (
        <p className="m-0 text-ink-dim text-xs">
          No knobs yet. A parameter becomes a knob on the pedal; bind a node's field to it.
        </p>
      ) : null}
      {def.params.map((p, i) => (
        <ParamRow
          key={`${String(i)}-${p.id}`}
          id={`${id}-${String(i)}`}
          param={p}
          live={live.params[i]}
          knob={knobs[i] ?? p.default}
          onPatch={(patch) => update(i, patch)}
          onRename={(to) => onChange(renameParam(def, p.id, to))}
          onRemove={() => onChange(removeParam(def, p.id))}
          onKnob={(v) => {
            setKnobs((k) => k.map((x, j) => (j === i ? v : x)));
            onValue(p.id, i, v);
          }}
        />
      ))}
    </section>
  );
}

type ParamRowProps = {
  id: string;
  param: ParamDef;
  live: ParamInfo | undefined;
  knob: number;
  onPatch: (patch: Partial<ParamDef>) => void;
  onRename: (to: string) => void;
  onRemove: () => void;
  onKnob: (value: number) => void;
};

function ParamRow({ id, param, live, knob, onPatch, onRename, onRemove, onKnob }: ParamRowProps) {
  const [pid, setPid] = useState(param.id);
  useEffect(() => setPid(param.id), [param.id]);
  const [choices, setChoices] = useState((param.choices ?? []).join(", "));
  useEffect(() => setChoices((param.choices ?? []).join(", ")), [param.choices]);
  const isChoice = (param.choices?.length ?? 0) > 0;
  const [min, max] = paramRange(param);
  return (
    <div className="surface-panel grid gap-1 p-2">
      {live ? <Fader param={{ ...live, value: knob }} value={knob} onChange={onKnob} /> : null}
      <div className="grid grid-cols-2 gap-2">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const to = pid.trim();
            if (to && to !== param.id) onRename(to);
            else setPid(param.id);
          }}
        >
          <label htmlFor={`${id}-id`} className="grid gap-1 text-xs">
            Id
            <input
              id={`${id}-id`}
              value={pid}
              onChange={(e) => setPid(e.target.value)}
              className="surface-lcd min-h-9 w-full min-w-0 px-1 font-lcd text-base"
            />
          </label>
        </form>
        <label htmlFor={`${id}-label`} className="grid gap-1 text-xs">
          Label
          <input
            id={`${id}-label`}
            value={param.label}
            onChange={(e) => onPatch({ label: e.target.value })}
            className="surface-lcd min-h-9 w-full min-w-0 px-1 font-lcd text-base"
          />
        </label>
      </div>
      {!isChoice && !param.toggle ? (
        <div className="grid grid-cols-[1fr_1fr_1fr_auto] items-end gap-1">
          <NumberField
            id={`${id}-min`}
            label="Min"
            value={param.min}
            onChange={(v) => onPatch({ min: v })}
          />
          <NumberField
            id={`${id}-max`}
            label="Max"
            value={param.max}
            onChange={(v) => onPatch({ max: v })}
          />
          <NumberField
            id={`${id}-default`}
            label="Default"
            value={param.default}
            onChange={(v) => onPatch({ default: v })}
          />
          <label htmlFor={`${id}-unit`} className="grid gap-1 text-xs">
            Unit
            <select
              id={`${id}-unit`}
              value={param.unit ?? ""}
              onChange={(e) => onPatch({ unit: e.target.value })}
              className="surface-lcd min-h-9 w-16 px-1 font-lcd text-base"
            >
              {UNITS.map((u) => (
                <option key={u} value={u}>
                  {u || "—"}
                </option>
              ))}
            </select>
          </label>
        </div>
      ) : (
        <NumberField
          id={`${id}-default`}
          label={`Default index (${min} to ${max})`}
          value={param.default}
          onChange={(v) => onPatch({ default: v })}
        />
      )}
      <div className="grid grid-cols-[1fr_auto_auto] items-end gap-2">
        <label htmlFor={`${id}-choices`} className="grid gap-1 text-xs">
          Choices, comma separated
          <input
            id={`${id}-choices`}
            value={choices}
            onChange={(e) => setChoices(e.target.value)}
            onBlur={() => {
              const list = choices
                .split(",")
                .map((c) => c.trim())
                .filter(Boolean);
              const patch: Partial<ParamDef> = { choices: list };
              if (list.length > 0) patch.toggle = false;
              onPatch(patch);
            }}
            className="surface-lcd min-h-9 w-full min-w-0 px-1 font-lcd text-base"
          />
        </label>
        <Button
          aria-pressed={param.toggle ?? false}
          onClick={() => onPatch({ toggle: !param.toggle, choices: [] })}
          className="min-h-9 px-2"
          title="A switch: 0 or 1"
        >
          Toggle
        </Button>
        <Button
          aria-label={`Remove parameter ${param.id}`}
          onClick={onRemove}
          className="min-h-9 px-2"
        >
          <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5 fill-current">
            <path d="M3 2l5 5 5-5 1 1-5 5 5 5-1 1-5-5-5 5-1-1 5-5-5-5z" />
          </svg>
        </Button>
      </div>
    </div>
  );
}
