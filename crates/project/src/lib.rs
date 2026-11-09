//! The Cantina project: the model the UI edits, the commands that edit
//! it, and the directory it is saved as.
//!
//! This crate knows nothing about audio devices or the engine. The shell
//! turns a [`Project`] into a graph and applies [`Command`]s as the UI
//! sends them; the engine and the UI both read the same model.

mod command;
mod file;
mod history;
mod journal;
mod model;
mod pattern_file;
#[cfg(any(test, feature = "schema"))]
pub mod schema;
mod tab;
pub mod tab_export;

pub use command::{Command, CommandError};
pub use file::{
    audio_dir, create_layout, dir_for, list, load, load_str, save, LoadError, SaveError, AUDIO_DIR,
    EXTENSION, PEDALS_DIR, PLUGINS_DIR, PROJECT_FILE,
};
pub use history::{History, UNDO_LIMIT};
pub use journal::{replay, Entry as JournalEntry, Journal, Read as JournalRead, JOURNAL_FILE};
pub use model::{
    Clip, ClipId, Id, LoopRegion, Pattern, PatternClip, PatternClipId, PatternId, PatternRow,
    Pedal, PedalId, Project, Strip, Tempo, Track, TrackId, DRUM_GAIN_DB, FORMAT_VERSION,
    MAX_PATTERN_STEPS, PPQ,
};
pub use pattern_file::{
    groove_from_json, imported, pattern_file_name, pattern_from_json, pattern_to_json, Feel,
    Groove, Meter, Section, TempoRange, PATTERN_SUFFIX,
};
pub use tab::{
    tab_from_json, tab_to_json, Bend, HeardNote, Instrument, Tab, TabNote, MAX_BEND, MAX_FRET,
    TAB_SUFFIX,
};
