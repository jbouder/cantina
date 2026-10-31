//! The Cantina shell: a Tauri 2 app that owns the engine handle and the
//! open project, and exposes both to the React UI as commands.
//!
//! The engine never reaches the web view directly. It lives in managed
//! state behind a mutex, and every command locks it for the duration of
//! one call. Continuous data (meters, later analysis) travels the other
//! way over channels, and status changes the engine makes on its own are
//! pushed as events.

mod commands;
mod engine;
mod frame;
mod project;
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
            commands::ui_log,
            commands::engine_status,
            commands::engine_start,
            commands::engine_stop,
            commands::dismiss_notice,
            commands::list_devices,
            commands::audio_settings,
            commands::set_audio_settings,
            commands::subscribe_meters,
            commands::meter_ack,
            commands::frames_dropped,
            commands::project_list,
            commands::project_new,
            commands::project_open,
            commands::project_save,
            commands::project_recover,
            commands::project_discard_recovery,
            commands::project_undo,
            commands::project_redo,
            commands::tempo_set,
            commands::strip_gain,
            commands::strip_pan,
            commands::strip_mute,
            commands::strip_solo,
            commands::track_add,
            commands::track_remove,
            commands::track_rename,
            commands::track_move,
            commands::track_arm,
            commands::clip_remove,
            commands::clip_move,
            commands::clip_trim,
            commands::clip_split,
            commands::audio_samples,
            commands::transport_play,
            commands::transport_record,
            commands::transport_stop,
            commands::transport_locate,
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
