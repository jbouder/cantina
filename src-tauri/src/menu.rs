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

/// The event File ▸ Open Recent emits to the main window, with the
/// project's name. The UI opens it as the Projects sheet would.
pub const OPEN_RECENT_EVENT: &str = "menu://open-recent";

/// Ids of Open Recent's items: the prefix and a project's name, or Clear.
const RECENT_PREFIX: &str = "recent:";
const RECENT_CLEAR: &str = "recent-clear";

/// Shown in the About window.
const CREDITS: &str = "A guitar studio for the Mac. Drum samples and \
cabinet impulse responses are credited in kits/CREDITS.md and \
irs/CREDITS.md; the visualizer scenes come from music-visualizer.";

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
    items: Vec<Item>,
}

/// A menu's item: a name (an action, `-` or `@` item) or a submenu.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Item {
    Name(String),
    Submenu(MenuSpec),
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
    "openRecent",
];

fn registry() -> Registry {
    serde_json::from_str(REGISTRY).expect("actions.json is checked by the menu tests")
}

/// The menu bar's action items by id, so `menu_state` can update them.
pub struct MenuItems<R: Runtime>(HashMap<String, MenuItemKind<R>>);

/// File ▸ Open Recent, filled by `set_recent`.
pub struct RecentMenu<R: Runtime>(Submenu<R>);

fn predefined<R: Runtime>(app: &AppHandle<R>, name: &str) -> tauri::Result<PredefinedMenuItem<R>> {
    match name {
        "about" => PredefinedMenuItem::about(
            app,
            Some("About Cantina"),
            Some(AboutMetadata {
                name: Some("Cantina".into()),
                version: Some(app.package_info().version.to_string()),
                copyright: Some("© 2026 Johnny Bouder. MIT License.".into()),
                license: Some("MIT".into()),
                comments: Some(CREDITS.into()),
                website: Some("https://cantina-docs.vibeproject.workers.dev/".into()),
                website_label: Some("Cantina docs".into()),
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
        let submenu = submenu(app, &registry, spec, &mut items)?;
        match spec.title.as_str() {
            "Window" => submenu.set_as_windows_menu_for_nsapp()?,
            // macOS adds its search field to the Help menu.
            "Help" => submenu.set_as_help_menu_for_nsapp()?,
            _ => {}
        }
        menu.append(&submenu as &dyn IsMenuItem<R>)?;
    }
    app.manage(MenuItems(items));
    Ok(menu)
}

/// One menu of the registry, its submenus built too.
fn submenu<R: Runtime>(
    app: &AppHandle<R>,
    registry: &Registry,
    spec: &MenuSpec,
    items: &mut HashMap<String, MenuItemKind<R>>,
) -> tauri::Result<Submenu<R>> {
    let submenu = Submenu::new(app, &spec.title, true)?;
    for item in &spec.items {
        let name = match item {
            Item::Submenu(inner) => {
                submenu.append(&self::submenu(app, registry, inner, items)?)?;
                continue;
            }
            Item::Name(name) => name,
        };
        match entry(name) {
            Entry::Separator => submenu.append(&PredefinedMenuItem::separator(app)?)?,
            Entry::Predefined("openRecent") => {
                let recent = Submenu::new(app, "Open Recent", true)?;
                submenu.append(&recent)?;
                app.manage(RecentMenu(recent));
            }
            Entry::Predefined(name) => submenu.append(&predefined(app, name)?)?,
            Entry::Action(id) => {
                let action = &registry.actions[id];
                let accelerator = action.accelerator.as_deref();
                let kind = if action.checkable {
                    let item =
                        CheckMenuItem::with_id(app, id, &action.label, true, false, accelerator)?;
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
    Ok(submenu)
}

/// The docs site.
/// The docs site.
pub(crate) const DOCS: &str = "https://cantina-docs.vibeproject.workers.dev/";
/// The repository on GitHub.
const REPO: &str = "https://github.com/jbouder/cantina";

/// Where the Help menu's links go, by name. A fixed list, so the web view
/// can never ask the shell to open an arbitrary URL. `version` fills in
/// the new issue's body.
pub fn link_url(which: &str, version: &str) -> Option<String> {
    match which {
        "docs" => Some(DOCS.to_string()),
        "shortcuts" => Some(format!("{DOCS}guides/menus/")),
        "assistant" => Some(format!("{DOCS}guides/assistant/#what-is-sent-and-when")),
        "releases" => Some(format!("{REPO}/releases")),
        "issue" => {
            let body = format!(
                "What happened, and what you expected:\n\n\n---\nCantina {version} on macOS"
            );
            Some(format!("{REPO}/issues/new?body={}", percent_encode(&body)))
        }
        _ => None,
    }
}

/// Encodes a query value: everything but unreserved characters as %XX.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Fills File ▸ Open Recent with `names`, newest first, and Clear Menu.
pub fn set_recent<R: Runtime>(app: &AppHandle<R>, names: &[String]) {
    let Some(menu) = app.try_state::<RecentMenu<R>>() else {
        return;
    };
    let fill = || -> tauri::Result<()> {
        let menu = &menu.0;
        for item in menu.items()? {
            menu.remove(&item)?;
        }
        for name in names {
            let id = format!("{RECENT_PREFIX}{name}");
            menu.append(&MenuItem::with_id(app, id, name, true, None::<&str>)?)?;
        }
        if !names.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        menu.append(&MenuItem::with_id(
            app,
            RECENT_CLEAR,
            "Clear Menu",
            !names.is_empty(),
            None::<&str>,
        )?)
    };
    if let Err(e) = fill() {
        eprintln!("[menu] could not fill Open Recent: {e}");
    }
}

/// Sends a chosen action item to the main window, and handles Open
/// Recent. Predefined items are handled by macOS and never reach here.
pub fn on_event(app: &AppHandle, id: &str) {
    if id == RECENT_CLEAR {
        app.state::<crate::engine::EngineState>()
            .lock()
            .recent_clear();
        return;
    }
    if let Some(name) = id.strip_prefix(RECENT_PREFIX) {
        if let Err(e) = app.emit_to("main", OPEN_RECENT_EVENT, name) {
            eprintln!("[menu] could not open {name}: {e}");
        }
        return;
    }
    if app
        .try_state::<MenuItems<tauri::Wry>>()
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
        fn walk<'a>(registry: &Registry, spec: &'a MenuSpec, seen: &mut HashSet<&'a str>) {
            for item in &spec.items {
                let name = match item {
                    Item::Submenu(inner) => {
                        walk(registry, inner, seen);
                        continue;
                    }
                    Item::Name(name) => name,
                };
                match entry(name) {
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
        let registry = registry();
        let mut seen = HashSet::new();
        for spec in &registry.menus {
            walk(&registry, spec, &mut seen);
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
    fn help_links_are_a_fixed_list() {
        assert_eq!(link_url("docs", "1.0.0").unwrap(), DOCS);
        assert!(link_url("assistant", "1.0.0")
            .unwrap()
            .ends_with("/guides/assistant/#what-is-sent-and-when"));
        assert!(link_url("releases", "1.0.0")
            .unwrap()
            .ends_with("/releases"));
        let issue = link_url("issue", "1.2.3").unwrap();
        assert!(issue.starts_with("https://github.com/jbouder/cantina/issues/new?body="));
        assert!(issue.contains("Cantina%201.2.3%20on%20macOS"));
        assert!(!issue[issue.find('=').unwrap()..].contains(' '));
        assert_eq!(link_url("https://example.com", "1.0.0"), None);
    }

    #[test]
    fn the_app_menu_comes_first() {
        let registry = registry();
        assert_eq!(registry.menus[0].title, "Cantina");
        assert!(matches!(&registry.menus[0].items[0], Item::Name(n) if n == "@about"));
    }
}
