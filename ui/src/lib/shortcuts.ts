import { ACTIONS, type ActionId, MENUS, type MenuSpec, menuNames } from "./actions";
import registry from "./actions.json";

/**
 * The keyboard shortcut reference, built from the action registry: the
 * menu items that have a shortcut, menu by menu, and the keys that work
 * only in one part of the window. The Help ▸ Keyboard Shortcuts sheet and
 * the docs page `reference/shortcuts.md` are both drawn from this, and a
 * test holds the page equal to it.
 */

/** One row: the keys and what they do. */
export interface ShortcutRow {
  keys: string;
  does: string;
}

/** A group of rows: a menu, or a part of the window. */
export interface ShortcutSection {
  title: string;
  note?: string;
  rows: ShortcutRow[];
}

const SYMBOLS: Record<string, string> = {
  cmdorctrl: "⌘",
  cmd: "⌘",
  super: "⌘",
  ctrl: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
};

/** Mac order for modifiers: ⌃⌥⇧⌘. */
const ORDER = ["⌃", "⌥", "⇧", "⌘"];

const KEYS: Record<string, string> = {
  comma: ",",
  equal: "=",
  minus: "−",
  period: ".",
  slash: "/",
  space: "Space",
  enter: "Return",
  escape: "Esc",
  backspace: "⌫",
  delete: "⌦",
  home: "Home",
  end: "End",
};

/** A shortcut as a Mac menu shows it: `Shift+CmdOrCtrl+Z` is `⇧⌘Z`. */
export function formatShortcut(shortcut: string): string {
  const parts = shortcut.split("+");
  const key = parts.pop() ?? "";
  const mods = parts
    .map((p) => SYMBOLS[p.toLowerCase()] ?? p)
    .sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  const name = KEYS[key.toLowerCase()] ?? key.toUpperCase();
  return mods.join("") + name;
}

function menuRows(menu: MenuSpec): ShortcutRow[] {
  const rows: ShortcutRow[] = [];
  for (const name of menuNames(menu)) {
    const spec = ACTIONS[name as ActionId];
    const shortcut = spec?.accelerator ?? spec?.key;
    if (spec && shortcut) rows.push({ keys: formatShortcut(shortcut), does: spec.label });
  }
  return rows;
}

/** Every shortcut, menu by menu, then the keys of each part of the window. */
export function shortcutSections(): ShortcutSection[] {
  const menus = MENUS.map((m) => ({ title: m.title, rows: menuRows(m) })).filter(
    (s) => s.rows.length > 0,
  );
  const contexts = registry.contexts.map((c) => ({
    title: c.title,
    note: c.note,
    rows: c.keys.map(([keys, does]) => ({ keys: keys ?? "", does: does ?? "" })),
  }));
  return [...menus, ...contexts];
}

/** The docs page, `docs/src/content/docs/reference/shortcuts.md`. */
export function shortcutsMarkdown(): string {
  const lines = [
    "---",
    'title: "Keyboard shortcuts"',
    'description: "Every keyboard shortcut in Cantina, by menu and by part of the window."',
    "---",
    "",
    "<!-- Generated from ui/src/lib/actions.json by ui/src/lib/shortcuts.ts. Do not edit: run UPDATE_SHORTCUTS=1 npx vitest run shortcuts. -->",
    "",
    "The same list is in the app under **Help ▸ Keyboard Shortcuts**. Keys",
    "without ⌘ do nothing while you are typing in a text field. The",
    "[menus guide](/guides/menus/) says what each item does.",
  ];
  for (const section of shortcutSections()) {
    lines.push("", `## ${section.title}`, "");
    if (section.note) lines.push(section.note, "");
    lines.push("| Keys | Does |", "| --- | --- |");
    for (const row of section.rows) lines.push(`| ${row.keys} | ${row.does} |`);
  }
  return `${lines.join("\n")}\n`;
}
