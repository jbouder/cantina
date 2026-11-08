//! The menu bar, built from the action registry the UI shares
//! (`ui/src/lib/actions.json`).
//!
//! The registry is the one place an action's label and shortcut live. The
//! shell builds the native menu from it at launch; the UI reads the same
//! file to match keys and to run the action. A menu item never does
//! anything itself: choosing it emits `menu://action` with the action's
//! id to the main window, which runs the same handler a button or a key
//! does. The UI reports which items apply (enabled, checked, label)
//! through `menu_state`.
//!
//! Shortcuts with a modifier (⌘E) are native accelerators and belong to
//! the menu. Bare keys (Space, R) stay in the web view, which ignores them
//! while a text field has focus; a menu accelerator would take them from
//! the field.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use tauri::menu::{
    AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem,
    Submenu,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The registry, compiled in so the menu exists before the UI loads.
const REGISTRY: &str = include_str!("../../ui/src/lib/actions.json");

/// The event a chosen menu item emits to the main window, with its id.
pub const ACTION_EVENT: &str = "menu://action";

#[derive(Debug, Deserialize)]
struct Registry {
    actions: BTreeMap<String, Action>,
    menus: Vec<MenuSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Action {
    label: String,
    #[serde(default)]
    accelerator: Option<String>,
    /// A bare key, handled by the web view; the menu shows no shortcut
    /// for it. Read only by the tests, which keep it apart from the
    /// accelerators.
    #[serde(default)]
    #[cfg_attr(not(test), allow(dead_code))]
    key: Option<String>,
    #[serde(default)]
    checkable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuSpec {
    title: String,
    items: Vec<String>,
}

/// One entry in a menu's `items`.
#[derive(Debug, PartialEq)]
enum Entry<'a> {
    Separator,
    /// A macOS item the system implements (`@copy`, `@quit`).
    Predefined(&'a str),
    Action(&'a str),
}

fn entry(item: &str) -> Entry<'_> {
    if item == "-" {
        Entry::Separator
    } else if let Some(name) = item.strip_prefix('@') {
        Entry::Predefined(name)
    } else {
        Entry::Action(item)
    }
}

/// The macOS items a menu can name with `@`; `predefined` builds each.
#[cfg(test)]
const PREDEFINED: &[&str] = &[
    "about",
    "services",
    "hide",
    "hideOthers",
    "showAll",
    "quit",
    "closeWindow",
    "cut",
    "copy",
    "paste",
    "selectAll",
    "fullscreen",
    "minimize",
    "maximize",
    "bringAllToFront",
];

fn registry() -> Registry {
    serde_json::from_str(REGISTRY).expect("actions.json is checked by the menu tests")
}

/// The menu bar's action items by id, so `menu_state` can update them.
pub struct MenuItems<R: Runtime>(HashMap<String, MenuItemKind<R>>);

fn predefined<R: Runtime>(app: &AppHandle<R>, name: &str) -> tauri::Result<PredefinedMenuItem<R>> {
    match name {
        "about" => PredefinedMenuItem::about(
            app,
            Some("About Cantina"),
            Some(AboutMetadata {
                name: Some("Cantina".into()),
                version: Some(app.package_info().version.to_string()),
                license: Some("MIT".into()),
                ..Default::default()
            }),
        ),
        "services" => PredefinedMenuItem::services(app, None),
        "hide" => PredefinedMenuItem::hide(app, Some("Hide Cantina")),
        "hideOthers" => PredefinedMenuItem::hide_others(app, None),
        "showAll" => PredefinedMenuItem::show_all(app, None),
        "quit" => PredefinedMenuItem::quit(app, Some("Quit Cantina")),
        "closeWindow" => PredefinedMenuItem::close_window(app, None),
        "cut" => PredefinedMenuItem::cut(app, None),
        "copy" => PredefinedMenuItem::copy(app, None),
        "paste" => PredefinedMenuItem::paste(app, None),
        "selectAll" => PredefinedMenuItem::select_all(app, None),
        "fullscreen" => PredefinedMenuItem::fullscreen(app, None),
        "minimize" => PredefinedMenuItem::minimize(app, None),
        "maximize" => PredefinedMenuItem::maximize(app, Some("Zoom")),
        "bringAllToFront" => PredefinedMenuItem::bring_all_to_front(app, None),
        other => unreachable!("unknown predefined menu item @{other}, caught by the menu tests"),
    }
}

/// Builds the menu bar from the registry and keeps its action items in
/// managed state for `menu_state`.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let registry = registry();
    let menu = Menu::new(app)?;
    let mut items = HashMap::new();
    for spec in &registry.menus {
        let submenu = Submenu::new(app, &spec.title, true)?;
        for item in &spec.items {
            match entry(item) {
                Entry::Separator => submenu.append(&PredefinedMenuItem::separator(app)?)?,
                Entry::Predefined(name) => submenu.append(&predefined(app, name)?)?,
                Entry::Action(id) => {
                    let action = &registry.actions[id];
                    let accelerator = action.accelerator.as_deref();
                    let kind = if action.checkable {
                        let item = CheckMenuItem::with_id(
                            app,
                            id,
                            &action.label,
                            true,
                            false,
                            accelerator,
                        )?;
                        submenu.append(&item)?;
                        MenuItemKind::Check(item)
                    } else {
                        let item = MenuItem::with_id(app, id, &action.label, true, accelerator)?;
                        submenu.append(&item)?;
                        MenuItemKind::MenuItem(item)
                    };
                    items.insert(id.to_owned(), kind);
                }
            }
        }
        if spec.title == "Window" {
            submenu.set_as_windows_menu_for_nsapp()?;
        }
        menu.append(&submenu as &dyn IsMenuItem<R>)?;
    }
    app.manage(MenuItems(items));
    Ok(menu)
}

/// Sends a chosen action item to the main window. Predefined items are
/// handled by macOS and never reach here.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if app
        .try_state::<MenuItems<R>>()
        .is_some_and(|items| items.0.contains_key(id))
    {
        if let Err(e) = app.emit_to("main", ACTION_EVENT, id) {
            eprintln!("[menu] could not send {id}: {e}");
        }
    }
}

/// What the UI says about one action item.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    id: String,
    enabled: bool,
    #[serde(default)]
    checked: Option<bool>,
    #[serde(default)]
    label: Option<String>,
}

/// Brings the action items in line with the UI's state. Unknown ids are
/// ignored, so a newer UI never breaks an older menu.
pub fn apply<R: Runtime>(app: &AppHandle<R>, states: &[ItemState]) -> tauri::Result<()> {
    let Some(items) = app.try_state::<MenuItems<R>>() else {
        return Ok(());
    };
    for state in states {
        match items.0.get(&state.id) {
            Some(MenuItemKind::MenuItem(item)) => {
                item.set_enabled(state.enabled)?;
                if let Some(label) = &state.label {
                    item.set_text(label)?;
                }
            }
            Some(MenuItemKind::Check(item)) => {
                item.set_enabled(state.enabled)?;
                if let Some(checked) = state.checked {
                    item.set_checked(checked)?;
                }
                if let Some(label) = &state.label {
                    item.set_text(label)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::str::FromStr;

    #[test]
    fn every_action_is_in_exactly_one_menu() {
        let registry = registry();
        let mut seen = HashSet::new();
        for spec in &registry.menus {
            for item in &spec.items {
                match entry(item) {
                    Entry::Separator => {}
                    Entry::Predefined(name) => {
                        assert!(
                            PREDEFINED.contains(&name),
                            "unknown predefined item @{name}"
                        )
                    }
                    Entry::Action(id) => {
                        assert!(
                            registry.actions.contains_key(id),
                            "{} names {id}, not an action",
                            spec.title
                        );
                        assert!(seen.insert(id), "{id} is in more than one menu");
                    }
                }
            }
        }
        for id in registry.actions.keys() {
            assert!(seen.contains(id.as_str()), "{id} is in no menu");
        }
    }

    #[test]
    fn shortcuts_parse_and_never_take_keys_from_text_fields() {
        let registry = registry();
        let mut taken = HashSet::new();
        for (id, action) in &registry.actions {
            assert!(
                action.accelerator.is_none() || action.key.is_none(),
                "{id} has both an accelerator and a key"
            );
            if let Some(accel) = &action.accelerator {
                muda::accelerator::Accelerator::from_str(accel)
                    .unwrap_or_else(|e| panic!("{id}: {accel} does not parse: {e}"));
                assert!(
                    accel.contains("CmdOrCtrl") || accel.contains("Cmd") || accel.contains("Ctrl"),
                    "{id}: a menu accelerator needs ⌘ or ⌃, or it would take the key from text fields"
                );
                assert!(taken.insert(accel.clone()), "{accel} is used twice");
            }
            if let Some(key) = &action.key {
                assert!(
                    !key.contains('+'),
                    "{id}: a key has no modifiers; use an accelerator"
                );
                assert!(taken.insert(key.clone()), "{key} is used twice");
            }
        }
    }

    #[test]
    fn the_app_menu_comes_first() {
        let registry = registry();
        assert_eq!(registry.menus[0].title, "Cantina");
        assert_eq!(registry.menus[0].items[0], "@about");
    }
}
