import type { Tab, TabGrid, TabNote } from "./engine";

/**
 * The tab editor's edits, pure: each takes the tab and what is selected
 * and gives back the notes to remove (indices) and the notes to add, as
 * `clip_tab_edit` takes them, or null when there is nothing to do. The
 * view sends the edit, and the engine makes it one undoable command.
 */

/** Notes in the order the engine keeps them: by start, the lowest string (6 on a guitar) first. */
export function sortTabNotes(notes: TabNote[]): TabNote[] {
  return notes.sort((a, b) => a.start - b.start || b.string - a.string);
}

/** The MIDI note a string (1 the highest) sounds at a fret, with the capo. */
export function tabPitch(tab: Tab, string: number, fret: number): number {
  const open = tab.tuning[tab.tuning.length - string] ?? 0;
  return open + (tab.capo ?? 0) + fret;
}

/** Ticks per step of a grid. */
export const GRID_TICKS: Record<TabGrid, number> = {
  "1/8": 480,
  "1/16": 240,
  "1/8t": 320,
  "1/16t": 160,
};

/** The highest fret the editor types. */
export const MAX_FRET = 24;

/** Where keyboard entry goes: a string (1 the highest) and a tick. */
export interface Caret {
  string: number;
  tick: number;
}

/** An edit as `clip_tab_edit` takes it. */
export interface NotesEdit {
  remove: number[];
  add: TabNote[];
}

/** The techniques a key turns on and off. */
export type Technique =
  | "hammerOn"
  | "pullOff"
  | "slide"
  | "bend"
  | "vibrato"
  | "palmMute"
  | "letRing"
  | "harmonic"
  | "dead"
  | "tap";

/** The key for each technique, as the shortcut reference lists them. */
export const TECHNIQUE_KEYS: Record<string, Technique> = {
  h: "hammerOn",
  p: "pullOff",
  s: "slide",
  b: "bend",
  v: "vibrato",
  m: "palmMute",
  l: "letRing",
  n: "harmonic",
  x: "dead",
  t: "tap",
};

/** The note under the caret: on its string, sounding at its tick. */
export function noteAt(tab: Tab, caret: Caret): number | null {
  const i = tab.notes.findIndex(
    (n) => n.string === caret.string && n.start <= caret.tick && caret.tick < n.start + n.length,
  );
  return i < 0 ? null : i;
}

/** The notes an edit works on: the selection, or the note under the caret. */
export function target(tab: Tab, selection: number[], caret: Caret): number[] {
  if (selection.length > 0) return selection.filter((i) => i < tab.notes.length);
  const i = noteAt(tab, caret);
  return i === null ? [] : [i];
}

/**
 * Types a fret at the caret: the note there gets it, or a new note one
 * grid step long starts there.
 */
export function typeFret(tab: Tab, caret: Caret, fret: number, step: number): NotesEdit | null {
  if (fret < 0 || fret > MAX_FRET) return null;
  const i = noteAt(tab, caret);
  if (i !== null) {
    const was = tab.notes[i] as TabNote;
    if (was.fret === fret && !was.dead) return null;
    const { dead: _, ...rest } = was;
    return { remove: [i], add: [{ ...rest, fret }] };
  }
  return { remove: [], add: [{ string: caret.string, fret, start: caret.tick, length: step }] };
}

/** Removes the notes. */
export function removeNotes(notes: number[]): NotesEdit | null {
  return notes.length > 0 ? { remove: [...notes], add: [] } : null;
}

/** Changes each note, leaving the rest; null if any change is refused. */
function each(tab: Tab, notes: number[], change: (n: TabNote) => TabNote | null): NotesEdit | null {
  if (notes.length === 0) return null;
  const add: TabNote[] = [];
  for (const i of notes) {
    const n = tab.notes[i];
    if (!n) return null;
    const changed = change(n);
    if (!changed) return null;
    add.push(changed);
  }
  return { remove: [...notes], add };
}

/** Moves the notes earlier or later by `ticks`; never before the tab starts. */
export function moveNotes(tab: Tab, notes: number[], ticks: number): NotesEdit | null {
  return each(tab, notes, (n) => (n.start + ticks < 0 ? null : { ...n, start: n.start + ticks }));
}

/**
 * Moves the notes to the next string up (`-1`, toward string 1) or down,
 * keeping their pitch: the fret changes by the strings' interval.
 */
export function moveString(tab: Tab, notes: number[], by: -1 | 1): NotesEdit | null {
  return each(tab, notes, (n) => {
    const string = n.string + by;
    if (string < 1 || string > tab.tuning.length) return null;
    if (n.dead) return { ...n, string };
    const fret = tabPitch(tab, n.string, n.fret) - tabPitch(tab, string, 0);
    return fret < 0 || fret > MAX_FRET ? null : { ...n, string, fret };
  });
}

/** Lengthens or shortens the notes by `ticks`, never below `min`. */
export function resizeNotes(
  tab: Tab,
  notes: number[],
  ticks: number,
  min: number,
): NotesEdit | null {
  return each(tab, notes, (n) => {
    const length = Math.max(min, n.length + ticks);
    return length === n.length ? null : { ...n, length };
  });
}

/**
 * Turns a technique on for every note, or off when every note already
 * has it. A bend steps through ½, full and 1½ and then off; a hammer-on
 * and a pull-off push each other out.
 */
export function toggleTechnique(tab: Tab, notes: number[], technique: Technique): NotesEdit | null {
  const chosen = notes.map((i) => tab.notes[i]).filter((n): n is TabNote => n !== undefined);
  if (chosen.length === 0) return null;
  if (technique === "bend") {
    const steps = [1, 2, 3];
    const first = chosen[0]?.bend?.semitones;
    const at = first === undefined ? -1 : steps.indexOf(first);
    const next = steps[at + 1];
    return each(tab, notes, (n) => {
      const { bend: _, ...rest } = n;
      return next === undefined ? rest : { ...rest, bend: { semitones: next } };
    });
  }
  const on = !chosen.every((n) => n[technique]);
  return each(tab, notes, (n) => {
    const { [technique]: _, ...rest } = n;
    const out: TabNote = on ? { ...rest, [technique]: true } : rest;
    if (on && technique === "hammerOn") delete out.pullOff;
    if (on && technique === "pullOff") delete out.hammerOn;
    return out;
  });
}

/**
 * The tab after an edit, as the engine makes it, and where the added
 * notes landed: the view keeps them selected.
 */
export function applyEdit(
  notes: TabNote[],
  edit: NotesEdit,
): { notes: TabNote[]; added: number[] } {
  const gone = new Set(edit.remove);
  const kept = notes.filter((_, i) => !gone.has(i));
  const all = sortTabNotes([...kept, ...edit.add]);
  const added = edit.add.map((n) => all.indexOf(n));
  return { notes: all, added: added.filter((i) => i >= 0).sort((a, b) => a - b) };
}

/** The caret one string up (`-1`) or down, kept on the tab. */
export function caretString(tab: Tab, caret: Caret, by: number): Caret {
  return { ...caret, string: Math.max(1, Math.min(tab.tuning.length, caret.string + by)) };
}

/** The caret one grid step earlier or later, on the grid. */
export function caretStep(caret: Caret, step: number, by: number): Caret {
  const snapped = Math.round(caret.tick / step) * step;
  return { ...caret, tick: Math.max(0, snapped + by * step) };
}
