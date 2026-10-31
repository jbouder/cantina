//! Tauri commands: the UI's view of the engine. Each one locks the engine
//! for one call and returns the status the UI should show.

use tauri::ipc::Channel;
use tauri::State;

use crate::engine::{DeviceEntry, EngineState, EngineStatus, MeterFrame};
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

#[tauri::command]
pub fn subscribe_meters(engine: State<'_, EngineState>, channel: Channel<MeterFrame>) {
    engine.lock().subscribe_meters(channel)
}
