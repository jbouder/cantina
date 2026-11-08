import registry from "./actions.json";

/**
 * The action registry: every command the menu bar and the keyboard can
 * run, with its label and shortcut, in `actions.json`. The shell builds
 * the native menu from the same file (`src-tauri/src/menu.rs`), so a
 * menu item and its shortcut cannot disagree.
 */
export type ActionId = keyof typeof registry.actions;

/** An action as the registry describes it. */
export interface ActionSpec {
  label: string;
  /** A shortcut with a modifier, in Tauri's accelerator syntax: `CmdOrCtrl+E`. Owned by the native menu. */
  accelerator?: string;
  /** A bare key (`Space`, `R`), handled in the web view and never while typing. */
  key?: string;
  /** Shown with a check mark when on. */
  checkable?: boolean;
}

export const ACTIONS = registry.actions as Record<ActionId, ActionSpec>;
export const ACTION_IDS = Object.keys(ACTIONS) as ActionId[];

/** One menu of the menu bar: action ids, `-` separators and `@` macOS items. */
export const MENUS: { title: string; items: string[] }[] = registry.menus;

/** Where an action stands now: whether it applies, whether it is on, and its label. */
export interface ActionState {
  enabled: boolean;
  checked?: boolean;
  /** Replaces the registry's label, as Play becomes Stop. */
  label?: string;
}

/** An action wired to the app: what it does and where it stands. */
export interface Bound extends ActionState {
  run: () => void;
}

export type Bindings = Record<ActionId, Bound>;

/** True while the element takes typed text, where bare keys and ⌘Z belong to it. */
export function isTyping(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "TEXTAREA") return true;
  if (tag === "INPUT") {
    const type = (el as HTMLInputElement).type;
    return ["text", "search", "email", "url", "password", "number", "tel"].includes(type);
  }
  return (el as HTMLElement).isContentEditable === true;
}

/** True on a control that uses Space, Enter or letters itself. */
function ownsKeys(el: Element | null): boolean {
  const tag = el?.tagName;
  return tag === "INPUT" || tag === "SELECT" || tag === "BUTTON" || tag === "TEXTAREA";
}

/** The parts of a key event a shortcut compares. */
export interface KeyPress {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const NAMED: Record<string, string> = {
  space: " ",
  comma: ",",
  period: ".",
  slash: "/",
  enter: "Enter",
  escape: "Escape",
  backspace: "Backspace",
  delete: "Delete",
};

function keyOf(name: string): string {
  return NAMED[name.toLowerCase()] ?? name;
}

/**
 * True when the key press is the accelerator (`Shift+CmdOrCtrl+Z`). Cmd
 * and Ctrl both count for `CmdOrCtrl`, so the browser stand-in works
 * anywhere.
 */
export function matchesAccelerator(accelerator: string, e: KeyPress): boolean {
  const parts = accelerator.split("+");
  const key = keyOf(parts.pop() ?? "");
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const cmd = mods.has("cmdorctrl") || mods.has("cmd") || mods.has("ctrl") || mods.has("super");
  if (cmd !== (e.metaKey || e.ctrlKey)) return false;
  if (mods.has("shift") !== e.shiftKey) return false;
  if ((mods.has("alt") || mods.has("option")) !== e.altKey) return false;
  return e.key.toLowerCase() === key.toLowerCase();
}

/** True when the key press is the bare key (`Space`, `R`), with no modifier. */
export function matchesKey(key: string, e: KeyPress): boolean {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  return e.key.toLowerCase() === keyOf(key).toLowerCase();
}

/**
 * The action a key press runs, if any. `native` is true in the app, where
 * the menu bar owns the accelerators and the web view must not run them
 * a second time. Bare keys never fire while typing or on a control that
 * uses the key itself.
 */
export function actionForKey(
  e: KeyPress,
  target: Element | null,
  native: boolean,
): ActionId | null {
  for (const id of ACTION_IDS) {
    const spec = ACTIONS[id];
    if (spec.accelerator && matchesAccelerator(spec.accelerator, e)) {
      return native ? null : id;
    }
    if (spec.key && matchesKey(spec.key, e)) {
      return isTyping(target) || ownsKeys(target) ? null : id;
    }
  }
  return null;
}

/** The menu bar's view of the bindings: one entry per action, for `menu_state`. */
export function menuStates(bindings: Bindings): ({ id: ActionId } & ActionState)[] {
  return ACTION_IDS.map((id) => {
    const { enabled, checked, label } = bindings[id];
    return {
      id,
      enabled,
      ...(ACTIONS[id].checkable ? { checked: checked ?? false } : {}),
      label: label ?? ACTIONS[id].label,
    };
  });
}

/**
 * A menu title for an edit, from its command's description: `move clip`
 * under Undo is `Undo Move Clip`. Lowercase words get a capital, as Mac
 * menus title-case; names in the description keep their case.
 */
export function editTitle(verb: "Undo" | "Redo", description: string | null): string {
  if (!description) return verb;
  const words = description
    .split(" ")
    .map((w) => (/^[a-z]/.test(w) ? w[0]?.toUpperCase() + w.slice(1) : w));
  return `${verb} ${words.join(" ")}`;
}
