//! Tauri commands: the UI's view of the engine. Each one locks the engine
//! for one call and returns the status the UI should show.

use tauri::State;

use crate::engine::{EngineState, EngineStatus};

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
