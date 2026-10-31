//! Tauri commands: the UI's view of the engine. Each one locks the engine
//! for one call and returns the status the UI should show.

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use crate::engine::{DeviceEntry, EngineState, EngineStatus};
use crate::settings::AudioSettings;

#[tauri::command]
pub fn engine_status(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().status()
}

#[tauri::command]
pub fn engine_start(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().start()
}

#[tauri::command]
pub fn engine_stop(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().stop()
}

#[tauri::command]
pub fn dismiss_notice(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().dismiss_notice()
}

#[tauri::command]
pub fn list_devices(engine: State<'_, EngineState>) -> Result<Vec<DeviceEntry>, String> {
    engine.lock().list_devices().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn audio_settings(engine: State<'_, EngineState>) -> AudioSettings {
    engine.lock().audio_settings()
}

#[tauri::command]
pub fn set_audio_settings(engine: State<'_, EngineState>, settings: AudioSettings) -> EngineStatus {
    engine.lock().set_audio_settings(settings)
}

/// Opens the binary frame stream (see `frame.rs` and `docs/frames.md`).
#[tauri::command]
pub fn subscribe_meters(engine: State<'_, EngineState>, channel: Channel<InvokeResponseBody>) {
    engine.lock().subscribe_meters(channel)
}

/// The UI took the last frame; called from its animation frame.
#[tauri::command]
pub fn meter_ack(engine: State<'_, EngineState>) {
    engine.lock().meter_ack()
}

/// Frames skipped so far because the UI had not taken the previous one.
#[tauri::command]
pub fn frames_dropped(engine: State<'_, EngineState>) -> u32 {
    engine.lock().frames_dropped()
}

fn fail(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
pub fn track_arm(engine: State<'_, EngineState>, armed: bool) -> EngineStatus {
    engine.lock().track_arm(armed)
}

#[tauri::command]
pub fn track_record(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().track_record().map_err(fail)
}

#[tauri::command]
pub fn track_play(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().track_play().map_err(fail)
}

#[tauri::command]
pub fn track_stop(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().track_stop().map_err(fail)
}

#[tauri::command]
pub fn chain_add(
    engine: State<'_, EngineState>,
    spec: String,
    index: Option<usize>,
) -> Result<EngineStatus, String> {
    engine.lock().chain_add(&spec, index).map_err(fail)
}

#[tauri::command]
pub fn chain_remove(engine: State<'_, EngineState>, id: u32) -> Result<EngineStatus, String> {
    engine.lock().chain_remove(id).map_err(fail)
}

#[tauri::command]
pub fn chain_move(
    engine: State<'_, EngineState>,
    id: u32,
    to: usize,
) -> Result<EngineStatus, String> {
    engine.lock().chain_move(id, to).map_err(fail)
}

#[tauri::command]
pub fn chain_set_param(
    engine: State<'_, EngineState>,
    id: u32,
    param: usize,
    value: f32,
) -> Result<(), String> {
    engine
        .lock()
        .chain_set_param(id, param, value)
        .map_err(fail)
}

/// Prints a line from the UI on the shell's stderr, so errors in the web
/// view show up in the terminal that launched the app.
#[tauri::command]
pub fn ui_log(level: String, message: String) {
    eprintln!("[ui {level}] {message}");
}
