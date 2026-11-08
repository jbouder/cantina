import { describe, expect, it } from "vitest";
import {
  ACTION_IDS,
  ACTIONS,
  actionForKey,
  type Bindings,
  type KeyPress,
  MENUS,
  matchesAccelerator,
  menuStates,
} from "./actions";

const press = (key: string, mods: Partial<KeyPress> = {}): KeyPress => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

// The tests run without a DOM; these are the fields `isTyping` reads.
const el = (tag: string, type?: string) =>
  ({ tagName: tag.toUpperCase(), type, isContentEditable: false }) as unknown as Element;

describe("the registry", () => {
  it("puts every action in exactly one menu", () => {
    const placed = MENUS.flatMap((m) => m.items).filter((i) => i !== "-" && !i.startsWith("@"));
    expect([...placed].sort()).toEqual([...ACTION_IDS].sort());
  });

  it("gives an action an accelerator or a key, never both", () => {
    for (const id of ACTION_IDS) {
      expect(ACTIONS[id].accelerator && ACTIONS[id].key, id).toBeFalsy();
    }
  });
});

describe("matchesAccelerator", () => {
  it("matches ⌘ or ⌃ for CmdOrCtrl", () => {
    expect(matchesAccelerator("CmdOrCtrl+E", press("e", { metaKey: true }))).toBe(true);
    expect(matchesAccelerator("CmdOrCtrl+E", press("e", { ctrlKey: true }))).toBe(true);
    expect(matchesAccelerator("CmdOrCtrl+E", press("e"))).toBe(false);
  });

  it("tells Undo from Redo by Shift", () => {
    const z = press("z", { metaKey: true });
    const shiftZ = press("Z", { metaKey: true, shiftKey: true });
    expect(matchesAccelerator("CmdOrCtrl+Z", z)).toBe(true);
    expect(matchesAccelerator("CmdOrCtrl+Z", shiftZ)).toBe(false);
    expect(matchesAccelerator("Shift+CmdOrCtrl+Z", shiftZ)).toBe(true);
  });

  it("reads named keys", () => {
    expect(matchesAccelerator("CmdOrCtrl+Comma", press(",", { metaKey: true }))).toBe(true);
  });
});

describe("actionForKey", () => {
  const body = el("div");

  it("runs bare keys outside text fields", () => {
    expect(actionForKey(press(" "), body, true)).toBe("play");
    expect(actionForKey(press("r"), body, true)).toBe("record");
    expect(actionForKey(press("K"), body, false)).toBe("metronome");
  });

  it("leaves bare keys to text fields and to controls that use them", () => {
    expect(actionForKey(press("r"), el("input", "text"), false)).toBeNull();
    expect(actionForKey(press(" "), el("button"), false)).toBeNull();
    expect(actionForKey(press("p"), el("textarea"), false)).toBeNull();
  });

  it("leaves accelerators to the native menu in the app", () => {
    expect(actionForKey(press("e", { metaKey: true }), body, true)).toBeNull();
    expect(actionForKey(press("e", { metaKey: true }), body, false)).toBe("export");
  });

  it("ignores a bare key with a modifier", () => {
    expect(actionForKey(press("r", { metaKey: true }), body, false)).toBeNull();
  });
});

describe("menuStates", () => {
  it("reports every action, checks only checkable ones and falls back to the label", () => {
    const bindings = Object.fromEntries(
      ACTION_IDS.map((id) => [id, { enabled: id !== "record", run: () => {} }]),
    ) as Bindings;
    bindings.play = { enabled: true, label: "Stop", run: () => {} };
    const states = menuStates(bindings);
    expect(states).toHaveLength(ACTION_IDS.length);
    expect(states.find((s) => s.id === "play")?.label).toBe("Stop");
    expect(states.find((s) => s.id === "export")?.label).toBe("Export Mixdown…");
    expect(states.find((s) => s.id === "record")?.enabled).toBe(false);
    expect(states.find((s) => s.id === "metronome")?.checked).toBe(false);
    expect(states.find((s) => s.id === "export")).not.toHaveProperty("checked");
  });
});
