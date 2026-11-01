/**
 * The pedal format as the designer edits it: the types mirror
 * `crates/dsp/src/pedal/mod.rs`, the kind catalog is the one the dsp
 * crate writes to `pedal-kinds.json`, and the helpers here make the
 * edits a node editor needs. Validation proper happens in Rust when a
 * draft is sent; `validate` here is the browser stand-in's copy of the
 * structural rules, so screenshots and tests see the same messages.
 */
import kindsJson from "./pedal-kinds.json";

export const PEDAL_VERSION = 1;

export type Num = number;

/** A field's value: a number, a name, a switch or a binding. */
export type Value = number | string | boolean | Binding;

export interface Binding {
  param?: string;
  node?: string;
  map?: [number, number];
  log?: boolean;
  values?: Value[];
}

export interface ParamDef {
  id: string;
  label: string;
  unit?: string;
  min: number;
  max: number;
  default: number;
  choices?: string[];
  toggle?: boolean;
}

export interface NodeDef {
  id: string;
  kind: string;
  pos?: [number, number];
  /** The kind's fields, by name. */
  [field: string]: Value | [number, number] | undefined;
}

export interface PedalDef {
  version: number;
  name: string;
  params: ParamDef[];
  nodes: NodeDef[];
  edges: [string, string][];
}

/** One thing wrong with a pedal. Mirrors `Problem` in the dsp crate. */
export interface Problem {
  node?: string;
  field?: string;
  message: string;
}

export type FieldKind =
  | { type: "number"; min: number; max: number; default: number; unit: string }
  | { type: "name"; options: string[]; default: number }
  | { type: "bool"; default: boolean };

export interface FieldInfo {
  name: string;
  label: string;
  summary: string;
  kind: FieldKind;
  modulatable: boolean;
  smoothed: boolean;
}

export interface KindInfo {
  kind: string;
  label: string;
  summary: string;
  source: boolean;
  control: boolean;
  sums: boolean;
  fields: FieldInfo[];
}

/** Every kind, in palette order. */
export const KINDS: KindInfo[] = kindsJson as KindInfo[];

export function kindOf(name: string): KindInfo | undefined {
  return KINDS.find((k) => k.kind === name);
}

/** The keys of a node that are not fields. */
const RESERVED = new Set(["id", "kind", "pos"]);

export function isBinding(value: Value | undefined): value is Binding {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The field values of a node, by name. */
export function fieldsOf(node: NodeDef): Record<string, Value> {
  const out: Record<string, Value> = {};
  for (const [key, value] of Object.entries(node)) {
    if (!RESERVED.has(key) && value !== undefined && !Array.isArray(value)) out[key] = value;
  }
  return out;
}

/** The nodes a node's bindings follow. */
export function signalSources(node: NodeDef): string[] {
  const out: string[] = [];
  for (const value of Object.values(fieldsOf(node))) {
    if (isBinding(value) && value.node && !value.param) out.push(value.node);
  }
  return out;
}

export function empty(name: string): PedalDef {
  return {
    version: PEDAL_VERSION,
    name,
    params: [],
    nodes: [
      { id: "in", kind: "input", pos: [40, 160] },
      { id: "out", kind: "output", pos: [400, 160] },
    ],
    edges: [["in", "out"]],
  };
}

/** An id of the kind not used by any node: `gain`, `gain2`, `gain3`… */
export function freshNodeId(def: PedalDef, kind: string): string {
  const base = kind === "input" ? "in" : kind === "output" ? "out" : kind;
  if (!def.nodes.some((n) => n.id === base)) return base;
  for (let i = 2; ; i++) {
    const id = `${base}${i}`;
    if (!def.nodes.some((n) => n.id === id)) return id;
  }
}

export function freshParamId(def: PedalDef): string {
  for (let i = 1; ; i++) {
    const id = `p${i}`;
    if (!def.params.some((p) => p.id === id)) return id;
  }
}

/** Somewhere below the lowest node, to the right of the input. */
export function placeFor(def: PedalDef): [number, number] {
  const ys = def.nodes.map((n) => n.pos?.[1] ?? 0);
  const lowest = ys.length ? Math.max(...ys) : 0;
  return [200, lowest + 140];
}

export function addNode(def: PedalDef, kind: string, pos?: [number, number]): [PedalDef, string] {
  const id = freshNodeId(def, kind);
  const node: NodeDef = { id, kind, pos: pos ?? placeFor(def) };
  return [{ ...def, nodes: [...def.nodes, node] }, id];
}

/** Removes a node with every edge and binding that touched it. */
export function removeNode(def: PedalDef, id: string): PedalDef {
  return {
    ...def,
    nodes: def.nodes
      .filter((n) => n.id !== id)
      .map((n) => {
        let changed = false;
        const next: NodeDef = { ...n };
        for (const [key, value] of Object.entries(fieldsOf(n))) {
          if (isBinding(value) && value.node === id) {
            delete next[key];
            changed = true;
          }
        }
        return changed ? next : n;
      }),
    edges: def.edges.filter(([a, b]) => a !== id && b !== id),
  };
}

export function renameNode(def: PedalDef, from: string, to: string): PedalDef {
  if (from === to) return def;
  return {
    ...def,
    nodes: def.nodes.map((n) => {
      const next: NodeDef = { ...n, id: n.id === from ? to : n.id };
      for (const [key, value] of Object.entries(fieldsOf(n))) {
        if (isBinding(value) && value.node === from) next[key] = { ...value, node: to };
      }
      return next;
    }),
    edges: def.edges.map(([a, b]) => [a === from ? to : a, b === from ? to : b]),
  };
}

export function moveNode(def: PedalDef, id: string, pos: [number, number]): PedalDef {
  return {
    ...def,
    nodes: def.nodes.map((n) =>
      n.id === id ? { ...n, pos: [Math.round(pos[0]), Math.round(pos[1])] } : n,
    ),
  };
}

export function setField(
  def: PedalDef,
  id: string,
  field: string,
  value: Value | undefined,
): PedalDef {
  return {
    ...def,
    nodes: def.nodes.map((n) => {
      if (n.id !== id) return n;
      const next: NodeDef = { ...n };
      if (value === undefined) delete next[field];
      else next[field] = value;
      return next;
    }),
  };
}

export function connect(def: PedalDef, from: string, to: string): PedalDef {
  if (def.edges.some(([a, b]) => a === from && b === to)) return def;
  return { ...def, edges: [...def.edges, [from, to]] };
}

export function disconnect(def: PedalDef, from: string, to: string): PedalDef {
  return { ...def, edges: def.edges.filter(([a, b]) => !(a === from && b === to)) };
}

/** Renames a parameter everywhere it is bound. */
export function renameParam(def: PedalDef, from: string, to: string): PedalDef {
  if (from === to) return def;
  return {
    ...def,
    params: def.params.map((p) => (p.id === from ? { ...p, id: to } : p)),
    nodes: def.nodes.map((n) => {
      const next: NodeDef = { ...n };
      for (const [key, value] of Object.entries(fieldsOf(n))) {
        if (isBinding(value) && value.param === from) next[key] = { ...value, param: to };
      }
      return next;
    }),
  };
}

/** Removes a parameter and every binding to it. */
export function removeParam(def: PedalDef, id: string): PedalDef {
  return {
    ...def,
    params: def.params.filter((p) => p.id !== id),
    nodes: def.nodes.map((n) => {
      const next: NodeDef = { ...n };
      for (const [key, value] of Object.entries(fieldsOf(n))) {
        if (isBinding(value) && value.param === id) delete next[key];
      }
      return next;
    }),
  };
}

/** The range a parameter's value runs over. */
export function paramRange(p: ParamDef): [number, number] {
  if (p.choices && p.choices.length > 0) return [0, p.choices.length - 1];
  if (p.toggle) return [0, 1];
  return [p.min, p.max];
}

/**
 * The parameter values to send with a definition: the value a
 * parameter already had, by id, or its default.
 */
export function valuesFor(def: PedalDef, had: Map<string, number>): number[] {
  return def.params.map((p) => {
    const v = had.get(p.id);
    const [min, max] = paramRange(p);
    return v === undefined ? p.default : Math.min(max, Math.max(min, v));
  });
}

const UNITS = new Set(["", "dB", "Hz", "s", "ms", "%"]);

/**
 * The structural rules, as the dsp crate checks them, for the browser
 * stand-in and instant feedback. The crate is the authority: what it
 * rejects is what the designer shows, and this follows its messages.
 */
export function validate(def: PedalDef): Problem[] {
  const problems: Problem[] = [];
  const pedal = (message: string) => problems.push({ message });
  const node = (id: string, message: string) => problems.push({ node: id, message });
  const field = (id: string, name: string, message: string) =>
    problems.push({ node: id, field: name, message });

  if (def.version !== PEDAL_VERSION)
    pedal(`version ${def.version} is not the version this build reads (${PEDAL_VERSION})`);
  if (!def.name.trim()) pedal("the pedal has no name");
  if (def.params.length > 32) pedal(`${def.params.length} parameters; at most 32`);
  const paramIds = new Set<string>();
  for (const p of def.params) {
    const bad = (m: string) => pedal(`parameter "${p.id}": ${m}`);
    if (!p.id) pedal(`parameter ${def.params.indexOf(p)} has no id`);
    else if (paramIds.has(p.id)) pedal(`two parameters are called "${p.id}"`);
    paramIds.add(p.id);
    const choices = p.choices ?? [];
    if (choices.length > 0 && p.toggle) bad("is both a choice and a toggle");
    if (choices.some((c) => c === "")) bad("has an empty choice");
    if (choices.length === 0 && !p.toggle) {
      if (!UNITS.has(p.unit ?? "")) bad(`unit "${p.unit}" is not one of dB, Hz, s, ms or %`);
      if (!(p.min < p.max)) bad(`min ${p.min} must be below max ${p.max}`);
    }
    const [min, max] = paramRange(p);
    if (!(p.default >= min && p.default <= max))
      bad(`default ${p.default} is outside ${min} to ${max}`);
  }

  if (def.nodes.length > 64) pedal(`${def.nodes.length} nodes; at most 64`);
  const ids = new Map<string, number>();
  const kinds: (KindInfo | undefined)[] = [];
  def.nodes.forEach((n, i) => {
    if (!n.id) pedal(`node ${i} has no id`);
    else if (ids.has(n.id)) node(n.id, "two nodes have this id");
    else ids.set(n.id, i);
    const k = kindOf(n.kind);
    if (!k) node(n.id, `unknown kind "${n.kind}"`);
    kinds.push(k);
  });
  for (const name of ["input", "output"]) {
    const count = kinds.filter((k) => k?.kind === name).length;
    if (count === 0) pedal(`there is no ${name} node`);
    else if (count > 1) pedal(`there are ${count} ${name} nodes; a pedal has one`);
  }

  const inputs: number[][] = def.nodes.map(() => []);
  const feeds = def.nodes.map(() => false);
  const seen = new Set<string>();
  for (const [from, to] of def.edges) {
    const a = ids.get(from);
    const b = ids.get(to);
    if (a === undefined) pedal(`edge "${from}" to "${to}": no node "${from}"`);
    if (b === undefined) pedal(`edge "${from}" to "${to}": no node "${to}"`);
    if (a === undefined || b === undefined) continue;
    if (seen.has(`${a}:${b}`)) {
      node(from, `is wired to "${to}" twice`);
      continue;
    }
    seen.add(`${a}:${b}`);
    const ka = kinds[a];
    const kb = kinds[b];
    if (ka?.kind === "output") {
      node(from, "the output feeds nothing further");
      continue;
    }
    if (ka?.control) {
      node(from, `an ${ka.kind} drives fields, not audio; bind a field to it`);
      continue;
    }
    if (kb?.source) {
      node(to, `the ${kb.kind} takes no input`);
      continue;
    }
    if (a === b) {
      node(from, "is wired to itself");
      continue;
    }
    inputs[b]?.push(a);
    feeds[a] = true;
  }
  def.nodes.forEach((n, i) => {
    const k = kinds[i];
    if (!k) return;
    if (!k.sums && (inputs[i]?.length ?? 0) > 1)
      node(n.id, "has more than one input; feed them into a mix");
    if (!k.source && (inputs[i]?.length ?? 0) === 0) node(n.id, "has no input");
  });

  // Fields.
  def.nodes.forEach((n, i) => {
    const k = kinds[i];
    if (!k) return;
    for (const [name, value] of Object.entries(fieldsOf(n))) {
      const info = k.fields.find((f) => f.name === name);
      if (!info) {
        field(n.id, name, `no such field on a ${k.kind}`);
        continue;
      }
      if (isBinding(value)) {
        if (value.param !== undefined && value.node !== undefined)
          field(n.id, name, "a binding follows a param or a node, not both");
        else if (value.param === undefined && value.node === undefined)
          field(n.id, name, "a binding names a param or a node");
        else if (value.param !== undefined) {
          if (!paramIds.has(value.param)) field(n.id, name, `no parameter "${value.param}"`);
          if (value.log && !value.map) field(n.id, name, "log needs a map");
        } else if (value.node !== undefined) {
          if (!info.modulatable) field(n.id, name, "cannot follow a signal");
          else if (value.node === n.id) field(n.id, name, "a node cannot follow itself");
          else if (!ids.has(value.node)) field(n.id, name, `no node "${value.node}"`);
          else {
            const k2 = ids.get(value.node);
            if (k2 !== undefined) feeds[k2] = true;
          }
        }
      } else if (info.kind.type === "number") {
        if (typeof value !== "number")
          field(n.id, name, `takes a number; ${name} takes ${info.kind.min} to ${info.kind.max}`);
        else if (value < info.kind.min || value > info.kind.max)
          field(
            n.id,
            name,
            `${value} is out of range; ${name} takes ${info.kind.min} to ${info.kind.max}`,
          );
      } else if (info.kind.type === "name") {
        if (typeof value !== "string")
          field(n.id, name, `takes one of ${info.kind.options.join(", ")}`);
        else if (!info.kind.options.includes(value))
          field(n.id, name, `"${value}" is not one of ${info.kind.options.join(", ")}`);
      } else if (typeof value !== "boolean" && value !== 0 && value !== 1) {
        field(n.id, name, "takes true or false");
      }
    }
  });
  def.nodes.forEach((n, i) => {
    const k = kinds[i];
    if (k && k.kind !== "output" && !feeds[i]) node(n.id, "feeds nothing");
  });

  // Cycles, through edges and bindings alike.
  const deps = inputs.map((list) => [...list]);
  def.nodes.forEach((n, i) => {
    for (const source of signalSources(n)) {
      const j = ids.get(source);
      if (j !== undefined) deps[i]?.push(j);
    }
  });
  const remaining = deps.map((d) => d.length);
  const dependents: number[][] = def.nodes.map(() => []);
  deps.forEach((d, i) => {
    for (const j of d) dependents[j]?.push(i);
  });
  const ready = remaining.map((r, i) => (r === 0 ? i : -1)).filter((i) => i >= 0);
  let done = 0;
  while (ready.length > 0) {
    const i = ready.shift() as number;
    done++;
    for (const d of dependents[i] ?? []) {
      remaining[d] = (remaining[d] ?? 1) - 1;
      if (remaining[d] === 0) ready.push(d);
    }
  }
  if (done < def.nodes.length) {
    const stuck = def.nodes.filter((_, i) => (remaining[i] ?? 0) > 0).map((n) => n.id);
    const message = `a cycle runs through ${stuck.map((s) => `"${s}"`).join(", ")}`;
    const first = stuck[0];
    problems.push(first === undefined ? { message } : { node: first, message });
  }
  return problems;
}

/** Problems grouped for display: by node id, with the pedal's own under "". */
export function problemsByNode(problems: Problem[]): Map<string, Problem[]> {
  const out = new Map<string, Problem[]>();
  for (const p of problems) {
    const key = p.node ?? "";
    out.set(key, [...(out.get(key) ?? []), p]);
  }
  return out;
}
