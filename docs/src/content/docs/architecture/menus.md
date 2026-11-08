---
title: "Menus and shortcuts"
description: "One action registry drives the macOS menu bar and the keyboard: where it lives, how a menu item reaches the UI, and how to add an action."
sidebar:
  order: 9.5
---

Every command the menu bar and the keyboard can run is an **action** in one
registry, `ui/src/lib/actions.json`. The [shell](/architecture/shell/) builds the
native menu bar from it ([what each item does](/guides/menus/)), and the [UI](/architecture/ui/) reads the same file to
match keys and run handlers. A menu item and its shortcut cannot disagree,
because there is only one place that says what they are.

```json
{
  "actions": {
    "export": { "label": "Export Mixdown…", "accelerator": "CmdOrCtrl+E" },
    "play": { "label": "Play", "key": "Space" },
    "metronome": { "label": "Metronome", "key": "K", "checkable": true }
  },
  "menus": [
    { "title": "File", "items": ["export", "-", "@closeWindow"] }
  ]
}
```

- An **action** has a `label` and at most one shortcut: an `accelerator`
  with ⌘ or ⌃, in Tauri's syntax, or a bare `key`. `checkable` actions show a
  check mark.
- A **menu** lists action ids, `-` for a separator, and `@` for items macOS
  implements itself (`@copy`, `@paste`, `@quit`, `@minimize`, and the rest
  listed in `src-tauri/src/menu.rs`). The first menu is the app menu.
- `@openRecent` is the one item the shell fills itself: File ▸ Open Recent,
  from the recent projects kept in the settings. Choosing a project emits
  `menu://open-recent` with its name, and the UI opens it as the Projects
  sheet would, asking first when there are unsaved edits. **Clear Menu** is
  handled in the shell.

## From a menu item to a handler

1. At launch `menu.rs` builds the menu bar from the registry, which is
   compiled into the shell, so the menu exists before the UI loads.
2. Choosing an item emits `menu://action` with the action's id to the main
   window. Menu items never do anything in the shell themselves.
3. `App.tsx` holds a binding for every action: what it runs, and whether
   it applies now (enabled, checked, a label such as Play turning into
   Stop). A menu item, a key and a button all run the same handler.
4. The UI sends the bindings' state to the shell through `menu_state`
   whenever it changes, and the shell updates the native items. An item
   that cannot apply, such as Record with no armed track, is disabled.

## Who owns which key

- **Accelerators** (⌘E, ⇧⌘Z) belong to the native menu. In the app the web
  view ignores them, so an action never runs twice; in a browser, where
  there is no menu bar, the web view handles them.
- **Bare keys** (Space, R, K, P) stay in the web view and never fire while
  a text field has focus, or on a control that uses the key itself. As
  menu accelerators they would take the key away from text fields, so the
  menu shows the item without a shortcut.
- **⌘Z and ⇧⌘Z** in a text field are the field's own undo: the Undo
  action checks for focus and hands the field the undo, and stays enabled
  while a field has focus even when the project has nothing to undo.

## Adding an action

1. Add it to `actions` in `ui/src/lib/actions.json` and put its id in one
   menu.
2. Add its binding in `App.tsx`: `run`, `enabled`, and `checked` or
   `label` if it has them. TypeScript fails until every action is bound.
3. Run `cargo test -p cantina menu` and `npm run check`. The tests fail on
   an action in no menu or in two, an unknown `@` item, an accelerator
   that does not parse or lacks ⌘ or ⌃, and a shortcut used twice.
