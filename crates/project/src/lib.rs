//! The Cantina project: the model the UI edits, the commands that edit
//! it, and the directory it is saved as.
//!
//! This crate knows nothing about audio devices or the engine. The shell
//! turns a [`Project`] into a graph and applies [`Command`]s as the UI
//! sends them; the engine and the UI both read the same model.

mod chords;
mod command;
mod course;
mod drum_part;
mod exercise;
mod file;
mod fill;
mod groove_file;
mod history;
mod jam;
mod journal;
mod model;
mod pattern_file;
mod practice_log;
mod rig;
#[cfg(any(test, feature = "schema"))]
pub mod schema;
mod tab;
pub mod tab_export;

pub use chords::{parse_chord, parse_progression, Bar, Chord, Preset, PRESETS};
pub use command::{Command, CommandError};
pub use course::{
    course_file_name, course_from_json, Course, CourseStep, COURSE_SUFFIX, COURSE_VERSION,
};
pub use drum_part::{
    kind_of, place_part, unique_name, DrumPart, OrderStep, PartSection, PlaceMode, PlacedPart,
    SectionFill, Span, Vary, MAX_REPEAT, MAX_VARIATIONS,
};
pub use exercise::{
    exercise_file_name, exercise_from_json, exercise_to_json, ChordShape, Exercise, ExerciseDrums,
    Phrase, Sounded, Strum, Teaches, TempoLadder, EXERCISE_SUFFIX, EXERCISE_VERSION, FRETBOARD,
    MAX_COUNT_IN, STRUM_STEPS, STRUM_TICKS,
};
pub use file::{
    audio_dir, create_layout, dir_for, list, load, load_str, save, LoadError, SaveError, AUDIO_DIR,
    EXTENSION, PEDALS_DIR, PLUGINS_DIR, PROJECT_FILE,
};
pub use fill::generate_fill;
pub use groove_file::{
    groove_file_from_json, groove_file_to_json, GrooveFile, GROOVE_SUFFIX, GROOVE_VERSION,
};
pub use history::{History, UNDO_LIMIT};
pub use jam::{
    jam_file_name, jam_from_json, jam_to_json, pitch_class, section_name, Backing, Jam, JamDrums,
    Key, Mode, Rhythm, JAM_SUFFIX, JAM_TEMPO, JAM_VERSION,
};
pub use journal::{replay, Entry as JournalEntry, Journal, Read as JournalRead, JOURNAL_FILE};
pub use model::{
    Clip, ClipId, Fill, Humanize, Id, LoopRegion, PadMix, Pattern, PatternClip, PatternClipId,
    PatternId, PatternRow, Pedal, PedalId, Project, StepMark, Strip, Tempo, Track, TrackId,
    DRUM_GAIN_DB, FORMAT_VERSION, MAX_PATTERN_STEPS, PPQ,
};
pub use pattern_file::{
    groove_from_json, imported, pattern_file_name, pattern_from_json, pattern_to_json, Feel,
    Groove, Meter, Section, TempoRange, PATTERN_SUFFIX,
};
pub use practice_log::{
    day_number, practice_log_from_json, practice_log_to_json, PracticeDay, PracticeLog,
    PRACTICE_LOG_FILE, PRACTICE_LOG_VERSION,
};
pub use rig::{
    decode_base64, encode_base64, resolve_values, rig_file_name, rig_from_json, rig_to_json,
    ParamSlot, ParamValue, Rig, RigError, RigPedal, MAX_INPUT_GAIN_DB, RIG_SUFFIX, RIG_VERSION,
};
pub use tab::{
    tab_from_json, tab_to_json, Bend, HeardNote, Instrument, Tab, TabNote, MAX_BEND, MAX_FRET,
    TAB_SUFFIX,
};
