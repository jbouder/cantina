import { describe, expect, it } from "vitest";
import type { Tab } from "./engine";
import {
  applyEdit,
  caretStep,
  caretString,
  moveNotes,
  moveString,
  noteAt,
  removeNotes,
  resizeNotes,
  target,
  toggleTechnique,
  typeFret,
} from "./tabEdit";

function tab(): Tab {
  return {
    name: "Riff",
    instrument: "guitar",
    tuning: [40, 45, 50, 55, 59, 64],
    tempo: { bpm: 120, numerator: 4, denominator: 4 },
    notes: [
      { string: 6, fret: 0, start: 0, length: 480 },
      { string: 5, fret: 2, start: 0, length: 480 },
      { string: 6, fret: 3, start: 480, length: 480, hammerOn: true },
    ],
  };
}

describe("the tab editor's edits", () => {
  it("finds the note under the caret, and works on the selection first", () => {
    const t = tab();
    expect(noteAt(t, { string: 6, tick: 240 })).toBe(0);
    expect(noteAt(t, { string: 4, tick: 240 })).toBeNull();
    expect(target(t, [], { string: 6, tick: 500 })).toEqual([2]);
    expect(target(t, [1], { string: 6, tick: 500 })).toEqual([1]);
  });

  it("types a fret onto a note, or adds one a grid step long", () => {
    const t = tab();
    expect(typeFret(t, { string: 6, tick: 0 }, 5, 240)).toEqual({
      remove: [0],
      add: [{ string: 6, fret: 5, start: 0, length: 480 }],
    });
    expect(typeFret(t, { string: 1, tick: 960 }, 12, 240)).toEqual({
      remove: [],
      add: [{ string: 1, fret: 12, start: 960, length: 240 }],
    });
    expect(typeFret(t, { string: 6, tick: 0 }, 0, 240)).toBeNull();
    expect(typeFret(t, { string: 6, tick: 0 }, 25, 240)).toBeNull();
  });

  it("moves notes in time and across strings, keeping the pitch", () => {
    const t = tab();
    expect(moveNotes(t, [2], -240)?.add[0]?.start).toBe(240);
    expect(moveNotes(t, [0], -240)).toBeNull();
    // A G (string 6, fret 3) on the A string is fret -2: refused.
    expect(moveString(t, [2], -1)).toBeNull();
    // B on the A string (fret 2) is fret 7 on the low E.
    expect(moveString(t, [1], 1)?.add[0]).toMatchObject({ string: 6, fret: 7 });
    expect(moveString(t, [0], 1)).toBeNull();
  });

  it("resizes, never below a step", () => {
    const t = tab();
    expect(resizeNotes(t, [0], 240, 240)?.add[0]?.length).toBe(720);
    expect(resizeNotes(t, [0], -480, 240)?.add[0]?.length).toBe(240);
  });

  it("toggles techniques, with bends stepping and hammer-ons pushing out pull-offs", () => {
    const t = tab();
    expect(toggleTechnique(t, [0, 1], "vibrato")?.add.every((n) => n.vibrato)).toBe(true);
    expect(toggleTechnique(t, [2], "hammerOn")?.add[0]?.hammerOn).toBeUndefined();
    expect(toggleTechnique(t, [2], "pullOff")?.add[0]).toMatchObject({ pullOff: true });
    expect(toggleTechnique(t, [2], "pullOff")?.add[0]?.hammerOn).toBeUndefined();
    const bent = (semitones?: number) => {
      const u = tab();
      if (semitones) (u.notes[0] as (typeof u.notes)[0]).bend = { semitones };
      return toggleTechnique(u, [0], "bend")?.add[0]?.bend?.semitones;
    };
    expect([bent(), bent(1), bent(2), bent(3)]).toEqual([1, 2, 3, undefined]);
  });

  it("applies an edit as the engine does and says where the new notes went", () => {
    const t = tab();
    const edit = typeFret(t, { string: 1, tick: 0 }, 3, 240);
    if (!edit) throw new Error("no edit");
    const { notes, added } = applyEdit(t.notes, edit);
    // By start, the lowest string first: 6, 5, then 1 at tick 0.
    expect(notes.map((n) => n.string)).toEqual([6, 5, 1, 6]);
    expect(added).toEqual([2]);
    expect(applyEdit(t.notes, removeNotes([0, 2]) ?? { remove: [], add: [] }).notes).toHaveLength(
      1,
    );
  });

  it("moves the caret on the grid and the strings", () => {
    const t = tab();
    expect(caretStep({ string: 1, tick: 250 }, 240, 1)).toEqual({ string: 1, tick: 480 });
    expect(caretStep({ string: 1, tick: 0 }, 240, -1)).toEqual({ string: 1, tick: 0 });
    expect(caretString(t, { string: 1, tick: 0 }, -1).string).toBe(1);
    expect(caretString(t, { string: 5, tick: 0 }, 1).string).toBe(6);
  });
});
