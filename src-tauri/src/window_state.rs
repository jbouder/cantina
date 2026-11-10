//! The main window's size and state, remembered between launches.
//!
//! The first launch opens full screen. After that the window comes back
//! the way it was left: full screen, zoomed, or at its last size and
//! place. The state lives in `window.json` beside the settings. A missing
//! or unreadable file counts as a first launch.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, Window, WindowEvent,
};

/// The label Tauri gives the window declared in `tauri.conf.json`.
const MAIN: &str = "main";
/// The smallest window the layout works in, in logical pixels; the same
/// as `minWidth` and `minHeight` in `tauri.conf.json`.
const MIN_SIZE: (f64, f64) = (800.0, 520.0);

/// What is remembered about the window. Sizes and positions are physical
/// pixels, as the window reports them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowState {
    /// In macOS full screen, on a space of its own.
    pub fullscreen: bool,
    /// Zoomed to fill the screen, with the title bar kept.
    pub maximized: bool,
    /// The window's size when it is neither full screen nor zoomed.
    pub width: u32,
    pub height: u32,
    /// Its top left corner then; `None` until it has been placed.
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for WindowState {
    /// A first launch: full screen, and the size from `tauri.conf.json`
    /// for when the user leaves it.
    fn default() -> Self {
        Self {
            fullscreen: true,
            maximized: false,
            width: 0,
            height: 0,
            x: None,
            y: None,
        }
    }
}

/// A screen's place on the desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What to do to the window at launch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restore {
    /// The size to give it, or `None` to keep the configured one.
    pub size: Option<(u32, u32)>,
    /// Where to put it, or `None` to leave it where the system does.
    pub position: Option<(i32, i32)>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl WindowState {
    /// Reads the file. `None` means a first launch: no file, or one that
    /// does not parse.
    pub fn load(path: &Path) -> Option<WindowState> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes the file, creating its folder.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)
    }

    /// The launch plan for this state on these screens. A size below the
    /// minimum (given in physical pixels) is dropped, and so is a
    /// position whose title bar would land on no screen, as when a
    /// second display has been unplugged.
    pub fn restore(&self, screens: &[Screen], min: (u32, u32)) -> Restore {
        let size =
            (self.width >= min.0 && self.height >= min.1).then_some((self.width, self.height));
        let position = match (self.x, self.y) {
            (Some(x), Some(y)) if size.is_some() && on_a_screen(x, y, self.width, screens) => {
                Some((x, y))
            }
            _ => None,
        };
        Restore {
            size,
            position,
            maximized: self.maximized,
            fullscreen: self.fullscreen,
        }
    }
}

/// True when enough of the window's top edge is on a screen to grab it.
fn on_a_screen(x: i32, y: i32, width: u32, screens: &[Screen]) -> bool {
    const GRAB: i64 = 80;
    let (left, right) = (i64::from(x), i64::from(x) + i64::from(width));
    screens.iter().any(|s| {
        let (sl, sr) = (i64::from(s.x), i64::from(s.x) + i64::from(s.width));
        let (st, sb) = (i64::from(s.y), i64::from(s.y) + i64::from(s.height));
        let shown = right.min(sr) - left.max(sl);
        shown >= GRAB && i64::from(y) >= st && i64::from(y) < sb
    })
}

/// The state as the app tracks it while running, and where it is saved.
pub struct Tracker {
    path: Option<PathBuf>,
    state: Mutex<WindowState>,
}

impl Tracker {
    fn write(&self) {
        let Some(path) = &self.path else { return };
        let state = *self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = state.save(path) {
            eprintln!("[window] could not save {}: {e}", path.display());
        }
    }
}

/// Applies the remembered state to the main window and starts tracking
/// it. Call once, from `setup`.
pub fn restore(app: &AppHandle) {
    let path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window.json"));
    let state = path
        .as_deref()
        .and_then(WindowState::load)
        .unwrap_or_default();
    if let Some(window) = app.get_webview_window(MAIN) {
        apply(&window, &state);
    }
    app.manage(Tracker {
        path,
        state: Mutex::new(state),
    });
}

fn apply(window: &WebviewWindow, state: &WindowState) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let min = (
        (MIN_SIZE.0 * scale).round() as u32,
        (MIN_SIZE.1 * scale).round() as u32,
    );
    let screens: Vec<Screen> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Screen {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        })
        .collect();
    let plan = state.restore(&screens, min);
    // A window that will not go where it is asked still opens; these are
    // best effort.
    if let Some((w, h)) = plan.size {
        let _ = window.set_size(PhysicalSize::new(w, h));
    }
    if let Some((x, y)) = plan.position {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    if plan.maximized {
        let _ = window.maximize();
    }
    if plan.fullscreen {
        let _ = window.set_fullscreen(true);
    }
}

/// Keeps the tracked state current as the main window changes, and saves
/// it when the window loses focus or closes. Call from `on_window_event`.
pub fn on_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    let Some(tracker) = window.try_state::<Tracker>() else {
        return;
    };
    match event {
        WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
            let fullscreen = window.is_fullscreen().unwrap_or(false);
            let maximized = window.is_maximized().unwrap_or(false);
            let mut state = tracker.state.lock().unwrap_or_else(|e| e.into_inner());
            state.fullscreen = fullscreen;
            state.maximized = maximized && !fullscreen;
            // The size and place to come back to are the ones the window
            // had as a plain window; full screen and zoom do not replace
            // them. A minimized window reports no size.
            if !fullscreen && !maximized && !window.is_minimized().unwrap_or(false) {
                if let Ok(size) = window.inner_size() {
                    if size.width > 0 && size.height > 0 {
                        state.width = size.width;
                        state.height = size.height;
                    }
                }
                if let Ok(at) = window.outer_position() {
                    state.x = Some(at.x);
                    state.y = Some(at.y);
                }
            }
        }
        WindowEvent::Focused(false)
        | WindowEvent::CloseRequested { .. }
        | WindowEvent::Destroyed => {
            tracker.write();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Screen = Screen {
        x: 0,
        y: 0,
        width: 3024,
        height: 1964,
    };
    const MIN: (u32, u32) = (1600, 1040);

    #[test]
    fn a_first_launch_is_full_screen_at_the_configured_size() {
        let dir = std::env::temp_dir().join(format!("cantina-window-{}", std::process::id()));
        let path = dir.join("window.json");
        let _ = fs::remove_file(&path);
        assert_eq!(WindowState::load(&path), None, "no file yet");
        let plan = WindowState::default().restore(&[LAPTOP], MIN);
        assert!(plan.fullscreen);
        assert!(!plan.maximized);
        assert_eq!(plan.size, None, "keeps the size in tauri.conf.json");
        assert_eq!(plan.position, None);
    }

    #[test]
    fn a_window_left_plain_comes_back_at_its_size_and_place() {
        let dir = std::env::temp_dir().join(format!("cantina-window-rt-{}", std::process::id()));
        let path = dir.join("window.json");
        let state = WindowState {
            fullscreen: false,
            maximized: false,
            width: 2400,
            height: 1500,
            x: Some(200),
            y: Some(120),
        };
        state.save(&path).unwrap();
        let loaded = WindowState::load(&path).unwrap();
        assert_eq!(loaded, state);
        let plan = loaded.restore(&[LAPTOP], MIN);
        assert!(!plan.fullscreen, "leaving full screen is remembered");
        assert_eq!(plan.size, Some((2400, 1500)));
        assert_eq!(plan.position, Some((200, 120)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_window_on_an_unplugged_display_is_not_put_back_there() {
        let state = WindowState {
            fullscreen: false,
            maximized: false,
            width: 2400,
            height: 1500,
            x: Some(3500),
            y: Some(100),
        };
        let plan = state.restore(&[LAPTOP], MIN);
        assert_eq!(plan.size, Some((2400, 1500)), "the size still holds");
        assert_eq!(plan.position, None, "the system places it");
        // With the second display back, it returns.
        let second = Screen {
            x: 3024,
            y: 0,
            width: 2560,
            height: 1440,
        };
        assert_eq!(
            state.restore(&[LAPTOP, second], MIN).position,
            Some((3500, 100))
        );
    }

    #[test]
    fn a_size_below_the_minimum_or_a_bad_file_is_dropped() {
        let small = WindowState {
            fullscreen: false,
            maximized: true,
            width: 400,
            height: 300,
            x: Some(10),
            y: Some(10),
        };
        let plan = small.restore(&[LAPTOP], MIN);
        assert_eq!(plan.size, None);
        assert_eq!(plan.position, None, "no place without a size");
        assert!(plan.maximized, "zoom is still remembered");

        let dir = std::env::temp_dir().join(format!("cantina-window-bad-{}", std::process::id()));
        let path = dir.join("window.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "not json").unwrap();
        assert_eq!(WindowState::load(&path), None, "counts as a first launch");
        // An older file with fewer fields still loads.
        fs::write(&path, r#"{"fullscreen":false}"#).unwrap();
        let old = WindowState::load(&path).unwrap();
        assert!(!old.fullscreen);
        assert_eq!(old.restore(&[LAPTOP], MIN).size, None);
        let _ = fs::remove_dir_all(&dir);
    }
}
