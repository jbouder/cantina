//! The Cantina shell: a Tauri 2 app that owns the engine handle and the
//! open project, and exposes both to the React UI as commands.
//!
//! The engine never reaches the web view directly. It lives in managed
//! state behind a mutex, and every command locks it for the duration of
//! one call. Continuous data (meters, later analysis) travels the other
//! way over channels, and status changes the engine makes on its own are
//! pushed as events.

mod assistant;
mod commands;
mod engine;
mod exercises;
mod frame;
mod jam;
mod jams;
mod library;
mod menu;
mod mixdown;
mod pedals;
mod project;
mod rigs;
mod settings;
mod tabs;
mod window_state;

use engine::EngineState;
use tauri::{Manager, RunEvent};

/// Starts the app: builds the Tauri window, manages the engine and project
/// state, and runs until the window closes.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            app.manage(EngineState::new(app.handle().clone()));
            // Full screen on a first launch; after that, as it was left.
            window_state::restore(app.handle());
            // The chat thread, kept beside the plugin cache.
            let thread = app
                .path()
                .app_data_dir()
                .ok()
                .map(|dir| dir.join("assistant-thread.json"));
            app.manage(assistant::Session::new(thread));
            let folders = app.state::<EngineState>().lock().library_folders();
            library::spawn_watcher(app.handle().clone(), folders);
            // The menu bar from the action registry. If it cannot be
            // built, macOS keeps Tauri's default menu and the app runs.
            match menu::build(app.handle()) {
                Ok(bar) => {
                    app.set_menu(bar)?;
                    let recent = app.state::<EngineState>().lock().recent_projects();
                    menu::set_recent(app.handle(), &recent);
                }
                Err(e) => eprintln!("[menu] could not build the menu bar: {e}"),
            }
            Ok(())
        })
        .on_window_event(window_state::on_event)
        .on_menu_event(|app, event| menu::on_event(app, event.id().as_ref()))
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
            commands::plugin_list,
            commands::plugin_rescan,
            commands::project_list,
            commands::project_new,
            commands::project_recent,
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
            commands::clip_duplicate,
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
            commands::latency_warning_set,
            commands::metronome_set,
            commands::track_add_drums,
            commands::track_set_kit,
            commands::analysis_set_source,
            commands::pedal_set_bypass,
            commands::chain_select,
            commands::kits_rescan,
            commands::chain_set_spec,
            commands::irs_rescan,
            commands::pattern_add,
            commands::pattern_remove,
            commands::pattern_rename,
            commands::pattern_set_step,
            commands::pattern_set_length,
            commands::pattern_set_resolution,
            commands::pattern_set_swing,
            commands::pattern_audition,
            commands::groove_audition,
            commands::rig_audition,
            commands::rig_ab,
            commands::jam_open,
            commands::jam_close,
            commands::jam_set,
            commands::jam_play,
            commands::jam_stop,
            commands::jam_section,
            commands::jam_tempo,
            commands::jam_tracks,
            commands::jam_keep,
            commands::jam_keep_place,
            commands::jam_capture_set,
            commands::jam_save,
            commands::jam_load,
            commands::rig_apply,
            commands::rig_save,
            commands::rig_rename,
            commands::rig_delete,
            commands::rig_import,
            commands::rig_export,
            commands::instrument_profile_set,
            commands::pattern_replace,
            commands::drum_rows_set,
            commands::pattern_clip_set_fill,
            commands::pad_mix_set,
            commands::pad_hit,
            commands::pattern_clip_generate_fill,
            commands::pattern_clip_add,
            commands::pattern_clip_remove,
            commands::pattern_clip_move,
            commands::pattern_clip_trim,
            commands::pattern_clip_duplicate,
            commands::pattern_clip_split,
            commands::editor_open,
            commands::editor_close,
            commands::designer_new,
            commands::designer_update,
            commands::pedal_definition,
            commands::pedal_stock_definition,
            commands::pedal_check,
            commands::designer_save,
            commands::pedal_export,
            commands::pedal_import,
            commands::pedals_rescan,
            commands::pattern_import,
            commands::pattern_export,
            commands::library_rescan,
            commands::mixdown_export,
            commands::track_bounce,
            commands::render_cancel,
            commands::clip_transcribe,
            commands::clip_tab,
            commands::clip_tab_requantize,
            commands::clip_tab_refit,
            commands::clip_tab_edit,
            commands::practice_set,
            commands::clip_tab_export,
            commands::tab_pdf_write,
            commands::menu_state,
            commands::project_rename,
            commands::project_delete,
            commands::project_duplicate,
            commands::reveal,
            commands::audio_import,
            commands::kit_import,
            commands::clip_paste,
            commands::pattern_clip_paste,
            commands::count_in_set,
            commands::open_last_at_launch_set,
            commands::chain_bypass,
            commands::open_link,
            commands::loop_set,
            commands::assistant_configure,
            commands::assistant_set_key,
            commands::assistant_models,
            commands::assistant_test,
            commands::assistant_thread,
            commands::assistant_send,
            commands::assistant_retry,
            commands::assistant_stop,
            commands::assistant_confirm,
            commands::assistant_clear,
            commands::assistant_open_url,
            commands::assistant_undo,
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
