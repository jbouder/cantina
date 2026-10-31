//! Tauri commands: the UI's view of the engine and the project. Each one
//! locks the engine for one call and returns the status the UI should
//! show.

use cantina_project::Tempo;
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::State;

use crate::engine::{DeviceEntry, EngineState, EngineStatus};
use crate::settings::AudioSettings;

fn fail(e: anyhow::Error) -> String {
    format!("{e:#}")
}

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
    engine.lock().list_devices().map_err(fail)
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

// ----- the project -----

#[tauri::command]
pub fn project_list(engine: State<'_, EngineState>) -> Result<Vec<String>, String> {
    engine.lock().project_list().map_err(fail)
}

#[tauri::command]
pub fn project_new(engine: State<'_, EngineState>, name: String) -> Result<EngineStatus, String> {
    engine.lock().project_new(&name).map_err(fail)
}

#[tauri::command]
pub fn project_open(engine: State<'_, EngineState>, name: String) -> Result<EngineStatus, String> {
    engine.lock().project_open(&name).map_err(fail)
}

#[tauri::command]
pub fn project_save(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().project_save().map_err(fail)
}

#[tauri::command]
pub fn tempo_set(engine: State<'_, EngineState>, tempo: Tempo) -> Result<EngineStatus, String> {
    engine.lock().tempo_set(tempo).map_err(fail)
}

// ----- the mixer -----

/// Gain on a track's strip, or the master's with no track. Not echoed.
#[tauri::command]
pub fn strip_gain(
    engine: State<'_, EngineState>,
    track: Option<u64>,
    db: f32,
) -> Result<(), String> {
    engine.lock().strip_gain(track, db).map_err(fail)
}

#[tauri::command]
pub fn strip_pan(engine: State<'_, EngineState>, track: u64, pan: f32) -> Result<(), String> {
    engine.lock().strip_pan(track, pan).map_err(fail)
}

#[tauri::command]
pub fn strip_mute(
    engine: State<'_, EngineState>,
    track: Option<u64>,
    on: bool,
) -> Result<EngineStatus, String> {
    engine.lock().strip_mute(track, on).map_err(fail)
}

#[tauri::command]
pub fn strip_solo(
    engine: State<'_, EngineState>,
    track: u64,
    on: bool,
) -> Result<EngineStatus, String> {
    engine.lock().strip_solo(track, on).map_err(fail)
}

// ----- tracks and clips -----

#[tauri::command]
pub fn track_add(
    engine: State<'_, EngineState>,
    name: Option<String>,
) -> Result<EngineStatus, String> {
    engine.lock().track_add(name).map_err(fail)
}

#[tauri::command]
pub fn track_remove(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().track_remove(id).map_err(fail)
}

#[tauri::command]
pub fn track_rename(
    engine: State<'_, EngineState>,
    id: u64,
    name: String,
) -> Result<EngineStatus, String> {
    engine.lock().track_rename(id, name).map_err(fail)
}

#[tauri::command]
pub fn track_move(
    engine: State<'_, EngineState>,
    id: u64,
    to: usize,
) -> Result<EngineStatus, String> {
    engine.lock().track_move(id, to).map_err(fail)
}

#[tauri::command]
pub fn track_arm(
    engine: State<'_, EngineState>,
    id: u64,
    armed: bool,
) -> Result<EngineStatus, String> {
    engine.lock().track_arm(id, armed).map_err(fail)
}

#[tauri::command]
pub fn clip_remove(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().clip_remove(id).map_err(fail)
}

#[tauri::command]
pub fn clip_move(
    engine: State<'_, EngineState>,
    id: u64,
    track: u64,
    start: u64,
) -> Result<EngineStatus, String> {
    engine.lock().clip_move(id, track, start).map_err(fail)
}

#[tauri::command]
pub fn clip_trim(
    engine: State<'_, EngineState>,
    id: u64,
    start: u64,
    offset: u64,
    length: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .clip_trim(id, start, offset, length)
        .map_err(fail)
}

#[tauri::command]
pub fn clip_split(
    engine: State<'_, EngineState>,
    id: u64,
    at: u64,
) -> Result<EngineStatus, String> {
    engine.lock().clip_split(id, at).map_err(fail)
}

/// A file in `audio/` as little-endian mono f32 samples, for the UI's
/// peak cache. Binary, not JSON: a take can be tens of megabytes.
#[tauri::command]
pub fn audio_samples(engine: State<'_, EngineState>, file: String) -> Result<Response, String> {
    engine
        .lock()
        .audio_samples(&file)
        .map(Response::new)
        .map_err(fail)
}

// ----- the transport -----

#[tauri::command]
pub fn transport_play(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().transport_play().map_err(fail)
}

#[tauri::command]
pub fn transport_record(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().transport_record().map_err(fail)
}

#[tauri::command]
pub fn transport_stop(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().transport_stop().map_err(fail)
}

#[tauri::command]
pub fn transport_locate(
    engine: State<'_, EngineState>,
    sample: u64,
) -> Result<EngineStatus, String> {
    engine.lock().transport_locate(sample).map_err(fail)
}

// ----- the chain -----

#[tauri::command]
pub fn chain_add(
    engine: State<'_, EngineState>,
    track: u64,
    spec: String,
    index: Option<usize>,
) -> Result<EngineStatus, String> {
    engine.lock().chain_add(track, &spec, index).map_err(fail)
}

#[tauri::command]
pub fn chain_remove(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().chain_remove(id).map_err(fail)
}

#[tauri::command]
pub fn chain_move(
    engine: State<'_, EngineState>,
    id: u64,
    to: usize,
) -> Result<EngineStatus, String> {
    engine.lock().chain_move(id, to).map_err(fail)
}

#[tauri::command]
pub fn chain_set_param(
    engine: State<'_, EngineState>,
    id: u64,
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
