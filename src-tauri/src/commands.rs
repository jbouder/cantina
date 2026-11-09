//! Tauri commands: the UI's view of the engine and the project. Each one
//! locks the engine for one call and returns the status the UI should
//! show.

use cantina_hosting::scan::PluginList;
use cantina_project::Tempo;
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::State;

use crate::assistant::chat;
use crate::assistant::provider::{self, Problem};
use crate::engine::{DesignerResult, DeviceEntry, Engine, EngineState, EngineStatus};
use crate::settings::{AudioSettings, MetronomeSettings};

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

/// Opens the binary frame stream (see `frame.rs` and `docs/src/content/docs/architecture/frames.md`).
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
pub fn clip_duplicate(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().clip_duplicate(id).map_err(fail)
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

/// Takes a pedal out of the signal path, or puts it back.
#[tauri::command]
pub fn pedal_set_bypass(
    engine: State<'_, EngineState>,
    id: u64,
    on: bool,
) -> Result<EngineStatus, String> {
    engine.lock().pedal_set_bypass(id, on).map_err(fail)
}

/// Plays the input through one audio track's chain, disarming the rest.
#[tauri::command]
pub fn chain_select(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().chain_select(id).map_err(fail)
}

/// Points the stage's analysis at a track, or the master with none.
#[tauri::command]
pub fn analysis_set_source(
    engine: State<'_, EngineState>,
    track: Option<u64>,
) -> Result<EngineStatus, String> {
    engine.lock().analysis_set_source(track).map_err(fail)
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
pub fn pattern_clip_duplicate(
    engine: State<'_, EngineState>,
    id: u64,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_clip_duplicate(id).map_err(fail)
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

/// Loops a stock groove's file on a drum track to hear it before adding
/// it; both `null` stops.
#[tauri::command]
pub fn groove_audition(
    engine: State<'_, EngineState>,
    track: Option<u64>,
    path: Option<String>,
) -> Result<EngineStatus, String> {
    engine.lock().groove_audition(track, path).map_err(fail)
}

/// Hears a rig on an audio track's live input in place of its chain,
/// without changing the project; both `null` stops.
#[tauri::command]
pub fn rig_audition(
    engine: State<'_, EngineState>,
    track: Option<u64>,
    path: Option<String>,
) -> Result<EngineStatus, String> {
    engine.lock().rig_audition(track, path).map_err(fail)
}

/// While auditioning: hear the rig (`true`) or the track's own chain.
#[tauri::command]
pub fn rig_ab(engine: State<'_, EngineState>, hear_rig: bool) -> Result<EngineStatus, String> {
    engine.lock().rig_ab(hear_rig).map_err(fail)
}

/// Replaces a track's chain with a rig's, as one undoable edit.
#[tauri::command]
pub fn rig_apply(
    engine: State<'_, EngineState>,
    track: u64,
    path: String,
) -> Result<EngineStatus, String> {
    engine.lock().rig_apply(track, &path).map_err(fail)
}

/// Sets what the player plays through, which rigs trim their input from.
#[tauri::command]
pub fn instrument_profile_set(
    engine: State<'_, EngineState>,
    profile: crate::settings::InstrumentProfile,
) -> Result<EngineStatus, String> {
    engine.lock().instrument_profile_set(profile).map_err(fail)
}

/// Writes a track's chain to the Rigs library folder as a rig.
#[tauri::command]
pub fn rig_save(
    engine: State<'_, EngineState>,
    track: u64,
    rig: crate::engine::RigDraft,
) -> Result<EngineStatus, String> {
    engine.lock().rig_save(track, rig).map_err(fail)
}

/// Renames one of the user's rigs.
#[tauri::command]
pub fn rig_rename(
    engine: State<'_, EngineState>,
    path: String,
    name: String,
) -> Result<EngineStatus, String> {
    engine.lock().rig_rename(&path, &name).map_err(fail)
}

/// Deletes one of the user's rigs.
#[tauri::command]
pub fn rig_delete(engine: State<'_, EngineState>, path: String) -> Result<EngineStatus, String> {
    engine.lock().rig_delete(&path).map_err(fail)
}

/// Copies a rig file into the Rigs library folder after checking it.
#[tauri::command]
pub fn rig_import(engine: State<'_, EngineState>, path: String) -> Result<EngineStatus, String> {
    engine.lock().rig_import(&path).map_err(fail)
}

/// Writes a copy of a rig file to `to`, for sharing.
#[tauri::command]
pub fn rig_export(engine: State<'_, EngineState>, path: String, to: String) -> Result<(), String> {
    engine.lock().rig_export(&path, &to).map_err(fail)
}

/// Replaces a pattern whole, as one undoable edit.
#[tauri::command]
pub fn pattern_replace(
    engine: State<'_, EngineState>,
    pattern: cantina_project::Pattern,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_replace(pattern).map_err(fail)
}

/// Mutes and solos rows of a drum track's patterns, for listening.
#[tauri::command]
pub fn drum_rows_set(
    engine: State<'_, EngineState>,
    track: u64,
    rows: crate::engine::RowMix,
) -> Result<EngineStatus, String> {
    engine.lock().drum_rows_set(track, rows).map_err(fail)
}

/// Plays a pad of a drum track now. Not an edit.
#[tauri::command]
pub fn pad_hit(
    engine: State<'_, EngineState>,
    track: u64,
    pad: String,
    velocity: f32,
) -> Result<(), String> {
    engine.lock().pad_hit(track, &pad, velocity).map_err(fail)
}

/// Sets a pad's mix on a drum track; `null` puts it back to the kit's.
#[tauri::command]
pub fn pad_mix_set(
    engine: State<'_, EngineState>,
    track: u64,
    pad: String,
    mix: Option<cantina_project::PadMix>,
) -> Result<EngineStatus, String> {
    engine.lock().pad_mix_set(track, pad, mix).map_err(fail)
}

/// Sets, or with `null` removes, a pattern clip's fill.
#[tauri::command]
pub fn pattern_clip_set_fill(
    engine: State<'_, EngineState>,
    id: u64,
    fill: Option<cantina_project::Fill>,
) -> Result<EngineStatus, String> {
    engine.lock().pattern_clip_set_fill(id, fill).map_err(fail)
}

/// Writes a fill for a pattern clip from its pattern and has it play it.
#[tauri::command]
pub fn pattern_clip_generate_fill(
    engine: State<'_, EngineState>,
    id: u64,
    seed: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_clip_generate_fill(id, seed)
        .map_err(fail)
}

/// Sets the compensation above which the UI warns, in milliseconds.
#[tauri::command]
pub fn latency_warning_set(engine: State<'_, EngineState>, ms: f32) -> EngineStatus {
    engine.lock().latency_warning_set(ms)
}

/// Sets the metronome's switch, sound and level.
#[tauri::command]
pub fn metronome_set(
    engine: State<'_, EngineState>,
    metronome: MetronomeSettings,
) -> Result<EngineStatus, String> {
    engine.lock().metronome_set(metronome).map_err(fail)
}

// ----- the designer -----

/// Starts a new design at the end of a track's chain.
#[tauri::command]
pub fn designer_new(engine: State<'_, EngineState>, track: u64) -> Result<EngineStatus, String> {
    engine.lock().designer_new(track).map_err(fail)
}

/// Replaces a designed pedal's definition and values; problems come
/// back instead of a change when the definition has any.
#[tauri::command]
pub fn designer_update(
    engine: State<'_, EngineState>,
    id: u64,
    definition: serde_json::Value,
    values: Vec<f32>,
) -> Result<DesignerResult, String> {
    engine
        .lock()
        .designer_update(id, definition, values)
        .map_err(fail)
}

/// The definition behind a designed pedal.
#[tauri::command]
pub fn pedal_definition(
    engine: State<'_, EngineState>,
    id: u64,
) -> Result<serde_json::Value, String> {
    engine.lock().pedal_definition(id).map_err(fail)
}

/// A stock pedal's definition, to start a design from.
#[tauri::command]
pub fn pedal_stock_definition(
    engine: State<'_, EngineState>,
    name: String,
) -> Result<serde_json::Value, String> {
    engine.lock().pedal_stock_definition(&name).map_err(fail)
}

/// Every problem with a definition; empty when it would compile.
#[tauri::command]
pub fn pedal_check(definition: serde_json::Value) -> Vec<cantina_dsp::Problem> {
    Engine::pedal_check(definition)
}

/// Saves a designed pedal to the library under an id made from its
/// name, and the project's own copy.
#[tauri::command]
pub fn designer_save(engine: State<'_, EngineState>, id: u64) -> Result<EngineStatus, String> {
    engine.lock().designer_save(id).map_err(fail)
}

/// Writes a designed pedal's definition to a file of the user's choosing.
#[tauri::command]
pub fn pedal_export(engine: State<'_, EngineState>, id: u64, path: String) -> Result<(), String> {
    engine.lock().pedal_export(id, &path).map_err(fail)
}

/// Copies a pedal file into the library and, with a track, adds it to
/// that track's chain.
#[tauri::command]
pub fn pedal_import(
    engine: State<'_, EngineState>,
    path: String,
    track: Option<u64>,
) -> Result<EngineStatus, String> {
    engine.lock().pedal_import(&path, track).map_err(fail)
}

/// Looks for pedals in the library again.
/// Adds a `*.pattern.json` to the project, checked against a track's kit.
#[tauri::command]
pub fn pattern_import(
    engine: State<'_, EngineState>,
    path: String,
    track: Option<u64>,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_import(&path, track)
        .map_err(|e| format!("{e:#}"))
}

/// Writes a clip's tab as MusicXML, ASCII tab or MIDI; returns the path.
#[tauri::command]
pub fn clip_tab_export(
    engine: State<'_, EngineState>,
    id: u64,
    format: String,
    path: String,
) -> Result<String, String> {
    engine
        .lock()
        .clip_tab_export(id, &format, &path)
        .map_err(|e| format!("{e:#}"))
}

/// Writes a tab's PDF, drawn by the UI from the page view; returns the path.
#[tauri::command]
pub fn tab_pdf_write(
    engine: State<'_, EngineState>,
    path: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    engine
        .lock()
        .tab_pdf_write(&path, &bytes)
        .map_err(|e| format!("{e:#}"))
}

/// Writes a pattern to a `*.pattern.json`.
#[tauri::command]
pub fn pattern_export(engine: State<'_, EngineState>, id: u64, path: String) -> Result<(), String> {
    engine
        .lock()
        .pattern_export(id, &path)
        .map_err(|e| format!("{e:#}"))
}

/// Scans the library folders again.
#[tauri::command]
pub fn library_rescan(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().library_rescan()
}

#[tauri::command]
pub fn pedals_rescan(engine: State<'_, EngineState>) -> EngineStatus {
    engine.lock().pedals_rescan()
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

/// Renders the mix to a WAV file, faster than real time, off the IPC
/// thread. Progress arrives on the `render-progress` event.
#[tauri::command]
pub async fn mixdown_export(
    app: tauri::AppHandle,
    options: crate::engine::ExportOptions,
) -> Result<crate::engine::ExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<EngineState>()
            .mixdown_export(options)
            .map_err(fail)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Renders a track through its chain to one new clip in its place, as
/// one undoable edit. Progress arrives on the `render-progress` event.
#[tauri::command]
pub async fn track_bounce(app: tauri::AppHandle, id: u64) -> Result<EngineStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<EngineState>().track_bounce(id).map_err(fail)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Transcribes a clip into a tab, off the IPC thread, as one undoable
/// edit. Progress arrives on the `render-progress` event as
/// `transcribe`; `render_cancel` stops it and leaves nothing behind.
#[tauri::command]
pub async fn clip_transcribe(
    app: tauri::AppHandle,
    id: u64,
    settings: crate::tabs::TabSettings,
) -> Result<EngineStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<EngineState>()
            .clip_transcribe(id, settings)
            .map_err(fail)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A clip's whole tab, for the tab view; `None` when it has none.
#[tauri::command]
pub fn clip_tab(
    state: tauri::State<'_, EngineState>,
    id: u64,
) -> Result<Option<cantina_project::Tab>, String> {
    state.clip_tab(id).map_err(fail)
}

/// Re-quantises a transcribed tab to another grid from what it heard:
/// the notes in the selection's span, or all of them.
#[tauri::command]
pub fn clip_tab_requantise(
    state: tauri::State<'_, EngineState>,
    id: u64,
    grid: crate::tabs::Grid,
    selection: Option<Vec<u32>>,
) -> Result<EngineStatus, String> {
    state
        .clip_tab_requantise(id, grid, &selection.unwrap_or_default())
        .map_err(fail)
}

/// Fits the selected notes of a tab (or all) to the fretboard again.
#[tauri::command]
pub fn clip_tab_refit(
    state: tauri::State<'_, EngineState>,
    id: u64,
    selection: Option<Vec<u32>>,
) -> Result<EngineStatus, String> {
    state
        .clip_tab_refit(id, &selection.unwrap_or_default())
        .map_err(fail)
}

/// Edits a tab's notes: removes those at `remove`, adds `add`.
#[tauri::command]
pub fn clip_tab_edit(
    state: tauri::State<'_, EngineState>,
    id: u64,
    remove: Vec<u32>,
    add: Vec<cantina_project::TabNote>,
) -> Result<EngineStatus, String> {
    state.clip_tab_edit(id, remove, add).map_err(fail)
}

/// Sets how to practise: hear the tabs, mute the recordings under them,
/// and the practice speed.
#[tauri::command]
pub fn practice_set(
    state: tauri::State<'_, EngineState>,
    practice: crate::engine::Practice,
) -> Result<EngineStatus, String> {
    state.lock().practice_set(practice).map_err(fail)
}

/// Stops the running export, bounce or transcription.
#[tauri::command]
pub fn render_cancel() {
    crate::engine::cancel_render();
}

/// The UI's view of which menu items apply: enabled, checked and label,
/// one entry per action. See `menu.rs`.
#[tauri::command]
pub fn menu_state(app: tauri::AppHandle, items: Vec<crate::menu::ItemState>) -> Result<(), String> {
    crate::menu::apply(&app, &items).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn project_rename(
    engine: State<'_, EngineState>,
    name: String,
) -> Result<EngineStatus, String> {
    engine.lock().project_rename(&name).map_err(fail)
}

#[tauri::command]
pub fn project_duplicate(
    engine: State<'_, EngineState>,
    name: String,
) -> Result<EngineStatus, String> {
    engine.lock().project_duplicate(&name).map_err(fail)
}

/// Shows the open project's folder (`project`) or the library folder
/// (`library`) in Finder.
#[tauri::command]
pub fn reveal(engine: State<'_, EngineState>, what: String) -> Result<(), String> {
    let (path, select) = engine.lock().reveal_path(&what).map_err(fail)?;
    let mut open = std::process::Command::new("open");
    if select {
        open.arg("-R");
    }
    open.arg(&path).status().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn audio_import(
    engine: State<'_, EngineState>,
    paths: Vec<String>,
    track: u64,
    start: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .audio_import(&paths, track, start)
        .map_err(fail)
}

#[tauri::command]
pub fn kit_import(engine: State<'_, EngineState>, dir: String) -> Result<EngineStatus, String> {
    engine.lock().kit_import(&dir).map_err(fail)
}

#[tauri::command]
pub fn clip_paste(
    engine: State<'_, EngineState>,
    track: u64,
    start: u64,
    name: String,
    file: String,
    offset: u64,
    length: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .clip_paste(track, start, &name, &file, offset, length)
        .map_err(fail)
}

#[tauri::command]
pub fn pattern_clip_paste(
    engine: State<'_, EngineState>,
    track: u64,
    start: u64,
    pattern: u64,
    offset: u64,
    length: u64,
) -> Result<EngineStatus, String> {
    engine
        .lock()
        .pattern_clip_paste(track, start, pattern, offset, length)
        .map_err(fail)
}

#[tauri::command]
pub fn count_in_set(engine: State<'_, EngineState>, on: bool) -> EngineStatus {
    engine.lock().count_in_set(on)
}

#[tauri::command]
pub fn chain_bypass(
    engine: State<'_, EngineState>,
    track: u64,
    on: bool,
) -> Result<EngineStatus, String> {
    engine.lock().chain_bypass(track, on).map_err(fail)
}

/// Opens one of the Help menu's links in the browser: `docs`,
/// `shortcuts`, `releases` or `issue`. Only those; see `menu::link_url`.
#[tauri::command]
pub fn open_link(app: tauri::AppHandle, which: String) -> Result<(), String> {
    let version = app.package_info().version.to_string();
    let url =
        crate::menu::link_url(&which, &version).ok_or_else(|| format!("no link called {which}"))?;
    std::process::Command::new("open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Sets the loop region and whether playback loops, or clears it with none.
#[tauri::command]
pub fn loop_set(
    engine: State<'_, EngineState>,
    region: Option<cantina_project::LoopRegion>,
) -> Result<EngineStatus, String> {
    engine.lock().loop_set(region).map_err(fail)
}

// ----- the assistant -----

/// Turns the assistant on or off and sets its endpoint and model.
#[tauri::command]
pub fn assistant_configure(
    engine: State<'_, EngineState>,
    settings: crate::settings::AssistantSettings,
) -> EngineStatus {
    engine.lock().assistant_configure(settings)
}

/// Saves the API key in the Keychain, or removes it when empty. The key
/// is never sent back to the UI.
#[tauri::command]
pub fn assistant_set_key(
    engine: State<'_, EngineState>,
    key: String,
) -> Result<EngineStatus, String> {
    engine.lock().assistant_set_key(&key).map_err(fail)
}

/// The connection from the settings, read under a short lock so no
/// request holds the engine.
fn assistant_connection(
    app: &tauri::AppHandle,
) -> Result<crate::assistant::provider::Connection, Problem> {
    use tauri::Manager;
    let settings = app.state::<EngineState>().lock().assistant_settings();
    crate::assistant::connection(&settings)
}

/// The models the endpoint offers. Makes no request while the assistant is off.
#[tauri::command]
pub async fn assistant_models(app: tauri::AppHandle) -> Result<Vec<String>, Problem> {
    let connection = assistant_connection(&app)?;
    connection.models(&provider::client()).await
}

/// Checks the endpoint, the key and the model, and says what is wrong in
/// plain words. Makes no request while the assistant is off.
#[tauri::command]
pub async fn assistant_test(app: tauri::AppHandle) -> Result<(), Problem> {
    let connection = assistant_connection(&app)?;
    connection.test(&provider::client()).await
}

/// The chat thread so far, kept between launches.
#[tauri::command]
pub fn assistant_thread(session: State<'_, crate::assistant::Session>) -> Vec<chat::Item> {
    session.items()
}

/// Adds the player's message and answers it, streaming events over the
/// channel until `Done`. Makes no request while the assistant is off.
#[tauri::command]
pub async fn assistant_send(
    app: tauri::AppHandle,
    text: String,
    channel: Channel<chat::Event>,
) -> Result<(), Problem> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let connection = assistant_connection(&app)?;
    answer(app, connection, Some(text), channel).await
}

/// Asks again after a failure, or for a new last reply.
#[tauri::command]
pub async fn assistant_retry(
    app: tauri::AppHandle,
    channel: Channel<chat::Event>,
) -> Result<(), Problem> {
    let connection = assistant_connection(&app)?;
    answer(app, connection, None, channel).await
}

/// Runs one conversation turn: `text` is the player's new message, or
/// none to retry the last.
async fn answer(
    app: tauri::AppHandle,
    connection: provider::Connection,
    text: Option<String>,
    channel: Channel<chat::Event>,
) -> Result<(), Problem> {
    use tauri::Manager;
    let session = app.state::<crate::assistant::Session>();
    let cancel = session.begin()?;
    // Ends the turn however this returns, a dropped future included.
    struct End<'a>(&'a crate::assistant::Session);
    impl Drop for End<'_> {
        fn drop(&mut self) {
            self.0.end();
        }
    }
    let _end = End(&session);
    let ready = {
        let mut thread = session.thread.lock().unwrap_or_else(|e| e.into_inner());
        match text {
            Some(text) => {
                let item = chat::Item::User { text };
                thread.push(item.clone());
                let _ = channel.send(chat::Event::Item { item });
                true
            }
            None => thread.rewind_for_retry(),
        }
    };
    if ready {
        let model = chat::Remote {
            connection,
            client: provider::chat_client(),
        };
        let persona = {
            use tauri::Manager;
            app.state::<EngineState>()
                .lock()
                .assistant_settings()
                .persona
        };
        let mut tools = crate::assistant::HostTools::new(app.clone()).with_persona(persona);
        let sink = |event: chat::Event| {
            let _ = channel.send(event);
        };
        let ask = |id: &str| session.ask(id);
        chat::converse(&model, &mut tools, &session.thread, &sink, &cancel, &ask).await;
    } else {
        let _ = channel.send(chat::Event::Done);
    }
    Ok(())
}

/// Stops the answer being written.
#[tauri::command]
pub fn assistant_stop(session: State<'_, crate::assistant::Session>) {
    session.stop();
}

/// The player's answer to a tool that asked first.
#[tauri::command]
pub fn assistant_confirm(session: State<'_, crate::assistant::Session>, id: String, yes: bool) {
    session.answer(&id, yes);
}

/// Forgets the thread. Not while an answer is being written.
#[tauri::command]
pub fn assistant_clear(session: State<'_, crate::assistant::Session>) -> Result<(), String> {
    if session.busy() {
        return Err("Stop the answer first.".into());
    }
    session
        .thread
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    Ok(())
}

/// Opens a web link from a reply in the browser: http and https only.
#[tauri::command]
pub fn assistant_open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("not a web link: {url}"));
    }
    std::process::Command::new("open")
        .arg(&url)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Undoes the edit behind a tool card, if nothing has changed since, and
/// marks the card undone.
#[tauri::command]
pub fn assistant_undo(
    engine: State<'_, EngineState>,
    session: State<'_, crate::assistant::Session>,
    id: String,
) -> Result<EngineStatus, String> {
    let mut thread = session.thread.lock().unwrap_or_else(|e| e.into_inner());
    let revision = thread.items.iter().find_map(|item| match item {
        chat::Item::Call {
            id: call,
            undo: Some(revision),
            undone: false,
            ..
        } if *call == id => Some(*revision),
        _ => None,
    });
    let revision = revision.ok_or("That change cannot be undone from here.")?;
    let status = engine.lock().assistant_undo(revision).map_err(fail)?;
    for item in &mut thread.items {
        if let chat::Item::Call {
            id: call, undone, ..
        } = item
        {
            if *call == id {
                *undone = true;
            }
        }
    }
    thread.save();
    Ok(status)
}
