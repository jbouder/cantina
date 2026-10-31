//! The Cantina shell: a Tauri 2 app that owns the engine handle and
//! exposes it to the React UI as commands.
//!
//! The engine never reaches the web view directly. It lives in managed
//! state behind a mutex, and every command locks it for the duration of
//! one call. Continuous data (meters, later analysis) travels the other
//! way over channels, and status changes the engine makes on its own are
//! pushed as events.

mod commands;
mod engine;
mod settings;

use engine::EngineState;
use tauri::{Manager, RunEvent};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(EngineState::new(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine_status,
            commands::engine_start,
            commands::engine_stop,
            commands::dismiss_notice,
            commands::list_devices,
            commands::audio_settings,
            commands::set_audio_settings,
            commands::subscribe_meters,
            commands::track_arm,
            commands::track_record,
            commands::track_play,
            commands::track_stop,
            commands::chain_add,
            commands::chain_remove,
            commands::chain_move,
            commands::chain_set_param,
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
