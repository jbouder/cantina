import { describe, expect, it } from "vitest";
import overdrive from "../../../pedals/overdrive.pedal.json";
import {
  addNode,
  connect,
  disconnect,
  empty,
  freshNodeId,
  KINDS,
  type PedalDef,
  removeNode,
  removeParam,
  renameNode,
  renameParam,
  setField,
  validate,
  valuesFor,
} from "./pedal";

const stock = overdrive as unknown as PedalDef;

describe("the catalog", () => {
  it("lists every kind the dsp crate has", () => {
    expect(KINDS.map((k) => k.kind)).toEqual([
      "input",
      "output",
      "gain",
      "biquad",
      "waveshaper",
      "delay",
      "lfo",
      "allpass",
      "mix",
      "split",
    ]);
  });
});

describe("validate", () => {
  it("passes the stock overdrive and an empty pedal", () => {
    expect(validate(stock)).toEqual([]);
    expect(validate(empty("New"))).toEqual([]);
  });

  it("names the node behind each problem", () => {
    const def: PedalDef = {
      ...stock,
      nodes: stock.nodes.map((n) => (n.id === "clip" ? { ...n, kind: "wavshaper" } : n)),
      edges: [...stock.edges, ["tone", "nowhere"]],
    };
    const problems = validate(def);
    expect(problems).toContainEqual({ node: "clip", message: 'unknown kind "wavshaper"' });
    expect(problems).toContainEqual({ message: 'edge "tone" to "nowhere": no node "nowhere"' });
  });

  it("finds a loose node, a missing input and a cycle", () => {
    let def = disconnect(stock, "tone", "level");
    let problems = validate(def);
    expect(problems).toContainEqual({ node: "tone", message: "feeds nothing" });
    expect(problems).toContainEqual({ node: "level", message: "has no input" });
    def = connect(def, "tone", "pre");
    problems = validate(def);
    expect(problems.some((p) => p.message.startsWith("a cycle runs through"))).toBe(true);
    expect(problems).toContainEqual({
      node: "pre",
      message: "has more than one input; feed them into a mix",
    });
  });

  it("checks fields and bindings", () => {
    let def = setField(stock, "pre", "freq", 5);
    def = setField(def, "pre", "gian", 3);
    def = setField(def, "clip", "curve", { node: "tone", map: [0, 1] });
    def = setField(def, "tone", "freq", { param: "missing" });
    const problems = validate(def);
    expect(problems).toContainEqual({
      node: "pre",
      field: "freq",
      message: "5 is out of range; freq takes 20 to 20000",
    });
    expect(problems).toContainEqual({
      node: "pre",
      field: "gian",
      message: "no such field on a biquad",
    });
    expect(problems).toContainEqual({
      node: "clip",
      field: "curve",
      message: "cannot follow a signal",
    });
    expect(problems).toContainEqual({
      node: "tone",
      field: "freq",
      message: 'no parameter "missing"',
    });
  });

  it("rejects an lfo on an audio edge and a second output", () => {
    let [def, lfo] = addNode(stock, "lfo");
    def = connect(def, lfo, "level");
    const problems = validate(def);
    expect(problems).toContainEqual({
      node: "lfo",
      message: "an lfo drives fields, not audio; bind a field to it",
    });
    const [two] = addNode(stock, "output");
    expect(validate(two)).toContainEqual({ message: "there are 2 output nodes; a pedal has one" });
  });
});

describe("edits", () => {
  it("adds nodes with fresh ids and places them below the rest", () => {
    const [def, id] = addNode(stock, "gain");
    expect(id).toBe("gain");
    const [again, id2] = addNode(def, "gain");
    expect(id2).toBe("gain2");
    expect(freshNodeId(again, "input")).toBe("in2");
    const node = again.nodes.find((n) => n.id === "gain2");
    expect(node?.pos?.[1]).toBeGreaterThan(160);
  });

  it("removes a node with its edges and the bindings to it", () => {
    let [def, lfo] = addNode(stock, "lfo");
    def = setField(def, "tone", "freq", { node: lfo, map: [500, 5000] });
    expect(validate(def)).toEqual([]);
    const gone = removeNode(def, lfo);
    expect(gone.nodes.some((n) => n.id === lfo)).toBe(false);
    expect(gone.nodes.find((n) => n.id === "tone")?.freq).toBeUndefined();
    const cut = removeNode(stock, "tone");
    expect(cut.edges).not.toContainEqual(["post", "tone"]);
    expect(cut.edges).not.toContainEqual(["tone", "level"]);
  });

  it("renames nodes and parameters through edges and bindings", () => {
    let def = renameNode(stock, "tone", "lp");
    expect(def.edges).toContainEqual(["post", "lp"]);
    expect(def.edges).toContainEqual(["lp", "level"]);
    def = renameParam(def, "tone", "cut");
    expect(def.params.some((p) => p.id === "cut")).toBe(true);
    expect(def.nodes.find((n) => n.id === "lp")?.freq).toEqual({ param: "cut" });
    expect(validate(def)).toEqual([]);
    const fewer = removeParam(def, "cut");
    expect(fewer.nodes.find((n) => n.id === "lp")?.freq).toBeUndefined();
    expect(validate(fewer)).toEqual([]);
  });

  it("keeps the values a parameter had and defaults the rest", () => {
    const had = new Map([
      ["drive", 33],
      ["level", 99],
    ]);
    expect(valuesFor(stock, had)).toEqual([33, 3000, 6, 0, 2]);
  });
});
