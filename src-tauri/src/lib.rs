//! The Cantina shell: a Tauri 2 app that owns the engine handle and
//! exposes it to the React UI as commands.
//!
//! The engine never reaches the web view directly. It lives in managed
//! state behind a mutex, and every command locks it for the duration of
//! one call. Continuous data (meters, later analysis) will travel the
//! other way over channels.

mod commands;
mod engine;

use engine::EngineState;
use tauri::{Manager, RunEvent};

pub fn run() {
    tauri::Builder::default()
        .manage(EngineState::default())
        .invoke_handler(tauri::generate_handler![
            commands::engine_status,
            commands::engine_start,
            commands::engine_stop,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Cantina shell")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Close the stream before the process goes away so the
                // audio units stop cleanly.
                app.state::<EngineState>().lock().stop();
            }
        });
}
