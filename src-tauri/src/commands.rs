//! Tauri commands: the UI's view of the engine and the project. Each one
//! locks the engine for one call and returns the status the UI should
//! show.

use cantina_hosting::scan::PluginList;
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

// ----- plugins -----

/// What the scanner found. Starts a scan if none has run yet.
#[tauri::command]
pub fn plugin_list(engine: State<'_, EngineState>) -> PluginList {
    engine.lock().plugin_list()
}

/// Scans new and changed plugins in the background, or everything when
/// `full`. The status is pushed when it finishes.
#[tauri::command]
pub fn plugin_rescan(engine: State<'_, EngineState>, full: bool) -> EngineStatus {
    engine.lock().plugin_rescan(full)
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
pub fn project_recover(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().project_recover().map_err(fail)
}

#[tauri::command]
pub fn project_discard_recovery(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().project_discard_recovery().map_err(fail)
}

#[tauri::command]
pub fn project_undo(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().undo().map_err(fail)
}

#[tauri::command]
pub fn project_redo(engine: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine.lock().redo().map_err(fail)
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

// ----- drums -----

/// Adds a drum track playing `kit`, or the first installed kit.
#[tauri::command]
pub fn track_add_drums(
    engine: State<'_, EngineState>,
    kit: Option<String>,
) -> Result<EngineStatus, String> {
    engine.lock().track_add_drums(kit).map_err(fail)
}

#[tauri::command]
pub fn track_set_kit(
    engine: State<'_, EngineState>,
    id: u64,
    kit: String,
) -> Result<EngineStatus, String> {
    engine.lock().track_set_kit(id, kit).map_err(fail)
}

/// Looks for installed kits again.
#[tauri::command]
pub fn kits_rescan(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().kits_rescan()
}

/// Swaps what a pedal is: a cabinet's impulse response.
#[tauri::command]
pub fn chain_set_spec(
    engine: State<'_, EngineState>,
    id: u64,
    spec: String,
) -> Result<EngineStatus, String> {
    engine.lock().chain_set_spec(id, &spec).map_err(fail)
}

/// Looks for installed impulse responses again.
#[tauri::command]
pub fn irs_rescan(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().irs_rescan()
}

#[tauri::command]
pub fn pattern_add(
    engine: State<'_, EngineState>,
    name: Option<String>,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_add(name).map_err(fail)
}

#[tauri::command]
pub fn pattern_remove(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().pattern_remove(id).map_err(fail)
}

#[tauri::command]
pub fn pattern_rename(
    engine: State<'_, EngineState>,
    id: u64,
    name: String,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_rename(id, name).map_err(fail)
}

/// Sets one cell: a velocity from 0 (clear) to 1.
#[tauri::command]
pub fn pattern_set_step(
    engine: State<'_, EngineState>,
    id: u64,
    pad: String,
    step: usize,
    velocity: f32,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_set_step(id, pad, step, velocity)
        .map_err(fail)
}

#[tauri::command]
pub fn pattern_set_length(
    engine: State<'_, EngineState>,
    id: u64,
    length: u16,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_set_length(id, length).map_err(fail)
}

/// Steps per beat: 1, 2, 3, 4, 6 or 8.
#[tauri::command]
pub fn pattern_set_resolution(
    engine: State<'_, EngineState>,
    id: u64,
    steps_per_beat: u8,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_set_resolution(id, steps_per_beat)
        .map_err(fail)
}

/// Swing from 0 to 1. Not echoed.
#[tauri::command]
pub fn pattern_set_swing(
    engine: State<'_, EngineState>,
    id: u64,
    swing: f32,
) -> Result<(), String> {
    engine.lock().pattern_set_swing(id, swing).map_err(fail)
}

/// Puts a pattern on a drum track at `start` ticks, for `length` ticks
/// or one pass of the pattern.
#[tauri::command]
pub fn pattern_clip_add(
    engine: State<'_, EngineState>,
    track: u64,
    pattern: u64,
    start: u64,
    length: Option<u64>,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_clip_add(track, pattern, start, length)
        .map_err(fail)
}

#[tauri::command]
pub fn pattern_clip_remove(
    engine: State<'_, EngineState>,
    id: u64,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_clip_remove(id).map_err(fail)
}

#[tauri::command]
pub fn pattern_clip_move(
    engine: State<'_, EngineState>,
    id: u64,
    track: u64,
    start: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_clip_move(id, track, start)
        .map_err(fail)
}

#[tauri::command]
pub fn pattern_clip_trim(
    engine: State<'_, EngineState>,
    id: u64,
    start: u64,
    offset: u64,
    length: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_clip_trim(id, start, offset, length)
        .map_err(fail)
}

#[tauri::command]
pub fn pattern_clip_split(
    engine: State<'_, EngineState>,
    id: u64,
    at: u64,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_clip_split(id, at).map_err(fail)
}

/// Loops a pattern on a drum track while the transport plays; both
/// `null` stops.
#[tauri::command]
pub fn pattern_audition(
    engine: State<'_, EngineState>,
    track: Option<u64>,
    pattern: Option<u64>,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_audition(track, pattern).map_err(fail)
}

/// Sets the compensation above which the UI warns, in milliseconds.
#[tauri::command]
pub fn latency_warning_set(engine: State<'_, EngineState>, ms: f32) -> EngineStatus {
    engine.lock().latency_warning_set(ms)
}

// ----- editor windows -----

/// Opens a plugin's native editor window, or brings it to the front.
#[tauri::command]
pub fn editor_open(engine: State<'_, EngineState>, id: u64) -> EngineStatus {
    engine.lock().editor_open(id)
}

#[tauri::command]
pub fn editor_close(engine: State<'_, EngineState>, id: u64) -> EngineStatus {
    engine.lock().editor_close(id)
}

/// Prints a line from the UI on the shell's stderr, so errors in the web
/// view show up in the terminal that launched the app.
#[tauri::command]
pub fn ui_log(level: String, message: String) {
    eprintln!("[ui {level}] {message}");
}
