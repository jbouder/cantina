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

pub use command::{Command, CommandError};
pub use file::{
    audio_dir, create_layout, dir_for, list, load, load_str, save, LoadError, SaveError, AUDIO_DIR,
    EXTENSION, PEDALS_DIR, PLUGINS_DIR, PROJECT_FILE,
};
pub use history::{History, UNDO_LIMIT};
pub use journal::{replay, Entry as JournalEntry, Journal, Read as JournalRead, JOURNAL_FILE};
pub use model::{
    Clip, ClipId, Id, Pedal, PedalId, Project, Strip, Tempo, Track, TrackId, FORMAT_VERSION,
};
