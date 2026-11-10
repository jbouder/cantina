//! The shell's handle on the audio engine and the open project.
//!
//! One [`Engine`] lives in Tauri managed state as [`EngineState`]. It owns
//! the project, the stream, the graph controller and the meters, and
//! everything the UI can do goes through methods here, called from
//! `commands.rs` while the mutex is held.
//!
//! The project is the source of truth. Every edit is a
//! [`Command`] applied to it, after which the live graph is brought in
//! line: parameter changes go over the audio thread's queue, anything
//! structural rebuilds the wiring and commits a new schedule.
//!
//! Two helper threads exist while a stream is open: one receives
//! hardware change events from the engine's [`Watcher`] and asks the
//! engine to rebuild when its devices go away or change rate, and one
//! pumps meter levels to the UI at display rate.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{
    Cabinet, Handle, LoadedKit, ParamInfo, PedalDef, PedalError, Problem, Processor, Readout, Unit,
    STOCK_NAMES,
};
use cantina_engine::click::{LEVEL as CLICK_LEVEL, ON as CLICK_ON, SOUND as CLICK_SOUND};
use cantina_engine::strip::{GAIN, MUTE, PAN};
use cantina_engine::{
    beat_at, device, ir, kit, plan, processors, wav, Analyzer, Click, Controller, Delay,
    DeviceEvent, DeviceId, Endpoint, GraphConfig, Impulse, IrEntry, KitEntry, LoopRange, Meter,
    MeterHandle, MusicalPosition, Mute, NodeId, PatternData, Placement, Player, PlayerHandle,
    Recorder, RecorderHandle, Runtime, Scope, ScopeHandle, Score, Sequencer, SequencerHandle,
    Stream, StreamConfig, Strip, TabLoop, TabScore, TabVoice, TabVoiceHandle, TempoMap,
    TimeSignature, TransportHandle, Watcher, MAX_BLOCK_FRAMES,
};
use cantina_hosting::scan::{self, PluginList, Scanner};
use cantina_hosting::{AudioUnitHandle, ClapHandle, Editor};
use cantina_project::{
    Clip, ClipId, Command, History, Journal, JournalEntry, LoopRegion, Pattern, PatternClip,
    PatternClipId, PatternId, PatternRow, Pedal, PedalId, Project, Strip as StripModel, Tempo,
    Track, TrackId, PEDALS_DIR, PLUGINS_DIR,
};
use objc2::MainThreadMarker;
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};

use crate::frame::{Analysis as FrameAnalysis, Beat as FrameBeat, Frame};
use crate::library;
use crate::mixdown;
use crate::pedals::{self, PedalEntry};
use crate::project::{NewProject, Projects};
use crate::settings::{AssistantSettings, AudioSettings, MetronomeSettings, Settings};
use crate::tabs;

#[path = "engine_jam.rs"]
mod jam_engine;
use jam_engine::JamSession;
pub use jam_engine::{JamStatus, JamTrackInfo, KeepInto};

/// Event name for status pushes to the UI.
pub const STATUS_EVENT: &str = "engine://status";

/// Where the engine is, as the UI sees it. Mirrors `EngineStatus` in
/// `ui/src/lib/engine.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub sample_rate: f64,
    pub buffer_frames: u32,
    pub input_uid: Option<String>,
    pub output_uid: Option<String>,
    pub input_name: Option<String>,
    pub output_name: Option<String>,
    /// Why the last start, stop or rebuild failed, if it did.
    pub error: Option<String>,
    /// What the engine did on its own, such as falling back to another
    /// device. `notice_seq` increments with each new one.
    pub notice: Option<String>,
    pub notice_seq: u32,
    pub transport: TransportStatus,
    pub project: ProjectStatus,
    /// The stock pedals `chain_add` accepts, with how each is made.
    pub pedal_kinds: Vec<PedalKindInfo>,
    pub plugins: PluginsStatus,
    pub latency: LatencyStatus,
    /// The drum kits installed, for a drum track to pick from.
    pub kits: Vec<KitInfo>,
    /// The cabinet impulse responses installed, for a cabinet pedal.
    pub irs: Vec<IrInfo>,
    /// The pattern looping on a drum track for editing, if any. Not
    /// part of the project.
    pub audition: Option<AuditionStatus>,
    /// The track the stage's analysis listens to, or none for the master.
    pub analysis_source: Option<u64>,
    /// The library folders: where they are, the pattern files in
    /// `Patterns`, the user's rigs in `Rigs`, and every file in them that
    /// does not load.
    pub library: library::LibraryStatus,
    pub metronome: MetronomeStatus,
    /// Record counts in four beats first. An app setting.
    pub count_in: bool,
    /// Open the last project at launch rather than show the start
    /// dialog. An app setting.
    pub open_last_at_launch: bool,
    /// The assistant's connection, without its key. An app setting.
    pub assistant: crate::assistant::AssistantStatus,
    /// Hearing tabs, muting what was recorded under them, and the
    /// practice speed. Session state, not part of the project.
    pub practice: Practice,
    /// The stock grooves that ship with the app, read-only; adding one
    /// imports a copy into the project.
    pub grooves: Vec<library::StockGroove>,
    /// Every rig the picker offers: the stock ones, then the user's.
    pub rigs: Vec<crate::rigs::RigInfo>,
    /// Every jam the Jam view offers: the stock ones, then the user's.
    pub jams: Vec<crate::jams::JamInfo>,
    /// The rig being heard on a track in place of its chain, if one is.
    /// Session state, not part of the project.
    pub rig_audition: Option<RigAuditionStatus>,
    /// What the player plays through, and the trim it gives a rig's
    /// input. An app setting.
    pub instrument_profile: InstrumentProfileStatus,
    /// Rows muted or soloed on drum tracks, by track id. Session state.
    pub drum_rows: HashMap<u64, RowMix>,
    /// The jam, while jam mode is open. Session state, not part of the
    /// project.
    pub jam: Option<JamStatus>,
}

/// A rig heard on a track's live input in place of its chain. Mirrors
/// `RigAudition` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RigAuditionStatus {
    pub track: u64,
    /// The rig's file.
    pub path: String,
    /// Hearing the rig (A) rather than the track's own chain (B).
    pub hearing_rig: bool,
    /// The rig's pedals that are not installed and are left out.
    pub missing: Vec<String>,
}

/// The instrument profile and the trim it gives rigs. Mirrors
/// `InstrumentProfile` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentProfileStatus {
    #[serde(flatten)]
    pub profile: crate::settings::InstrumentProfile,
    /// Decibels added to every rig's input gain when it is applied.
    pub trim_db: f32,
}

/// What a saved rig says about itself. Mirrors `RigDraft` in the UI.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigDraft {
    pub name: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub description: String,
    /// `guitar` or `bass`.
    pub instrument: cantina_project::Instrument,
    /// Overwrite a rig of the same name.
    #[serde(default)]
    pub replace: bool,
}

/// A rig being auditioned: its pedals, built as a chain's with ids no
/// project pedal has.
struct RigAudition {
    track: TrackId,
    path: String,
    chain: Vec<Pedal>,
    hearing_rig: bool,
    missing: Vec<String>,
}

/// The first id an auditioned rig's pedals take: far above any a project
/// gives out, so they never meet a project pedal's.
const AUDITION_IDS: u64 = 1 << 62;

/// How long a track's strip takes to fade out before its chain is
/// swapped, and how long to leave for the swap to reach the audio
/// thread before it fades back in: five of the strip's 4 ms time
/// constants, and a block at the largest buffer with room to spare.
const SWAP_FADE: Duration = Duration::from_millis(25);
const SWAP_SETTLE: Duration = Duration::from_millis(30);

/// Practice with a tab: hear it played, mute the recording it came from
/// and slow everything that can follow down. Mirrors `Practice` in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Practice {
    /// Play every track's tabs through the plucked-string voice, into
    /// the track's pedals.
    pub voice: bool,
    /// Silence the recordings on tracks that have a tab, to play the
    /// part yourself. Drums and other tracks play on.
    pub mute_recordings: bool,
    /// The practice speed, 0.5 to 1. Below 1 the click, the drums and
    /// the tab voice slow down; recordings cannot follow and fall silent,
    /// and nothing can be recorded.
    pub speed: f64,
}

impl Default for Practice {
    fn default() -> Self {
        Self {
            voice: false,
            mute_recordings: false,
            speed: 1.0,
        }
    }
}

impl Practice {
    /// Whether `track`'s recordings are muted for practice.
    fn mutes(&self, track: &Track) -> bool {
        self.mute_recordings && track.clips.iter().any(|c| c.tab.is_some())
    }

    /// What the voice plays on `track`: its tabs when the voice is on.
    fn score(
        &self,
        track: &Track,
        project: &Project,
        sample_rate: f64,
    ) -> cantina_engine::TabScore {
        if self.voice {
            crate::tabs::timeline_notes(track, &tempo_map(&project.tempo), sample_rate)
        } else {
            Vec::new()
        }
    }
}

/// The metronome's settings and the sounds it offers. Mirrors
/// `MetronomeStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetronomeStatus {
    #[serde(flatten)]
    pub settings: MetronomeSettings,
    /// Every sound `metronome_set` takes, by name.
    pub sounds: Vec<String>,
}

/// What the designer gets back from an update: the status, and the
/// problems that stopped the definition from being used, if any.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignerResult {
    pub status: EngineStatus,
    pub problems: Vec<Problem>,
}

/// A kind of pedal the picker offers. Mirrors `PedalKindInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalKindInfo {
    /// What `chain_add` takes.
    pub spec: String,
    pub name: String,
    /// Ships with Cantina.
    pub stock: bool,
    /// Defined in the pedal format, so the designer can open it as a
    /// starting point.
    pub designable: bool,
}

/// The stock kinds, described once: building each to ask its name.
fn stock_kinds() -> &'static [PedalKindInfo] {
    static KINDS: OnceLock<Vec<PedalKindInfo>> = OnceLock::new();
    KINDS.get_or_init(|| {
        STOCK_NAMES
            .iter()
            .map(|spec| PedalKindInfo {
                spec: spec.to_string(),
                name: cantina_dsp::stock(spec)
                    .map_or_else(|| spec.to_string(), |p| p.name().to_string()),
                stock: true,
                designable: cantina_dsp::stock_definition(spec).is_some(),
            })
            .collect()
    })
}

/// An installed kit. Mirrors `KitInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitInfo {
    pub id: String,
    pub name: String,
}

/// An installed cabinet impulse response. Mirrors `IrInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrInfo {
    pub id: String,
    pub name: String,
}

/// What is looping on a drum track: a project pattern, or a stock groove
/// by its file. Mirrors `AuditionStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditionStatus {
    pub track: u64,
    /// The project pattern looping, if it is one.
    pub pattern: Option<u64>,
    /// The stock groove's file looping, if it is one.
    pub groove: Option<String>,
}

/// What loops on a drum track while it is auditioned: a project pattern
/// from the start, for editing it by ear, or a stock groove from the bar
/// it was chosen on, for choosing one. Session state, never saved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Audition {
    /// A pattern of the project, looping from tick 0.
    Pattern(PatternId),
    /// A stock groove read from its file, looping from tick `from`.
    Groove {
        path: String,
        pattern: Arc<Pattern>,
        from: u64,
    },
}

/// The drum track auditioning, and what.
pub(crate) type Auditioning = Option<(TrackId, Audition)>;

/// Rows of a drum track's patterns muted or soloed for listening while
/// editing. Session state: never saved, and a mixdown plays every row.
/// Mirrors `DrumRows` in the UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowMix {
    /// Pads not heard.
    pub muted: Vec<String>,
    /// With any, only these pads are heard.
    pub soloed: Vec<String>,
}

impl RowMix {
    /// Whether a pad's row is heard.
    fn plays(&self, pad: &str) -> bool {
        !self.muted.iter().any(|p| p == pad)
            && (self.soloed.is_empty() || self.soloed.iter().any(|p| p == pad))
    }
}

/// Muted and soloed rows by drum track.
pub(crate) type RowMixes = HashMap<TrackId, RowMix>;

/// Where the time goes. Mirrors `LatencyStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStatus {
    /// The largest chain latency, which every other track is delayed to
    /// match: how late playback is against the timeline.
    pub compensation_frames: usize,
    /// What CoreAudio reports for the open devices, each way.
    pub input_frames: u32,
    pub output_frames: u32,
    /// Compensation above this many milliseconds is flagged.
    pub warn_ms: f32,
    pub warn: bool,
}

/// What the plugin scanner has found, in brief; `plugin_list` has the
/// list itself. Mirrors `PluginsStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsStatus {
    /// True while a scan runs in the background.
    pub scanning: bool,
    pub count: usize,
    pub failed: usize,
    /// Seconds since the epoch of the last scan, zero before the first.
    pub scanned_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportStatus {
    pub playing: bool,
    /// True while any track is recording.
    pub recording: bool,
}

/// The open project, as the UI sees it. Mirrors `ProjectStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    pub name: String,
    pub dir: String,
    /// True when there are edits since the last save.
    pub dirty: bool,
    /// What Undo would revert, if anything.
    pub undo_label: Option<String>,
    /// What Redo would reapply, if anything.
    pub redo_label: Option<String>,
    /// Edits found in the journal from a session that did not save,
    /// waiting for the user to recover or discard them.
    pub recovery: Option<RecoveryInfo>,
    pub sample_rate: f64,
    pub tempo: Tempo,
    pub tracks: Vec<TrackStatus>,
    /// Drum patterns, shared by every drum track.
    pub patterns: Vec<PatternInfo>,
    /// The master strip; pan and solo are unused on it.
    pub master: StripModel,
    /// The loop region, and whether playback loops over it.
    #[serde(rename = "loop")]
    pub loop_region: Option<LoopRegion>,
}

/// A pattern, as the grid shows it. Mirrors `PatternInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternInfo {
    pub id: u64,
    pub name: String,
    pub steps_per_beat: u8,
    pub length: u16,
    pub swing: f32,
    pub rows: Vec<PatternRow>,
    /// The pattern's humanize, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub humanize: Option<cantina_project::Humanize>,
    /// Seeds humanize and chance; 0 is left out.
    #[serde(skip_serializing_if = "is_zero_seed")]
    pub seed: u32,
}

fn is_zero_seed(v: &u32) -> bool {
    *v == 0
}

impl PatternInfo {
    /// The status of a pattern: every field, since the grid sends it back
    /// whole to `pattern_replace`.
    fn of(q: &Pattern) -> Self {
        Self {
            id: q.id.0,
            name: q.name.clone(),
            steps_per_beat: q.steps_per_beat,
            length: q.length,
            swing: q.swing,
            rows: q.rows.clone(),
            humanize: q.humanize,
            seed: q.seed,
        }
    }
}

/// A project in the start dialog's recent list. Mirrors `RecentProject`
/// in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub name: String,
    /// When its `project.json` was last written, in milliseconds since
    /// the epoch.
    pub saved: Option<u64>,
    /// Moved or deleted since it was open.
    pub missing: bool,
}

/// What a journal left behind. Mirrors `RecoveryInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    pub edits: usize,
    /// The last edit's label.
    pub last: String,
}

/// A track, as the UI sees it. Mirrors `TrackStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackStatus {
    pub id: u64,
    pub name: String,
    /// Armed tracks monitor their input and record when the transport
    /// records.
    pub armed: bool,
    pub recording: bool,
    /// The kit of a drum track; an audio track has none.
    pub kit: Option<KitStatus>,
    /// The pedal chain in order.
    pub chain: Vec<PedalInfo>,
    pub clips: Vec<ClipInfo>,
    /// Patterns on the timeline, for a drum track. Positions in ticks.
    pub pattern_clips: Vec<PatternClipInfo>,
    pub strip: StripModel,
    /// Frames the chain delays the signal by.
    pub latency_frames: usize,
    /// A drum track's pad mixer, by pad id; pads not here are as the kit has them.
    pub pads: std::collections::BTreeMap<String, cantina_project::PadMix>,
}

/// A drum track's kit, with the pads the grid shows rows for. Mirrors
/// `KitStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitStatus {
    pub id: String,
    pub name: String,
    pub pads: Vec<PadInfo>,
    /// True when no installed kit has this id; the track plays silence.
    pub missing: bool,
}

/// A pad of a kit. Mirrors `PadInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadInfo {
    pub id: String,
    pub name: String,
}

/// A clip, as the UI sees it. Positions are in samples at the project's
/// rate. Mirrors `ClipInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipInfo {
    pub id: u64,
    pub name: String,
    pub file: String,
    pub start: u64,
    pub offset: u64,
    pub length: u64,
    /// The clip's tab, in brief, when it has one.
    pub tab: Option<TabSummary>,
}

/// A clip's tab as the clip list shows it. Mirrors `TabSummary` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabSummary {
    /// `guitar` or `bass`.
    pub instrument: cantina_project::Instrument,
    /// Open strings, lowest first, as MIDI notes.
    pub tuning: Vec<u8>,
    /// The capo's fret.
    pub capo: u8,
    /// Notes in the tab.
    pub notes: usize,
    /// True when it came from a transcription and can be re-quantized.
    pub transcribed: bool,
}

/// A pattern clip, as the UI sees it. Positions are in ticks. Mirrors
/// `PatternClipInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternClipInfo {
    pub id: u64,
    pub pattern: u64,
    pub start: u64,
    pub offset: u64,
    pub length: u64,
    /// The clip's fill, if it plays one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<cantina_project::Fill>,
}

impl PatternClipInfo {
    /// The status of a pattern clip: every field the UI shows or sends back.
    fn of(c: &cantina_project::PatternClip) -> Self {
        Self {
            id: c.id.0,
            pattern: c.pattern.0,
            start: c.start,
            offset: c.offset,
            length: c.length,
            fill: c.fill.clone(),
        }
    }
}

/// A pedal in a chain. Mirrors `PedalInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalInfo {
    /// Stable for the pedal's life, across stream rebuilds.
    pub id: u64,
    pub spec: String,
    pub name: String,
    pub params: Vec<ParamOut>,
    /// Whether the plugin can open its own editor window.
    pub has_editor: bool,
    pub editor_open: bool,
    /// Frames of delay this pedal adds.
    pub latency_frames: usize,
    /// Names of the values the pedal measures for its display, in the
    /// order they appear in the frame's readout section.
    pub readouts: Vec<String>,
    /// Out of the signal path, settings kept.
    pub bypassed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamOut {
    pub name: String,
    /// A unit suffix for display: "dB", "Hz", "s", "ms", "%" or "".
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub value: f32,
    /// A switch: the value is 0 or 1.
    pub toggle: bool,
    /// For a pick from a list, the names; the value is an index.
    pub choices: Vec<String>,
}

impl ParamOut {
    fn new(p: &ParamInfo, value: f32) -> Self {
        Self {
            name: p.name.to_string(),
            unit: unit_label(p.unit),
            min: p.min,
            max: p.max,
            default: p.default,
            value,
            toggle: p.unit == Unit::Toggle,
            choices: match p.unit {
                Unit::Choice(names) => names.iter().map(|n| n.to_string()).collect(),
                _ => Vec::new(),
            },
        }
    }
}

fn unit_label(unit: Unit) -> &'static str {
    match unit {
        Unit::Generic | Unit::Toggle | Unit::Choice(_) => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Milliseconds => "ms",
        Unit::Percent => "%",
    }
}

/// A device as the picker shows it. Mirrors `AudioDevice` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub uid: String,
    pub name: String,
    pub input_channels: u32,
    pub output_channels: u32,
    pub sample_rates: Vec<f64>,
    pub buffer_min: u32,
    pub buffer_max: u32,
    pub default_input: bool,
    pub default_output: bool,
}

/// A UI listener for binary frames (see `frame.rs`). At most one frame is
/// in flight per subscriber: the pump skips frames until the UI has taken
/// the last one, so a slow or hidden UI gets fresh frames, never a queue.
struct Subscriber {
    channel: Channel<InvokeResponseBody>,
    in_flight: Arc<AtomicBool>,
}

/// Frames per second the meter pump sends.
const METER_RATE: u32 = 60;

/// The pump polls plugin handles for changes every this many frames.
const PLUGIN_POLL_FRAMES: u32 = 6;

/// The graph runs in stereo so strips can pan; the mono input is copied
/// to both channels on the way in.
const CHANNELS: usize = 2;

/// The most latency compensation a track's delay can hold.
const MAX_COMPENSATION_SECONDS: usize = 2;

/// Compensation above this is flagged in the UI unless the settings say
/// otherwise.
pub const DEFAULT_LATENCY_WARN_MS: f32 = 20.0;

/// The managed-state wrapper. `Stream` and `Controller` are `Send` but not
/// `Sync`, so the mutex is what lets Tauri share them between commands.
pub struct EngineState(Mutex<Engine>);

impl EngineState {
    pub fn new(app: AppHandle) -> Self {
        Self(Mutex::new(Engine::new(app)))
    }

    /// Locks the engine. A poisoned lock is recovered rather than
    /// propagated: a panic in one command leaves nothing half-done.
    pub fn lock(&self) -> MutexGuard<'_, Engine> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Renders the mix to a WAV file. The graph is built under the lock;
    /// the render runs without it, so the app keeps playing and editing
    /// meanwhile, and what is exported is the project as it was when the
    /// export began.
    pub fn mixdown_export(&self, options: ExportOptions) -> Result<ExportResult> {
        let format = wav::SampleFormatOut::from_bits(options.bits)?;
        if !matches!(options.sample_rate, 44_100 | 48_000) {
            bail!("export at 44.1 or 48 kHz");
        }
        let _busy = Busy::take()?;
        let (mut render, range, app) = {
            let mut engine = self.lock();
            let range = engine.export_range(options.from_bar, options.to_bar)?;
            let render = engine.mixdown_render(mixdown::Scope::Mix)?;
            (render, range, engine.app.clone())
        };
        let began = Instant::now();
        let rendered = render.run(range, options.tail, progress_to(&app, "export"))?;
        let from = render.sample_rate();
        drop(render);
        let to = options.sample_rate as f64;
        let channels = mixdown::convert_rate(rendered, from, to);
        let (peak_db, rms_db) = mixdown::levels(&channels);
        let mut path = PathBuf::from(&options.path);
        if path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("wav"))
        {
            path.set_extension("wav");
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        wav::write_as(&path, to, &channels, format)?;
        let frames = channels.first().map_or(0, Vec::len);
        Ok(ExportResult {
            path: path.display().to_string(),
            seconds: frames as f64 / to,
            render_seconds: began.elapsed().as_secs_f64(),
            peak_db,
            rms_db,
        })
    }

    /// Renders one track through its chain to a new file and puts it in
    /// the track's place as a single clip, as one undoable edit that
    /// takes the chain off (the undo brings it back). The render runs
    /// without the lock; if the track changed meanwhile, the bounce is
    /// dropped and its file removed.
    pub fn track_bounce(&self, id: u64) -> Result<EngineStatus> {
        let _busy = Busy::take()?;
        let id = TrackId(id);
        let (mut render, range, before, file, clip_name, audio_dir, app) = {
            let mut engine = self.lock();
            if !engine.recording.is_empty() {
                bail!("stop recording before bouncing");
            }
            let track = engine
                .project
                .track(id)
                .cloned()
                .with_context(|| format!("no track {id}"))?;
            let range = mixdown::track_range(&engine.project, &track)
                .with_context(|| format!("{} has nothing on it to bounce", track.name))?;
            let render = engine.mixdown_render(mixdown::Scope::Track(id))?;
            let audio_dir = cantina_project::audio_dir(&engine.project_dir);
            let (file, clip_name) = take_name(&audio_dir, &format!("{} bounce", track.name));
            (
                render,
                range,
                track,
                file,
                clip_name,
                audio_dir,
                engine.app.clone(),
            )
        };
        let rendered = render.run(range, true, progress_to(&app, "bounce"))?;
        let rate = render.sample_rate();
        drop(render);
        std::fs::create_dir_all(&audio_dir)
            .with_context(|| format!("creating {}", audio_dir.display()))?;
        let path = audio_dir.join(&file);
        wav::write(&path, rate, &rendered)?;
        let length = rendered.first().map_or(0, Vec::len) as u64;

        let mut engine = self.lock();
        if engine.project.track(id) != Some(&before) {
            let _ = std::fs::remove_file(&path);
            bail!(
                "{} changed while it was bouncing; bounce it again",
                before.name
            );
        }
        let clip = Clip {
            id: ClipId(engine.project.fresh_id()),
            name: clip_name,
            file,
            start: range.start,
            offset: 0,
            length,
            tab: None,
        };
        let track = Track {
            kit: None,
            chain: Vec::new(),
            clips: vec![clip],
            pattern_clips: Vec::new(),
            pads: Default::default(),
            ..before
        };
        engine.apply(Command::BounceTrack { track })
    }
}

impl EngineState {
    /// Transcribes a clip into a tab, off the IPC thread, as one undoable
    /// edit. Reads the clip's audio (never writing it), runs the model
    /// with progress on the `render-progress` event and stops on
    /// [`cancel_render`], leaving the project as it was.
    pub fn clip_transcribe(&self, id: u64, settings: tabs::TabSettings) -> Result<EngineStatus> {
        let _busy = Busy::take()?;
        let id = ClipId(id);
        let (clip, path, tempo, rate, app) = {
            let engine = self.lock();
            let (_, clip) = engine
                .project
                .clip(id)
                .with_context(|| format!("no clip {id}"))?;
            let path = cantina_project::audio_dir(&engine.project_dir).join(&clip.file);
            (
                clip.clone(),
                path,
                engine.project.tempo.clone(),
                engine.project.sample_rate,
                engine.app.clone(),
            )
        };
        let audio = tabs::clip_audio(&path, &clip)?;
        let model = cantina_engine::transcribe::Transcriber::new()?;
        let Some(maps) = model.run(&audio, progress_to(&app, "transcribe"))? else {
            bail!("canceled");
        };
        let heard = cantina_engine::transcribe::notes(
            &maps,
            cantina_engine::transcribe::Options::default(),
        );
        let origin = tabs::origin(&clip, &tempo, rate);
        let lead = clip.offset.saturating_sub(origin) as f64 / rate;
        let tab = tabs::transcribed(&clip.name, &settings, &tempo, origin, lead, &heard)?;

        let mut engine = self.lock();
        if engine.project.clip(id).is_none() {
            bail!("{} was deleted while it was being transcribed", clip.name);
        }
        engine.apply(Command::SetClipTab {
            id,
            tab: Some(Box::new(tab)),
        })
    }

    /// A clip's whole tab; `None` when it has none.
    pub fn clip_tab(&self, id: u64) -> Result<Option<cantina_project::Tab>> {
        let engine = self.lock();
        let (_, clip) = engine
            .project
            .clip(ClipId(id))
            .with_context(|| format!("no clip {id}"))?;
        Ok(clip.tab.clone())
    }

    /// Builds a transcribed tab's notes again on another grid, from what
    /// it heard, as one undoable edit. Nothing is transcribed again. With
    /// a selection, only the notes in its span change.
    pub fn clip_tab_requantize(
        &self,
        id: u64,
        grid: tabs::Grid,
        selection: &[u32],
    ) -> Result<EngineStatus> {
        self.edit_clip_tab(id, |clip, tab| {
            if tab.heard.is_empty() {
                bail!(
                    "{}'s tab was not transcribed, so there is nothing to re-quantize",
                    clip.name
                );
            }
            Ok(tabs::requantize(tab, grid, selection))
        })
    }

    /// Fits the selected notes of a clip's tab (all of them with none
    /// selected) to the fretboard again, as one undoable edit.
    pub fn clip_tab_refit(&self, id: u64, selection: &[u32]) -> Result<EngineStatus> {
        self.edit_clip_tab(id, |_, tab| Ok(tabs::refit(tab, selection)))
    }

    /// Takes the notes at `remove` out of a clip's tab and adds `add`, as
    /// one undoable edit: how the tab editor changes anything.
    pub fn clip_tab_edit(
        &self,
        id: u64,
        remove: Vec<u32>,
        add: Vec<cantina_project::TabNote>,
    ) -> Result<EngineStatus> {
        self.edit_clip_tab(id, |_, _| Ok((remove, add)))
    }

    /// Applies the edit `make` builds from a clip and its tab; a change
    /// of nothing is not an edit and leaves the history alone.
    fn edit_clip_tab(
        &self,
        id: u64,
        make: impl FnOnce(&Clip, &cantina_project::Tab) -> Result<tabs::NotesEdit>,
    ) -> Result<EngineStatus> {
        let id = ClipId(id);
        let mut engine = self.lock();
        let (_, clip) = engine
            .project
            .clip(id)
            .with_context(|| format!("no clip {id}"))?;
        let tab = clip
            .tab
            .as_ref()
            .with_context(|| format!("{} has no tab", clip.name))?;
        let (remove, add) = make(clip, tab)?;
        if remove.is_empty() && add.is_empty() {
            return Ok(engine.status());
        }
        engine.apply(Command::EditTabNotes { id, remove, add })
    }
}

/// What the export sheet asks for.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    /// Where to write; `.wav` is added when missing.
    pub path: String,
    /// 44100 or 48000.
    pub sample_rate: u32,
    /// 16, 24 or 32 (float).
    pub bits: u16,
    /// The first bar, counting from 1, or the start of the song.
    pub from_bar: Option<u32>,
    /// The last bar, included, or the end of the song.
    pub to_bar: Option<u32>,
    /// Render past the end until reverbs and delays have rung out.
    pub tail: bool,
}

/// What an export wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// The file.
    pub path: String,
    /// Its length.
    pub seconds: f64,
    /// How long the render took: less than `seconds` is faster than
    /// real time.
    pub render_seconds: f64,
    /// Loudest sample, dBFS.
    pub peak_db: f32,
    /// Level over the whole file, dBFS.
    pub rms_db: f32,
}

/// The event a render reports its progress on.
pub const RENDER_PROGRESS_EVENT: &str = "render-progress";

/// Progress of an export or a bounce.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderProgress {
    /// `export` or `bounce`.
    what: &'static str,
    /// 0 to 1.
    fraction: f32,
}

/// Set by [`cancel_render`]; the running render stops at its next look.
static CANCEL: AtomicBool = AtomicBool::new(false);
/// One render at a time.
static RENDERING: AtomicBool = AtomicBool::new(false);

/// Asks the running export or bounce to stop.
pub fn cancel_render() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Held while a render runs.
struct Busy;

impl Busy {
    fn take() -> Result<Self> {
        if RENDERING.swap(true, Ordering::AcqRel) {
            bail!("an export, bounce or transcription is already running");
        }
        CANCEL.store(false, Ordering::Relaxed);
        Ok(Busy)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        RENDERING.store(false, Ordering::Release);
    }
}

/// Sends a render's progress to the UI, and says whether to go on.
fn progress_to(app: &AppHandle, what: &'static str) -> impl FnMut(f32) -> bool {
    let app = app.clone();
    move |fraction| {
        let _ = app.emit(RENDER_PROGRESS_EVENT, RenderProgress { what, fraction });
        !CANCEL.load(Ordering::Relaxed)
    }
}

/// What a pedal spec builds: learned by building one once, then cached,
/// so the status can describe a chain without touching the graph.
#[derive(Debug, Clone)]
pub struct PedalKind {
    pub name: String,
    pub params: Vec<ParamInfo>,
    pub readouts: Vec<String>,
}

impl PedalKind {
    fn info(&self, pedal: &Pedal) -> PedalInfo {
        PedalInfo {
            id: pedal.id.0,
            spec: pedal.spec.clone(),
            name: self.name.clone(),
            params: self
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| ParamOut::new(p, pedal.values.get(i).copied().unwrap_or(p.default)))
                .collect(),
            has_editor: false,
            editor_open: false,
            latency_frames: 0,
            readouts: self.readouts.clone(),
            bypassed: pedal.bypassed,
        }
    }
}

/// Holds what only the main thread may touch (AppKit windows) inside the
/// engine, which lives in a mutex shared by every thread. Reaching the
/// contents takes proof of being on the main thread.
struct MainThreadCell<T>(T);

// The contents are only ever accessed through `get_mut`, which demands a
// `MainThreadMarker`, and dropped with the engine at exit on the main
// thread.
unsafe impl<T> Send for MainThreadCell<T> {}

impl<T> MainThreadCell<T> {
    fn get_mut(&mut self, _mtm: MainThreadMarker) -> &mut T {
        &mut self.0
    }
}

/// The nodes one track owns while the stream is open. An audio track:
///
/// ```text
/// input -> mute -> bus -> pedals... -> strip -> (master)
///            |      ^                    |
///       recorder  delay <- player      meter
/// ```
///
/// A drum track has no input: the sequencer takes the player's place.
///
/// ```text
/// sequencer -> delay -> bus -> pedals... -> strip -> (master)
/// ```
struct LiveTrack {
    id: TrackId,
    source: Source,
    /// Where live input and playback meet before the pedals.
    bus: NodeId,
    pedals: Vec<LivePedal>,
    /// Latency compensation on the playback branch only, so live input
    /// is never delayed to match another track.
    delay: NodeId,
    /// Gain, pan and mute after the pedals.
    strip: NodeId,
    /// Post-fader, a tap off the strip.
    meter: NodeId,
    levels: MeterHandle,
}

/// What feeds a track: its input and clips, or a kit under a sequencer.
enum Source {
    Audio {
        /// The monitor gate: input passes when the track is armed.
        mute: NodeId,
        player_node: NodeId,
        player: PlayerHandle,
        recorder_node: NodeId,
        recorder: RecorderHandle,
        /// Plays the track's tabs, into its pedals, when practicing.
        voice_node: NodeId,
        voice: TabVoiceHandle,
    },
    Drums {
        /// The kit id the sequencer was built with; a change rebuilds it.
        kit: String,
        sequencer_node: NodeId,
        sequencer: SequencerHandle,
    },
}

impl Source {
    fn nodes(&self) -> Vec<NodeId> {
        match self {
            Source::Audio {
                mute,
                player_node,
                recorder_node,
                voice_node,
                ..
            } => vec![*mute, *player_node, *recorder_node, *voice_node],
            Source::Drums { sequencer_node, .. } => vec![*sequencer_node],
        }
    }
}

/// A pedal's node, and for a plugin the UI-thread side of it, which the
/// shell keeps while the processor runs on the audio thread.
struct LivePedal {
    id: PedalId,
    /// What the node was built from; a changed spec means a new node.
    spec: String,
    node: NodeId,
    handle: Option<Box<dyn Handle>>,
    /// Latency as last reported, in frames.
    latency: usize,
    /// The pedal's display values, shared with the audio thread.
    readout: Option<Readout>,
    /// Out of the signal path: the node stays, unwired, with its state.
    bypassed: bool,
}

impl LiveTrack {
    /// Frames the chain delays the signal by. A bypassed pedal adds none.
    fn latency(&self) -> usize {
        self.pedals
            .iter()
            .filter(|p| !p.bypassed)
            .map(|p| p.latency)
            .sum()
    }

    fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        [self.bus, self.delay, self.strip, self.meter]
            .into_iter()
            .chain(self.source.nodes())
            .chain(self.pedals.iter().map(|p| p.node))
    }

    fn recorder(&mut self) -> Option<&mut RecorderHandle> {
        match &mut self.source {
            Source::Audio { recorder, .. } => Some(recorder),
            Source::Drums { .. } => None,
        }
    }
}

/// Plugin state by pedal, with the spec it was saved from so a stale file
/// never lands in a different plugin.
type PluginStates = HashMap<PedalId, (String, Vec<u8>)>;

/// What [`Live::sync`] needs from the engine.
struct SyncContext<'a> {
    project: &'a Project,
    catalog: &'a mut Catalog,
    kits: &'a mut Kits,
    irs: &'a mut Irs,
    designs: &'a Designs,
    audition: &'a Auditioning,
    rows: &'a RowMixes,
    audio_dir: &'a Path,
    states: &'a PluginStates,
    practice: Practice,
    /// Drum scores that stand in for a track's pattern clips: a jam's.
    scores: &'a HashMap<TrackId, Score>,
    /// What a track's voice plays in place of its tabs: a jam's backing.
    voices: &'a HashMap<TrackId, (TabScore, TabLoop)>,
}

/// The meters the pump reads, in frame order: input, master, then one
/// per track in the project's order. Replaced whole on each sync.
type Meters = Arc<Mutex<Vec<MeterHandle>>>;

/// The pedal readouts the pump reads, in track and chain order, as the
/// status lists them. Replaced whole on each sync.
type Readouts = Arc<Mutex<Vec<Readout>>>;

/// What the pump analyzes for the stage: the scope's capture, the meter
/// that measures the same signal, and the tempo map for the beat.
struct PumpAnalysis {
    scope: ScopeHandle,
    /// Frame order: 1 for the master, 2 and up for a track.
    meter: AtomicU32,
    tempo: Mutex<TempoMap>,
    /// Frames from the transport's position to what is heard now, so the
    /// beat lands with the click rather than a buffer ahead of it.
    output_latency: u64,
}

/// The last minutes of a jam's input, dry and through the rig, kept in
/// rings the audio thread only writes. See [`cantina_engine::capture`].
struct LiveCapture {
    /// The track whose input and chain are captured.
    track: TrackId,
    dry_node: NodeId,
    wet_node: NodeId,
    dry: Arc<cantina_engine::CaptureRing>,
    wet: Arc<cantina_engine::CaptureRing>,
    /// How long they hold.
    minutes: f32,
}

/// What is running while the stream is open.
struct Live {
    stream: Stream,
    controller: Controller,
    /// In the project's track order.
    tracks: Vec<LiveTrack>,
    /// The master strip: where the tracks sum before the output.
    master: NodeId,
    /// The largest chain latency, which playback on every track is
    /// delayed to match.
    compensation: usize,
    input_meter: NodeId,
    output_meter: NodeId,
    input_levels: MeterHandle,
    output_levels: MeterHandle,
    meters: Meters,
    readouts: Readouts,
    /// Captures what the stage analyzes: the master, or one track.
    scope: NodeId,
    /// The metronome, straight to the output: heard, but not metered,
    /// analyzed or mixed down.
    click: NodeId,
    /// The track the scope listens to; the master when it is gone.
    analysis_source: Option<TrackId>,
    analysis: Arc<PumpAnalysis>,
    input: DeviceId,
    output: DeviceId,
    input_name: String,
    output_name: String,
    /// The capture rings on a jam's input, while a jam is open.
    capture: Option<LiveCapture>,
    /// Dropping it removes the hardware listeners and ends their thread.
    _watcher: Watcher,
    pump_stop: Arc<AtomicBool>,
}

impl Live {
    /// Hands the audio thread a new tempo map, and the pump a copy for
    /// the beat position.
    fn set_tempo_map(&mut self, map: TempoMap) {
        *self
            .analysis
            .tempo
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = map.clone();
        self.transport().set_tempo_map(map);
    }

    fn transport(&mut self) -> &mut TransportHandle {
        self.controller.transport_mut()
    }

    /// Hands the click its switch, sound and level.
    fn set_metronome(&mut self, m: &MetronomeSettings) {
        let on = if m.on { 1.0 } else { 0.0 };
        self.controller.set_param(self.click, CLICK_ON, on);
        self.controller
            .set_param(self.click, CLICK_SOUND, m.sound_index() as f32);
        self.controller
            .set_param(self.click, CLICK_LEVEL, m.level_db);
    }

    fn track(&self, id: TrackId) -> Result<&LiveTrack> {
        self.tracks
            .iter()
            .find(|t| t.id == id)
            .with_context(|| format!("track {id} is not in the graph"))
    }

    fn track_mut(&mut self, id: TrackId) -> Result<&mut LiveTrack> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == id)
            .with_context(|| format!("track {id} is not in the graph"))
    }

    fn pedal_node(&self, id: PedalId) -> Result<NodeId> {
        self.tracks
            .iter()
            .flat_map(|t| t.pedals.iter())
            .find(|p| p.id == id)
            .map(|p| p.node)
            .with_context(|| format!("pedal {id} is not in the graph"))
    }

    fn pedals_mut(&mut self) -> impl Iterator<Item = &mut LivePedal> {
        self.tracks.iter_mut().flat_map(|t| t.pedals.iter_mut())
    }

    /// Brings the graph in line with the project: tracks and pedals are
    /// created, dropped and reordered to match, each player gets its
    /// track's clips, every gate follows its track's arm state, and the
    /// result is wired and committed. Nodes that survive keep their
    /// state; a plugin created here gets its saved state from `states`
    /// before the project's parameter values go on top. Clips that
    /// cannot load are reported but do not stop the rest.
    fn sync(&mut self, cx: SyncContext<'_>, removed_pedals: &mut Vec<PedalId>) -> Result<()> {
        let SyncContext {
            project,
            catalog,
            kits,
            irs,
            designs,
            audition,
            rows,
            audio_dir,
            states,
            practice,
            scores,
            voices,
        } = cx;
        let sample_rate = self.stream.info().sample_rate;
        let mut problems = Vec::new();

        // Tracks that went away, with every node they held.
        let mut gone = Vec::new();
        self.tracks.retain(|t| {
            if project.track(t.id).is_some() {
                true
            } else {
                gone.extend(t.nodes());
                removed_pedals.extend(t.pedals.iter().map(|p| p.id));
                false
            }
        });
        for node in gone {
            self.controller.graph_mut().remove(node)?;
        }

        let mut ordered = Vec::with_capacity(project.tracks.len());
        for track in &project.tracks {
            let mut live = match self.tracks.iter().position(|t| t.id == track.id) {
                Some(i) => self.tracks.remove(i),
                None => Self::create_track(&mut self.controller, track, sample_rate, kits),
            };
            // A drum track whose kit changed gets a new sequencer; the
            // old one's voices go with it. A bounced drum track is an
            // audio track now, and undoing the bounce makes it drums
            // again: either way the source is built anew.
            let stale = match (&track.kit, &live.source) {
                (Some(wanted), Source::Drums { kit: have, .. }) => wanted != have,
                (None, Source::Audio { .. }) => false,
                _ => true,
            };
            if stale {
                for node in live.source.nodes() {
                    self.controller.graph_mut().remove(node)?;
                }
                live.source = Self::create_source(&mut self.controller, track, sample_rate, kits);
            }
            let graph = self.controller.graph_mut();
            // Pedals that left this chain.
            let mut removed = Vec::new();
            live.pedals.retain(|p| {
                if track.pedal(p.id).is_some_and(|q| q.spec == p.spec) {
                    true
                } else {
                    removed.push(p.node);
                    removed_pedals.push(p.id);
                    false
                }
            });
            for node in removed {
                graph.remove(node)?;
            }
            // Pedals in the chain's order, creating the new ones.
            let mut pedals = Vec::with_capacity(track.chain.len());
            for pedal in &track.chain {
                let live_pedal = match live.pedals.iter().position(|p| p.id == pedal.id) {
                    Some(i) => live.pedals.remove(i),
                    None => {
                        let mut processor = build_processor(irs, designs, &pedal.spec, CHANNELS)?;
                        catalog.learn(&pedal.spec, processor.as_ref());
                        if let Some((spec, state)) = states.get(&pedal.id) {
                            if *spec == pedal.spec {
                                if let Err(e) = processor.set_state(state) {
                                    problems.push(format!(
                                        "{}: saved state not restored: {e}",
                                        processor.name()
                                    ));
                                }
                            }
                        }
                        for (param, value) in pedal.values.iter().enumerate() {
                            processor.set_param(param, *value);
                        }
                        let handle = processor.handle();
                        let readout = processor.readout();
                        let node = graph.add_processor(processor);
                        let latency = graph.node(node).map_or(0, |n| n.latency);
                        LivePedal {
                            id: pedal.id,
                            spec: pedal.spec.clone(),
                            node,
                            handle,
                            latency,
                            readout,
                            bypassed: false,
                        }
                    }
                };
                let mut live_pedal = live_pedal;
                live_pedal.bypassed = pedal.bypassed;
                pedals.push(live_pedal);
            }
            live.pedals = pedals;
            match &mut live.source {
                Source::Audio {
                    mute,
                    player,
                    voice,
                    ..
                } => {
                    self.controller
                        .set_param(*mute, 0, if track.armed { 1.0 } else { 0.0 });
                    let clips = if practice.mutes(track) {
                        Vec::new()
                    } else {
                        engine_clips(track, audio_dir)
                    };
                    if player.clips() != clips.as_slice() {
                        if let Err(e) = player.set_clips(clips) {
                            problems.push(format!("{}: {e:#}", track.name));
                        }
                    }
                    let (score, looped) = match voices.get(&track.id) {
                        Some((score, looped)) => (score.clone(), looped.clone()),
                        None => (
                            practice.score(track, project, sample_rate),
                            TabLoop::default(),
                        ),
                    };
                    if (voice.score() != &score && !voice.set_score(score))
                        || (voice.looped() != &looped && !voice.set_loop(looped))
                    {
                        problems.push(format!(
                            "{}: the audio thread is not taking the tab",
                            track.name
                        ));
                    }
                }
                Source::Drums { sequencer, kit, .. } => {
                    let score = match scores.get(&track.id) {
                        Some(score) => score.clone(),
                        None => score_for(
                            track,
                            project,
                            &kits.get(kit),
                            audition.as_ref(),
                            rows.get(&track.id),
                        ),
                    };
                    if sequencer.score() != &score && !sequencer.set_score(score) {
                        problems.push(format!(
                            "{}: the audio thread is not taking patterns",
                            track.name
                        ));
                    }
                }
            }
            ordered.push(live);
        }
        self.tracks = ordered;
        self.apply_pad_mixes(project, kits);
        self.apply_strips(project);
        self.apply_compensation();
        self.wire()?;
        self.controller.collect();
        let mut meters = vec![self.input_levels.clone(), self.output_levels.clone()];
        meters.extend(self.tracks.iter().map(|t| t.levels.clone()));
        *self.meters.lock().unwrap_or_else(|e| e.into_inner()) = meters;
        let readouts: Vec<Readout> = self
            .tracks
            .iter()
            .flat_map(|t| t.pedals.iter())
            .filter_map(|p| p.readout.clone())
            .collect();
        *self.readouts.lock().unwrap_or_else(|e| e.into_inner()) = readouts;
        if problems.is_empty() {
            Ok(())
        } else {
            bail!("{}", problems.join("; "))
        }
    }

    fn create_track(
        controller: &mut Controller,
        track: &Track,
        sample_rate: f64,
        kits: &mut Kits,
    ) -> LiveTrack {
        let source = Self::create_source(controller, track, sample_rate, kits);
        let graph = controller.graph_mut();
        let bus = graph.add_processor(cantina_dsp::stock("gain").expect("gain is stock"));
        let delay = graph.add(Box::new(Delay::new(
            MAX_COMPENSATION_SECONDS * sample_rate as usize,
        )));
        let s = &track.strip;
        let strip = graph.add(Box::new(Strip::new(s.gain_db, s.pan, s.mute)));
        let (meter, levels) = Meter::new();
        let meter = graph.add(Box::new(meter));
        LiveTrack {
            id: track.id,
            source,
            bus,
            pedals: Vec::new(),
            delay,
            strip,
            meter,
            levels,
        }
    }

    fn create_source(
        controller: &mut Controller,
        track: &Track,
        sample_rate: f64,
        kits: &mut Kits,
    ) -> Source {
        let graph = controller.graph_mut();
        match &track.kit {
            Some(kit) => {
                let (sequencer, handle) = Sequencer::new(kits.get(kit));
                Source::Drums {
                    kit: kit.clone(),
                    sequencer_node: graph.add(Box::new(sequencer)),
                    sequencer: handle,
                }
            }
            None => {
                let mute = graph.add(Box::new(Mute::new(track.armed)));
                let (player, player_handle) = Player::new(sample_rate, CHANNELS);
                let player_node = graph.add(Box::new(player));
                // Takes are mono: the input is one channel.
                let (recorder, recorder_handle) = Recorder::new(sample_rate, 1);
                let recorder_node = graph.add(Box::new(recorder));
                let (voice, voice_handle) = TabVoice::new();
                let voice_node = graph.add(Box::new(voice));
                Source::Audio {
                    mute,
                    player_node,
                    player: player_handle,
                    recorder_node,
                    recorder: recorder_handle,
                    voice_node,
                    voice: voice_handle,
                }
            }
        }
    }

    /// Sends every drum track the score the project gives it now, when
    /// it changed. Pattern edits come through here without touching the
    /// graph's structure.
    fn refresh_scores(
        &mut self,
        project: &Project,
        kits: &mut Kits,
        audition: &Auditioning,
        rows: &RowMixes,
    ) -> Result<()> {
        for live in &mut self.tracks {
            let Source::Drums { sequencer, kit, .. } = &mut live.source else {
                continue;
            };
            let Some(track) = project.track(live.id) else {
                continue;
            };
            let score = score_for(
                track,
                project,
                &kits.get(kit),
                audition.as_ref(),
                rows.get(&track.id),
            );
            if sequencer.score() != &score && !sequencer.set_score(score) {
                bail!("{}: the audio thread is not taking patterns", track.name);
            }
        }
        Ok(())
    }

    /// Sends every audio track's tab voice what it plays now: after a
    /// tab edit, a tempo change or a change of practice.
    fn refresh_tab_voices(&mut self, project: &Project, practice: Practice) -> Result<()> {
        let sample_rate = self.stream.info().sample_rate;
        for live in &mut self.tracks {
            let Source::Audio { voice, .. } = &mut live.source else {
                continue;
            };
            let Some(track) = project.track(live.id) else {
                continue;
            };
            let score = practice.score(track, project, sample_rate);
            if voice.score() != &score && !voice.set_score(score) {
                bail!("{}: the audio thread is not taking the tab", track.name);
            }
        }
        Ok(())
    }

    /// Delays every track's playback to the slowest chain. Live input
    /// joins after the delay, so monitoring is never compensated.
    fn apply_compensation(&mut self) {
        let latencies: Vec<usize> = self.tracks.iter().map(|t| t.latency()).collect();
        let (max, delays) = plan(&latencies);
        self.compensation = max;
        for (track, delay) in self.tracks.iter().zip(delays) {
            self.controller.set_param(track.delay, 0, delay as f32);
        }
    }

    /// Asks every plugin what its latency is now; when one changed, the
    /// compensation follows. Returns whether anything changed.
    fn refresh_latencies(&mut self) -> bool {
        let mut changed = false;
        for pedal in self.pedals_mut() {
            if let Some(handle) = pedal.handle.as_ref() {
                let now = handle.latency();
                if now != pedal.latency {
                    pedal.latency = now;
                    changed = true;
                }
            }
        }
        if changed {
            self.apply_compensation();
        }
        changed
    }

    /// Plays a pad of a drum track now, through its sampler: it sounds at
    /// the start of the next audio block. False when the track, its kit
    /// or the pad is not there, or the queue is full.
    fn pad_hit(&mut self, track: TrackId, pad: &str, velocity: f32, kits: &mut Kits) -> bool {
        let Some(live) = self.tracks.iter_mut().find(|t| t.id == track) else {
            return false;
        };
        let Source::Drums { sequencer, kit, .. } = &mut live.source else {
            return false;
        };
        let Some(index) = kits.get(kit).pad_index(pad) else {
            return false;
        };
        sequencer.hit(index, velocity)
    }

    /// Sends every drum track's sampler its pad mixer: each pad's level,
    /// pan, tune, decay and mute, by the kit's pad order. The sampler
    /// smooths the level and the mute; pan, tune and decay apply to the
    /// next hits.
    fn apply_pad_mixes(&mut self, project: &Project, kits: &mut Kits) {
        for live in &self.tracks {
            let Source::Drums {
                sequencer_node,
                kit,
                ..
            } = &live.source
            else {
                continue;
            };
            let Some(track) = project.track(live.id) else {
                continue;
            };
            let loaded = kits.get(kit);
            for (i, pad) in loaded.pads().iter().enumerate() {
                let mix = track.pads.get(&pad.id).copied().unwrap_or_default();
                for (which, value) in [
                    (cantina_dsp::PAD_LEVEL, mix.level_db),
                    (cantina_dsp::PAD_PAN, mix.pan),
                    (cantina_dsp::PAD_TUNE, mix.tune),
                    (cantina_dsp::PAD_DECAY, mix.decay),
                    (cantina_dsp::PAD_MUTE, f32::from(u8::from(mix.mute))),
                ] {
                    self.controller.set_param(
                        *sequencer_node,
                        cantina_dsp::pad_param(i, which),
                        value,
                    );
                }
            }
        }
    }

    /// Sends every strip its gain, pan and effective mute. A solo
    /// anywhere mutes every track that is not soloed; solos add up.
    fn apply_strips(&mut self, project: &Project) {
        let any_solo = project.tracks.iter().any(|t| t.strip.solo);
        for track in &project.tracks {
            let Some(live) = self.tracks.iter().find(|t| t.id == track.id) else {
                continue;
            };
            let s = &track.strip;
            let muted = s.mute || (any_solo && !s.solo);
            let node = live.strip;
            self.controller.set_param(node, GAIN, s.gain_db);
            self.controller.set_param(node, PAN, s.pan);
            self.controller
                .set_param(node, MUTE, if muted { 1.0 } else { 0.0 });
        }
        let m = &project.master;
        self.controller.set_param(self.master, GAIN, m.gain_db);
        self.controller
            .set_param(self.master, MUTE, if m.mute { 1.0 } else { 0.0 });
    }

    /// Captures `track`'s input and chain for the last `minutes`, or with
    /// none stops. The rings are allocated here, whole, before their nodes
    /// reach the audio thread; another length makes new ones.
    fn set_capture(&mut self, wanted: Option<(TrackId, f32)>) -> Result<()> {
        let same = match (&self.capture, wanted) {
            (Some(c), Some((track, minutes))) => c.track == track && c.minutes == minutes,
            (None, None) => true,
            _ => false,
        };
        if same {
            return Ok(());
        }
        if let Some(old) = self.capture.take() {
            let graph = self.controller.graph_mut();
            graph.remove(old.dry_node)?;
            graph.remove(old.wet_node)?;
        }
        if let Some((track, minutes)) = wanted {
            let frames = (f64::from(minutes) * 60.0 * self.stream.info().sample_rate) as usize;
            let dry = Arc::new(cantina_engine::CaptureRing::new(1, frames));
            let wet = Arc::new(cantina_engine::CaptureRing::new(CHANNELS, frames));
            let graph = self.controller.graph_mut();
            let dry_node = graph.add(Box::new(cantina_engine::Capture::new(Arc::clone(&dry))));
            let wet_node = graph.add(Box::new(cantina_engine::Capture::new(Arc::clone(&wet))));
            self.capture = Some(LiveCapture {
                track,
                dry_node,
                wet_node,
                dry,
                wet,
                minutes,
            });
        }
        Ok(())
    }

    /// Connects everything for the current tracks and commits:
    ///
    /// ```text
    /// input -> mute -> bus -> pedals... -> strip -> master -> output
    ///            |      ^                    |         |
    ///       recorder  delay <- player      meter   output meter
    /// input -> input meter
    /// ```
    ///
    /// The scope taps the master, or the strip of the track the stage
    /// listens to. The click goes to the output alone.
    fn wire(&mut self) -> Result<()> {
        let graph = self.controller.graph_mut();
        graph.clear_edges();
        graph.connect(Endpoint::Input, self.input_meter)?;
        for track in &self.tracks {
            match &track.source {
                Source::Audio {
                    mute,
                    player_node,
                    recorder_node,
                    voice_node,
                    ..
                } => {
                    graph.connect(Endpoint::Input, *mute)?;
                    graph.connect(*mute, track.bus)?;
                    graph.connect(*mute, *recorder_node)?;
                    if let Some(c) = self.capture.as_ref().filter(|c| c.track == track.id) {
                        graph.connect(*mute, c.dry_node)?;
                    }
                    graph.connect(*player_node, track.delay)?;
                    // The tab voice plays like the recording, through the
                    // track's pedals.
                    graph.connect(*voice_node, track.delay)?;
                }
                Source::Drums { sequencer_node, .. } => {
                    graph.connect(*sequencer_node, track.delay)?;
                }
            }
            graph.connect(track.delay, track.bus)?;
            let mut previous = track.bus;
            for pedal in track.pedals.iter().filter(|p| !p.bypassed) {
                graph.connect(previous, pedal.node)?;
                previous = pedal.node;
            }
            graph.connect(previous, track.strip)?;
            if let Some(c) = self.capture.as_ref().filter(|c| c.track == track.id) {
                graph.connect(previous, c.wet_node)?;
            }
            graph.connect(track.strip, track.meter)?;
            graph.connect(track.strip, self.master)?;
        }
        graph.connect(self.master, self.output_meter)?;
        graph.connect(self.master, Endpoint::Output)?;
        graph.connect(self.click, Endpoint::Output)?;
        let tapped = self
            .analysis_source
            .and_then(|id| self.tracks.iter().position(|t| t.id == id));
        let (from, meter) = match tapped {
            Some(i) => (self.tracks[i].strip, 2 + i as u32),
            None => (self.master, 1),
        };
        graph.connect(from, self.scope)?;
        self.analysis.meter.store(meter, Ordering::Relaxed);
        self.controller.commit()?;
        Ok(())
    }
}

/// What a drum track plays: its pattern clips, or, while a pattern is
/// auditioned on it, that pattern alone looping from the start.
pub(crate) fn score_for(
    track: &Track,
    project: &Project,
    kit: &LoadedKit,
    audition: Option<&(TrackId, Audition)>,
    mix: Option<&RowMix>,
) -> Score {
    if let Some((t, what)) = audition {
        if *t == track.id {
            return match what {
                Audition::Pattern(id) => project
                    .pattern(*id)
                    .map(|p| vec![Placement::forever(pattern_data(p, kit, mix))])
                    .unwrap_or_default(),
                Audition::Groove { pattern, from, .. } => vec![Placement {
                    start: *from,
                    ..Placement::forever(pattern_data(pattern, kit, mix))
                }],
            };
        }
    }
    // One PatternData per pattern, shared by every clip of it. A clip
    // whose pattern is gone (which validation refuses) plays nothing.
    let mut data: HashMap<PatternId, Arc<PatternData>> = HashMap::new();
    let mut score = Score::with_capacity(track.pattern_clips.len());
    for clip in &track.pattern_clips {
        let Some(pattern) = project.pattern(clip.pattern) else {
            continue;
        };
        let base = data
            .entry(clip.pattern)
            .or_insert_with(|| pattern_data(pattern, kit, mix))
            .clone();
        let fill = clip.fill.as_ref().and_then(|f| {
            let pattern = project.pattern(f.pattern)?;
            let data = data
                .entry(f.pattern)
                .or_insert_with(|| pattern_data(pattern, kit, mix))
                .clone();
            Some((f, data))
        });
        let Some((fill, fill_data)) = fill else {
            score.push(Placement {
                pattern: base,
                start: clip.start,
                offset: clip.offset,
                length: clip.length,
            });
            continue;
        };
        // Bars count from the clip's start. A fill bar plays the fill
        // pattern's first bar; the bars between play the clip's pattern
        // as they would have, so the groove does not restart after a fill.
        let tempo = &project.tempo;
        let bar = u64::from(cantina_project::PPQ) * 4 / u64::from(tempo.denominator.max(1))
            * u64::from(tempo.numerator.max(1));
        let bars = clip.length.div_ceil(bar);
        let mut b = 0;
        while b < bars {
            let from = b * bar;
            if fill.plays_at(b, bars) {
                score.push(Placement {
                    pattern: fill_data.clone(),
                    start: clip.start + from,
                    offset: 0,
                    length: bar.min(clip.length - from),
                });
                b += 1;
                continue;
            }
            let mut end = b + 1;
            while end < bars && !fill.plays_at(end, bars) {
                end += 1;
            }
            score.push(Placement {
                pattern: base.clone(),
                start: clip.start + from,
                offset: clip.offset + from,
                length: (end * bar).min(clip.length) - from,
            });
            b = end;
        }
    }
    score
}

/// A pattern with its pads resolved against a kit, leaving out the rows
/// `mix` silences.
fn pattern_data(pattern: &Pattern, kit: &LoadedKit, mix: Option<&RowMix>) -> Arc<PatternData> {
    let mut rows = Vec::new();
    let mut feel = Vec::new();
    for row in &pattern.rows {
        if !mix.is_none_or(|m| m.plays(&row.pad)) {
            continue;
        }
        if let Some(i) = kit.pad_index(&row.pad) {
            rows.push((i, row.steps.clone()));
            feel.push(row_feel(row));
        }
    }
    // The hits are worked out here, off the audio thread.
    Arc::new(PatternData::with_feel(
        pattern.steps_per_beat as u32,
        pattern.length as u32,
        pattern.swing,
        rows,
        feel,
        pattern_humanize(pattern),
        pattern.seed,
    ))
}

/// A row's feel as the sequencer takes it.
fn row_feel(row: &cantina_project::PatternRow) -> cantina_engine::sequencer::RowFeel {
    cantina_engine::sequencer::RowFeel {
        swing: row.swing,
        humanize: row.humanize.map(|h| cantina_engine::sequencer::Humanize {
            timing: h.timing,
            velocity: h.velocity,
        }),
        marks: row
            .marks
            .iter()
            .map(|m| cantina_engine::sequencer::Mark {
                step: u32::from(m.step),
                chance: m.chance,
                ratchet: m.ratchet,
                flam: m.flam,
            })
            .collect(),
    }
}

/// A pattern's own humanize as the sequencer takes it.
fn pattern_humanize(pattern: &Pattern) -> cantina_engine::sequencer::Humanize {
    pattern
        .humanize
        .map(|h| cantina_engine::sequencer::Humanize {
            timing: h.timing,
            velocity: h.velocity,
        })
        .unwrap_or_default()
}

/// The installed drum kits: found once, each loaded the first time a
/// track asks for it and kept for the session.
#[derive(Default)]
pub struct Kits {
    roots: Vec<PathBuf>,
    entries: Vec<KitEntry>,
    loaded: HashMap<String, Arc<LoadedKit>>,
    /// Kits that would not load, by id, with the reason.
    failed: HashMap<String, String>,
}

impl Kits {
    /// Looks in `roots`, first root winning on a duplicate id.
    fn discover(roots: Vec<PathBuf>, problems: &mut Vec<String>) -> Self {
        let entries = kit::discover(&roots, problems);
        Self {
            roots,
            entries,
            loaded: HashMap::new(),
            failed: HashMap::new(),
        }
    }

    /// Looks again, keeping what is loaded.
    fn rescan(&mut self, problems: &mut Vec<String>) {
        self.entries = kit::discover(&self.roots, problems);
        self.failed.clear();
    }

    fn entries(&self) -> &[KitEntry] {
        &self.entries
    }

    fn entry(&self, id: &str) -> Option<&KitEntry> {
        self.entries.iter().find(|k| k.id == id)
    }

    /// The kit, loading it the first time. A kit that is not installed
    /// or will not load comes back empty, so the track plays silence,
    /// and the reason is kept for the status.
    fn get(&mut self, id: &str) -> Arc<LoadedKit> {
        if let Some(kit) = self.loaded.get(id) {
            return Arc::clone(kit);
        }
        let kit = match self.entry(id).cloned() {
            Some(entry) => match kit::load(&entry.path) {
                Ok((_, loaded)) => Arc::new(loaded),
                Err(e) => {
                    self.failed.insert(id.to_string(), format!("{e:#}"));
                    Arc::new(LoadedKit::empty(&entry.name))
                }
            },
            None => {
                self.failed
                    .insert(id.to_string(), format!("no kit {id:?} is installed"));
                Arc::new(LoadedKit::empty(id))
            }
        };
        self.loaded.insert(id.to_string(), Arc::clone(&kit));
        kit
    }

    /// Why `get` came back empty for `id`, if it did.
    fn problem(&self, id: &str) -> Option<&str> {
        self.failed.get(id).map(String::as_str)
    }
}

/// User-designed pedals by id: what a `pedal:<id>` spec compiles. The
/// designer puts drafts here; saving and loading them is the next step.
type Designs = HashMap<String, PedalDef>;

/// Builds a processor for a spec: a cabinet with its impulse response
/// loaded (`cab:<id>`), a designed pedal compiled from its definition
/// (`pedal:<id>`), or whatever the engine's `processors` make.
fn build_processor(
    irs: &mut Irs,
    designs: &Designs,
    spec: &str,
    channels: usize,
) -> Result<Box<dyn Processor>> {
    if let Some(id) = spec.strip_prefix("cab:") {
        let impulse = irs.load(id)?;
        let mut cab = Cabinet::default();
        cab.set_impulse(&impulse.samples, impulse.sample_rate);
        return Ok(Box::new(cab));
    }
    if let Some(id) = spec.strip_prefix("pedal:") {
        // A design the project refers to but has no copy of passes the
        // signal through under a name that says so, rather than
        // taking the whole chain down.
        let missing;
        let def = match designs.get(id) {
            Some(def) => def,
            None => {
                missing = PedalDef::empty(&format!("{id} (missing)"));
                &missing
            }
        };
        return Ok(Box::new(cantina_dsp::compile(def)?));
    }
    processors::create(spec, channels)
}

/// The installed cabinet impulse responses: found once, each loaded
/// the first time a cabinet asks for it and kept for the session.
#[derive(Default)]
pub struct Irs {
    roots: Vec<PathBuf>,
    entries: Vec<IrEntry>,
    loaded: HashMap<String, Arc<Impulse>>,
}

impl Irs {
    fn discover(roots: Vec<PathBuf>) -> Self {
        let entries = ir::discover(&roots);
        Self {
            roots,
            entries,
            loaded: HashMap::new(),
        }
    }

    /// Looks again, keeping what is loaded.
    fn rescan(&mut self) {
        self.entries = ir::discover(&self.roots);
    }

    fn entries(&self) -> &[IrEntry] {
        &self.entries
    }

    fn entry(&self, id: &str) -> Option<&IrEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The first installed impulse, for a new cabinet.
    fn first_id(&self) -> Option<&str> {
        self.entries.first().map(|e| e.id.as_str())
    }

    /// The impulse, loading it the first time.
    fn load(&mut self, id: &str) -> Result<Arc<Impulse>> {
        if let Some(impulse) = self.loaded.get(id) {
            return Ok(Arc::clone(impulse));
        }
        let entry = self
            .entry(id)
            .cloned()
            .with_context(|| format!("no impulse response {id:?} is installed"))?;
        let impulse = Arc::new(ir::load(&entry.path)?);
        self.loaded.insert(id.to_string(), Arc::clone(&impulse));
        Ok(impulse)
    }
}

/// The user's pedal library: found once, looked for again on request.
#[derive(Default)]
pub struct Library {
    roots: Vec<PathBuf>,
    entries: Vec<PedalEntry>,
}

impl Library {
    fn discover(roots: Vec<PathBuf>, problems: &mut Vec<String>) -> Self {
        let entries = pedals::discover(&roots, problems);
        Self { roots, entries }
    }

    fn rescan(&mut self, problems: &mut Vec<String>) {
        self.entries = pedals::discover(&self.roots, problems);
    }

    fn entries(&self) -> &[PedalEntry] {
        &self.entries
    }

    fn entry(&self, id: &str) -> Option<&PedalEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Where saves and imports go: the first root.
    fn dir(&self) -> &Path {
        self.roots.first().map_or(Path::new("."), |p| p.as_path())
    }
}

/// The project's clips for one track as the player wants them.
pub(crate) fn engine_clips(track: &Track, audio_dir: &Path) -> Vec<cantina_engine::Clip> {
    track
        .clips
        .iter()
        .map(|c| cantina_engine::Clip {
            path: audio_dir.join(&c.file),
            start: c.start,
            offset: c.offset,
            length: c.length,
        })
        .collect()
}

/// Writes an export to `path`, with `extension` added when it lacks it,
/// and returns the path written.
fn write_export(path: &str, extension: &str, bytes: &[u8]) -> Result<String> {
    let mut path = PathBuf::from(path);
    if path.extension().and_then(|e| e.to_str()) != Some(extension) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Tab".into());
        path.set_file_name(format!("{name}.{extension}"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path.display().to_string())
}

/// Pedal kinds by spec.
#[derive(Default)]
pub struct Catalog(HashMap<String, PedalKind>);

impl Catalog {
    fn learn(&mut self, spec: &str, processor: &dyn cantina_dsp::Processor) {
        self.0.entry(spec.to_string()).or_insert_with(|| PedalKind {
            name: processor.name().to_string(),
            params: processor.params().to_vec(),
            readouts: processor
                .readout()
                .map(|r| r.names().iter().map(|n| n.to_string()).collect())
                .unwrap_or_default(),
        });
    }

    /// What `spec` builds, building one to find out the first time.
    fn get(&mut self, spec: &str, irs: &mut Irs, designs: &Designs) -> Result<&PedalKind> {
        if !self.0.contains_key(spec) {
            let processor = build_processor(irs, designs, spec, 1)?;
            self.learn(spec, processor.as_ref());
        }
        Ok(&self.0[spec])
    }

    /// Drops what was learned about `spec`, for a design that changed.
    fn forget(&mut self, spec: &str) {
        self.0.remove(spec);
    }

    fn describe(&self, pedal: &Pedal) -> PedalInfo {
        match self.0.get(&pedal.spec) {
            Some(kind) => kind.info(pedal),
            None => PedalInfo {
                id: pedal.id.0,
                spec: pedal.spec.clone(),
                name: pedal.spec.clone(),
                params: Vec::new(),
                has_editor: false,
                editor_open: false,
                latency_frames: 0,
                readouts: Vec::new(),
                bypassed: pedal.bypassed,
            },
        }
    }
}

/// A take being written.
struct Recording {
    track: TrackId,
    file: String,
    name: String,
    /// Timeline sample it started at.
    from: u64,
    /// Frames the take is moved earlier when placed: what the player
    /// heard was that late (compensation plus the output latency), and
    /// what they played arrived the input latency after that.
    correction: u64,
}

/// How long `transport_stop` waits for a writer to finalize its take.
const TAKE_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// The engine as the shell drives it.
pub struct Engine {
    app: AppHandle,
    live: Option<Live>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    projects: Projects,
    project: Project,
    project_dir: PathBuf,
    dirty: bool,
    history: History,
    journal: Journal,
    /// Journal entries newer than the saved file, until recovered or
    /// discarded.
    recovery: Option<Vec<JournalEntry>>,
    /// Plugin state by pedal: what `plugins/` held when the project was
    /// opened, refreshed from the live plugins on save and whenever the
    /// stream closes, so a rebuild hands each plugin back its state.
    plugin_states: PluginStates,
    /// Open editor windows, main thread only.
    editors: MainThreadCell<HashMap<PedalId, Editor>>,
    /// Which pedals have an editor open, for the status from any thread.
    open_editors: HashSet<PedalId>,
    catalog: Catalog,
    kits: Kits,
    irs: Irs,
    /// The designed pedals the project uses, by the id in their
    /// `pedal:<id>` spec: the project's own copies, plus drafts and
    /// library pedals added this session. Written to `pedals/` on save.
    designs: Designs,
    library: Library,
    /// The user's library folders, and what scanning them last found.
    folders: library::Folders,
    library_status: library::LibraryStatus,
    /// The pattern looping on a drum track for editing. Session state,
    /// not part of the project.
    audition: Auditioning,
    /// The track the stage analyzes, or none for the master. Session
    /// state, kept across rebuilds of the stream.
    analysis_source: Option<TrackId>,
    /// Hearing tabs and practice speed. Session state, kept across
    /// rebuilds of the stream.
    practice: Practice,
    /// The stock grooves, found once at launch.
    grooves: Vec<library::StockGroove>,
    /// The stock rigs, found once at launch.
    stock_rigs: Vec<crate::rigs::RigInfo>,
    /// The stock jams, found once at launch.
    stock_jams: Vec<crate::jams::JamInfo>,
    /// A rig heard on a track in place of its chain.
    rig_audition: Option<RigAudition>,
    /// Rows muted or soloed on drum tracks, for listening while editing.
    drum_rows: RowMixes,
    /// The jam heard in place of the project while jam mode is open.
    jam: Option<JamSession>,
    recording: Vec<Recording>,
    last_error: Option<String>,
    notice: Option<String>,
    notice_seq: u32,
    meter_subscribers: Arc<Mutex<Vec<Subscriber>>>,
    /// Shared with the meter pump so frames carry the transport state.
    transport_state: Arc<TransportState>,
    /// Frames skipped because the UI had not taken the previous one.
    frames_dropped: Arc<AtomicU32>,
    /// What the scanner found last, from the cache until a scan runs.
    plugins: PluginList,
    plugins_path: Option<PathBuf>,
    scanning: bool,
}

/// The assistant's tools work on the engine through the same methods
/// and the same `apply` as the UI.
impl crate::assistant::tools::Host for Engine {
    fn project(&self) -> &Project {
        &self.project
    }

    fn revision(&self) -> u64 {
        self.project.revision
    }

    fn apply(&mut self, command: Command) -> Result<()> {
        Engine::apply(self, command).map(drop)
    }

    fn pedal_kinds(&self) -> Vec<(String, String)> {
        stock_kinds()
            .iter()
            .map(|k| (k.spec.clone(), k.name.clone()))
            .chain(
                self.library
                    .entries()
                    .iter()
                    .map(|e| (format!("pedal:{}", e.id), e.name.clone())),
            )
            .collect()
    }

    fn pedal_kind(&mut self, spec: &str) -> Result<(String, Vec<ParamInfo>)> {
        let kind = self.catalog.get(spec, &mut self.irs, &self.designs)?;
        Ok((kind.name.clone(), kind.params.clone()))
    }

    fn add_pedal(
        &mut self,
        track: TrackId,
        spec: &str,
        index: Option<usize>,
        values: Option<Vec<f32>>,
    ) -> Result<PedalId> {
        self.chain_add_with(track, spec, index, values)
    }

    fn kits(&self) -> Vec<(String, String)> {
        self.kits
            .entries()
            .iter()
            .filter(|k| !self.is_broken(&k.path))
            .map(|k| (k.id.clone(), k.name.clone()))
            .collect()
    }

    fn pads(&self, kit: &str) -> Vec<String> {
        self.kit_status(kit)
            .pads
            .into_iter()
            .map(|p| p.id)
            .collect()
    }

    fn add_drum_track(&mut self, kit: Option<String>) -> Result<TrackId> {
        let id = TrackId(self.project.fresh_id());
        self.track_add_drums(kit)?;
        Ok(id)
    }

    fn remove_track(&mut self, track: TrackId) -> Result<()> {
        self.track_remove(track.0).map(drop)
    }

    fn library(&self) -> library::Folders {
        self.folders.clone()
    }

    fn kit_dir(&self, kit: &str) -> Option<PathBuf> {
        self.kits.entry(kit).map(|k| k.path.clone())
    }

    fn rescan_library(&mut self) {
        self.library_rescan();
    }

    fn set_chain(
        &mut self,
        track: TrackId,
        pedals: Vec<(String, Vec<f32>)>,
        replace: bool,
    ) -> Result<Vec<PedalId>> {
        self.chain_set(track, pedals, replace)
    }

    fn import_pattern(&mut self, path: &Path, track: Option<TrackId>) -> Result<PatternId> {
        let id = PatternId(self.project.fresh_id());
        self.pattern_import(&path.display().to_string(), track.map(|t| t.0))?;
        Ok(id)
    }

    fn engine_report(&self) -> serde_json::Value {
        let status = self.status();
        let rate = status.sample_rate;
        let ms = |frames: f64| (frames / rate * 10_000.0).round() / 10.0;
        let db = |level: f32| {
            if level <= 0.0 {
                None
            } else {
                Some((cantina_dsp::linear_to_db(level) * 10.0).round() / 10.0)
            }
        };
        let live = self.live.as_ref().map(|l| {
            let stats = l.stream.stats();
            let (input, output) = (l.input_levels.peek(0), l.output_levels.peek(0));
            serde_json::json!({
                "inputPeakDb": db(input.peak),
                "outputPeakDb": db(output.peak),
                "inputClipping": input.peak >= 0.99,
                "outputUnderrunFrames": stats.output_underruns.load(Ordering::Relaxed),
                "inputOverrunFrames": stats.input_overruns.load(Ordering::Relaxed),
                "outputCallbacks": stats.output_callbacks.load(Ordering::Relaxed),
                "largestBlockFrames": stats.max_block_frames.load(Ordering::Relaxed),
            })
        });
        // Pedals in the project that cannot be built: a plugin the scanner
        // has not found, or a design the project has no copy of.
        let scanned: HashSet<&str> = self
            .plugins
            .plugins
            .iter()
            .map(|p| p.spec.as_str())
            .collect();
        let unavailable: Vec<serde_json::Value> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.chain.iter().map(move |p| (t, p)))
            .filter_map(|(t, p)| {
                let why = if p.spec.starts_with("au:") || p.spec.starts_with("clap:") {
                    (!scanned.contains(p.spec.as_str()))
                        .then_some("the plugin is not installed, or has not been scanned")
                } else if let Some(id) = p.spec.strip_prefix("pedal:") {
                    (!self.designs.contains_key(id))
                        .then_some("the designed pedal's file is missing; it passes the signal through")
                } else {
                    None
                }?;
                Some(serde_json::json!({ "track": t.name, "pedal": p.id.0, "kind": p.spec, "why": why }))
            })
            .collect();
        serde_json::json!({
            "running": status.running,
            "input": status.input_name,
            "output": status.output_name,
            "sampleRate": rate,
            "bufferFrames": status.buffer_frames,
            "bufferMs": ms(status.buffer_frames as f64),
            "latency": {
                "inputMs": ms(status.latency.input_frames as f64),
                "outputMs": ms(status.latency.output_frames as f64),
                "pluginCompensationMs": ms(status.latency.compensation_frames as f64),
                "roundTripMs": ms((status.latency.input_frames + status.latency.output_frames) as f64),
            },
            "live": live,
            "pedalsUnavailable": unavailable,
            "lastError": status.error,
            "notice": status.notice,
            "pluginsFailedToScan": self.plugins.failed.iter().map(|f| serde_json::json!({
                "name": f.name,
                "why": f.why,
            })).collect::<Vec<_>>(),
        })
    }
}

/// What the meter pump reads to fill in a frame.
#[derive(Default)]
struct TransportState {
    recording: AtomicBool,
}

impl Engine {
    fn new(app: AppHandle) -> Self {
        let settings_path = app
            .path()
            .app_config_dir()
            .ok()
            .map(|dir| dir.join("settings.json"));
        let plugins_path = app
            .path()
            .app_data_dir()
            .ok()
            .map(|dir| dir.join("plugins.json"));
        let mut last_error = None;
        let plugins = match plugins_path.as_deref().map(PluginList::load) {
            Some(Ok(list)) => list,
            Some(Err(e)) => {
                last_error = Some(format!("{e:#}"));
                PluginList::default()
            }
            None => PluginList::default(),
        };
        let settings = match settings_path.as_deref().map(Settings::load) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                last_error = Some(format!("{e:#}"));
                Settings::default()
            }
            None => Settings::default(),
        };
        let projects = Projects::locate(&app);
        let mut kit_problems = Vec::new();
        let kits = Kits::discover(kit_roots(&app, &projects), &mut kit_problems);
        let irs = Irs::discover(ir_roots(&app, &projects));
        let library = Library::discover(pedal_roots(&app, &projects), &mut kit_problems);
        if !kit_problems.is_empty() {
            last_error = Some(kit_problems.join("; "));
        }
        let (project, project_dir) = match projects
            .open_last_or_untitled(settings.last_project.as_deref(), settings.audio.sample_rate)
        {
            Ok(opened) => opened,
            Err(e) => {
                last_error = Some(format!("{e:#}"));
                let project = Projects::untitled(settings.audio.sample_rate);
                let dir = projects.dir_for(&project.name);
                (project, dir)
            }
        };
        let journal = Journal::in_dir(&project_dir);
        let folders = library::Folders {
            pedals: projects.pedals_dir(),
            kits: projects.kits_dir(),
            patterns: projects.patterns_dir(),
            rigs: projects.rigs_dir(),
            jams: projects.jams_dir(),
        };
        // The folders are where files written outside the app go, so they
        // exist from the first launch.
        for dir in folders.all() {
            let _ = std::fs::create_dir_all(dir);
        }
        let library_status = folders.scan();
        let grooves = library::scan_grooves(&groove_roots(&app));
        let stock_rigs = crate::rigs::scan_stock(&rig_roots(&app));
        let stock_jams = crate::jams::scan_stock(&jam_roots(&app));
        let mut engine = Self {
            app,
            live: None,
            settings,
            settings_path,
            projects,
            project,
            project_dir,
            dirty: false,
            history: History::new(),
            journal,
            recovery: None,
            plugin_states: PluginStates::new(),
            editors: MainThreadCell(HashMap::new()),
            open_editors: HashSet::new(),
            catalog: Catalog::default(),
            designs: HashMap::new(),
            library,
            folders,
            library_status,
            kits,
            irs,
            audition: None,
            analysis_source: None,
            practice: Practice::default(),
            grooves,
            stock_rigs,
            stock_jams,
            rig_audition: None,
            drum_rows: RowMixes::new(),
            jam: None,
            recording: Vec::new(),
            last_error,
            notice: None,
            notice_seq: 0,
            meter_subscribers: Arc::new(Mutex::new(Vec::new())),
            transport_state: Arc::new(TransportState::default()),
            frames_dropped: Arc::new(AtomicU32::new(0)),
            plugins,
            plugins_path,
            scanning: false,
        };
        engine.adopt_sample_rate();
        engine.remember_project();
        engine.read_journal();
        engine.plugin_states = read_plugin_states(&engine.project_dir);
        // For measuring the UI's cost of the frame stream without audio:
        // a pump of synthetic levels with no stream behind it.
        if std::env::var_os("CANTINA_SYNTHETIC_METERS").is_some() {
            engine.spawn_synthetic_pump();
        }
        engine
    }

    fn spawn_synthetic_pump(&self) {
        let (_, input) = Meter::new();
        let (_, output) = Meter::new();
        let (_, controller) = Runtime::new(GraphConfig {
            sample_rate: 48_000.0,
            max_block_frames: MAX_BLOCK_FRAMES,
            channels: 1,
        })
        .expect("a one-channel graph is valid");
        let analysis = Arc::new(PumpAnalysis {
            scope: Scope::new().1,
            meter: AtomicU32::new(1),
            tempo: Mutex::new(TempoMap::default()),
            output_latency: 0,
        });
        spawn_meter_pump(
            self.app.clone(),
            Arc::clone(&self.meter_subscribers),
            Arc::new(Mutex::new(vec![input, output])),
            Arc::new(Mutex::new(Vec::new())),
            analysis,
            controller.transport().clock(),
            48_000.0,
            Arc::clone(&self.transport_state),
            Arc::clone(&self.frames_dropped),
            Arc::new(AtomicBool::new(false)),
        );
        // The controller must outlive the pump's clock view of it.
        std::mem::forget(controller);
    }

    pub fn status(&self) -> EngineStatus {
        let a = &self.settings.audio;
        let (sample_rate, buffer_frames) = match &self.live {
            Some(l) => (l.stream.info().sample_rate, l.stream.info().buffer_frames),
            None => (a.sample_rate, a.buffer_frames),
        };
        EngineStatus {
            running: self.live.is_some(),
            sample_rate,
            buffer_frames,
            input_uid: a.input_uid.clone(),
            output_uid: a.output_uid.clone(),
            input_name: self.live.as_ref().map(|l| l.input_name.clone()),
            output_name: self.live.as_ref().map(|l| l.output_name.clone()),
            error: self.last_error.clone(),
            notice: self.notice.clone(),
            notice_seq: self.notice_seq,
            transport: TransportStatus {
                playing: self
                    .live
                    .as_ref()
                    .is_some_and(|l| l.controller.transport().is_playing()),
                recording: !self.recording.is_empty(),
            },
            project: self.project_status(),
            pedal_kinds: stock_kinds()
                .iter()
                .cloned()
                .chain(self.library.entries().iter().map(|e| PedalKindInfo {
                    spec: format!("pedal:{}", e.id),
                    name: e.name.clone(),
                    stock: false,
                    designable: true,
                }))
                .collect(),
            plugins: PluginsStatus {
                scanning: self.scanning,
                count: self.plugins.plugins.len(),
                failed: self.plugins.failed.len(),
                scanned_at: self.plugins.scanned_at,
            },
            latency: self.latency_status(),
            kits: self
                .kits
                .entries()
                .iter()
                .filter(|k| !self.is_broken(&k.path))
                .map(|k| KitInfo {
                    id: k.id.clone(),
                    name: k.name.clone(),
                })
                .collect(),
            irs: self
                .irs
                .entries()
                .iter()
                .map(|e| IrInfo {
                    id: e.id.clone(),
                    name: e.name.clone(),
                })
                .collect(),
            audition: self.audition.as_ref().map(|(track, what)| AuditionStatus {
                track: track.0,
                pattern: match what {
                    Audition::Pattern(id) => Some(id.0),
                    Audition::Groove { .. } => None,
                },
                groove: match what {
                    Audition::Groove { path, .. } => Some(path.clone()),
                    Audition::Pattern(_) => None,
                },
            }),
            analysis_source: self
                .analysis_source
                .filter(|id| self.project.track(*id).is_some())
                .map(|t| t.0),
            library: self.library_status.clone(),
            metronome: MetronomeStatus {
                settings: self.settings.metronome.clone(),
                sounds: cantina_engine::CLICK_SOUNDS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            count_in: self.settings.count_in,
            open_last_at_launch: self.settings.open_last_at_launch,
            assistant: crate::assistant::AssistantStatus::of(&self.settings.assistant),
            practice: self.practice,
            grooves: self.grooves.clone(),
            rigs: self
                .stock_rigs
                .iter()
                .chain(&self.library_status.rigs)
                .cloned()
                .collect(),
            jams: self
                .stock_jams
                .iter()
                .chain(&self.library_status.jams)
                .cloned()
                .collect(),
            instrument_profile: InstrumentProfileStatus {
                profile: self.settings.instrument.clone(),
                trim_db: self.settings.instrument.trim_db(),
            },
            rig_audition: self.rig_audition.as_ref().map(|a| RigAuditionStatus {
                track: a.track.0,
                path: a.path.clone(),
                hearing_rig: a.hearing_rig,
                missing: a.missing.clone(),
            }),
            drum_rows: self
                .drum_rows
                .iter()
                .filter(|(_, m)| !m.muted.is_empty() || !m.soloed.is_empty())
                .map(|(t, m)| (t.0, m.clone()))
                .collect(),
            jam: self.jam_status(),
        }
    }

    /// True when the library scan found the file or folder at `path`
    /// broken, so it is listed as such instead of offered.
    fn is_broken(&self, path: &Path) -> bool {
        let path = path.display().to_string();
        self.library_status.broken.iter().any(|b| b.path == path)
    }

    /// The library folders, for the watcher.
    pub fn library_folders(&self) -> library::Folders {
        self.folders.clone()
    }

    /// Scans the library folders again: pedals, kits and patterns, with
    /// broken files listed rather than reported. A kit loaded from the
    /// user's folder is dropped from the cache so it is read afresh the
    /// next time a track asks for it.
    pub fn library_rescan(&mut self) -> EngineStatus {
        let mut ignored = Vec::new();
        self.library.rescan(&mut ignored);
        self.kits.rescan(&mut ignored);
        let user_kits = self.folders.kits.clone();
        let stale: Vec<String> = self
            .kits
            .entries()
            .iter()
            .filter(|k| k.path.starts_with(&user_kits))
            .map(|k| k.id.clone())
            .collect();
        let in_use: HashSet<&str> = self
            .project
            .tracks
            .iter()
            .filter_map(|t| t.kit.as_deref())
            .collect();
        for id in stale {
            if !in_use.contains(id.as_str()) {
                self.kits.loaded.remove(&id);
            }
        }
        self.library_status = self.folders.scan();
        self.publish()
    }

    /// Adds a pattern file to the project with a fresh id, renamed on a
    /// clash. Rows for pads the track's kit lacks are kept, and named in
    /// a notice.
    pub fn pattern_import(&mut self, path: &str, track: Option<u64>) -> Result<EngineStatus> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let pads: Option<Vec<String>> = track
            .and_then(|t| self.project.track(TrackId(t)))
            .and_then(|t| t.kit.clone())
            .map(|kit| {
                self.kit_status(&kit)
                    .pads
                    .into_iter()
                    .map(|p| p.id)
                    .collect()
            });
        let (pattern, missing) = library::import_pattern(&text, &self.project, pads.as_deref())
            .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        let name = pattern.name.clone();
        let status = self.apply(Command::InsertPattern {
            index: usize::MAX,
            pattern,
        })?;
        if missing.is_empty() {
            return Ok(status);
        }
        self.notice = Some(format!(
            "Imported {name}. This kit has no {} pad{}; {} kept for other kits.",
            missing.join(", "),
            if missing.len() == 1 { "" } else { "s" },
            if missing.len() == 1 {
                "that row is"
            } else {
                "those rows are"
            }
        ));
        self.notice_seq += 1;
        Ok(self.publish())
    }

    /// Writes a pattern as a `*.pattern.json` file, adding the suffix
    /// when the path lacks it.
    /// Writes a clip's tab to `path` as `format` (`musicxml`, `ascii`
    /// or `midi`), adding the format's extension when it is missing. A
    /// PDF is drawn by the UI and written with [`Engine::tab_pdf_write`].
    pub fn clip_tab_export(&self, id: u64, format: &str, path: &str) -> Result<String> {
        let (_, clip) = self
            .project
            .clip(ClipId(id))
            .with_context(|| format!("no clip {id}"))?;
        let tab = clip
            .tab
            .as_ref()
            .with_context(|| format!("{} has no tab", clip.name))?;
        let mut tab = tab.clone();
        if tab.name.is_empty() {
            tab.name.clone_from(&clip.name);
        }
        use cantina_project::tab_export;
        let (bytes, extension) = match format {
            "musicxml" => (tab_export::musicxml(&tab).into_bytes(), "musicxml"),
            "ascii" => (tab_export::ascii(&tab).into_bytes(), "txt"),
            "midi" => (tab_export::midi(&tab), "mid"),
            other => bail!("no tab export as {other:?}"),
        };
        write_export(path, extension, &bytes)
    }

    /// Writes the PDF the UI drew from a tab's page view.
    pub fn tab_pdf_write(&self, path: &str, bytes: &[u8]) -> Result<String> {
        if !bytes.starts_with(b"%PDF-") {
            bail!("that is not a PDF");
        }
        write_export(path, "pdf", bytes)
    }

    pub fn pattern_export(&self, id: u64, path: &str) -> Result<()> {
        let pattern = self
            .project
            .pattern(PatternId(id))
            .with_context(|| format!("no pattern {id}"))?;
        let mut path = PathBuf::from(path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.ends_with(cantina_project::PATTERN_SUFFIX) {
            let stem = name.trim_end_matches(".json").trim_end_matches(".pattern");
            path.set_file_name(format!("{stem}{}", cantina_project::PATTERN_SUFFIX));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&path, cantina_project::pattern_to_json(pattern))
            .with_context(|| format!("writing {}", path.display()))
    }

    // ----- mixdown -----

    /// The timeline an export covers: the whole song, or from the start
    /// of one bar to the end of another, counting from 1.
    fn export_range(&self, from_bar: Option<u32>, to_bar: Option<u32>) -> Result<mixdown::Range> {
        let map = tempo_map(&self.project.tempo);
        let rate = self.project.sample_rate;
        let bar = |n: u32| map.position_to_samples(MusicalPosition::new(n, 0, 0), rate);
        let start = from_bar.map_or(0, |b| bar(b.saturating_sub(1)));
        let end = match to_bar {
            Some(b) => bar(b.max(1)),
            None => mixdown::song_end(&self.project),
        };
        if end <= start {
            bail!(if to_bar.is_none() && from_bar.is_none() {
                "there is nothing on the timeline to export".to_string()
            } else {
                "the range is empty: the last bar must come after the first".to_string()
            });
        }
        Ok(mixdown::Range { start, end })
    }

    /// Builds the graph for an export or a bounce from the project as it
    /// is now, with every plugin given the state its live instance has.
    fn mixdown_render(&mut self, scope: mixdown::Scope) -> Result<mixdown::Render> {
        self.capture_plugin_states();
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        let (irs, designs, kits, states) = (
            &mut self.irs,
            &self.designs,
            &mut self.kits,
            &self.plugin_states,
        );
        let mut pedal = |p: &Pedal| -> Result<Box<dyn Processor>> {
            let mut processor = build_processor(irs, designs, &p.spec, CHANNELS)?;
            if let Some((spec, state)) = states.get(&p.id) {
                if *spec == p.spec {
                    processor
                        .set_state(state)
                        .map_err(|e| anyhow!("{}: saved state not restored: {e}", p.spec))?;
                }
            }
            for (param, value) in p.values.iter().enumerate() {
                processor.set_param(param, *value);
            }
            Ok(processor)
        };
        let mut kit = |id: &str| kits.get(id);
        mixdown::build(
            &self.project,
            scope,
            mixdown::Parts {
                audio_dir: &audio_dir,
                pedal: &mut pedal,
                kit: &mut kit,
            },
        )
    }

    fn latency_status(&self) -> LatencyStatus {
        let (compensation_frames, input_frames, output_frames, sample_rate) = match &self.live {
            Some(l) => {
                let info = l.stream.info();
                (
                    l.compensation,
                    info.input_latency_frames,
                    info.output_latency_frames,
                    info.sample_rate,
                )
            }
            None => (0, 0, 0, self.settings.audio.sample_rate),
        };
        let warn_ms = self.settings.latency_warn_ms;
        let ms = compensation_frames as f64 * 1000.0 / sample_rate;
        LatencyStatus {
            compensation_frames,
            input_frames,
            output_frames,
            warn_ms,
            warn: ms > warn_ms as f64,
        }
    }

    /// Sets the compensation above which the UI warns, in milliseconds.
    pub fn latency_warning_set(&mut self, ms: f32) -> EngineStatus {
        self.settings.latency_warn_ms = ms.max(0.0);
        self.persist();
        self.publish()
    }

    /// Turns the metronome on or off and sets its sound and level. It
    /// clicks on every beat of the tempo map while the transport plays.
    /// A setting, not part of the project, so it is not undoable.
    pub fn metronome_set(&mut self, metronome: MetronomeSettings) -> Result<EngineStatus> {
        if !cantina_engine::CLICK_SOUNDS.contains(&metronome.sound.as_str()) {
            bail!("the metronome has no sound called {}", metronome.sound);
        }
        self.settings.metronome = MetronomeSettings {
            level_db: metronome.level_db.clamp(-40.0, 6.0),
            ..metronome
        };
        if let Some(live) = &mut self.live {
            live.set_metronome(&self.settings.metronome);
        }
        self.persist();
        Ok(self.publish())
    }

    /// Sets what the player plays through. Rigs applied or auditioned
    /// from now on trim their input from it; chains already on tracks
    /// keep theirs. An app setting, not undoable.
    pub fn instrument_profile_set(
        &mut self,
        profile: crate::settings::InstrumentProfile,
    ) -> Result<EngineStatus> {
        if let Some(peak) = profile.calibrated_peak_db {
            if !(peak.is_finite() && (-90.0..=6.0).contains(&peak)) {
                bail!("a calibration of {peak} dBFS is not a level an input gives");
            }
        }
        if !(1..=12).contains(&profile.tuning.len()) || profile.tuning.iter().any(|n| *n > 127) {
            bail!("a tuning is one to twelve strings, as MIDI notes");
        }
        self.settings.instrument = profile;
        self.persist();
        Ok(self.publish())
    }

    /// Turns Record's count-in on or off. An app setting, not undoable.
    pub fn count_in_set(&mut self, on: bool) -> EngineStatus {
        self.settings.count_in = on;
        self.persist();
        self.publish()
    }

    /// Whether launch opens the last project rather than the start dialog.
    pub fn open_last_at_launch_set(&mut self, on: bool) -> EngineStatus {
        self.settings.open_last_at_launch = on;
        self.persist();
        self.publish()
    }

    // ----- the assistant -----

    /// The assistant's endpoint and model, and whether it is on.
    pub fn assistant_settings(&self) -> AssistantSettings {
        self.settings.assistant.clone()
    }

    /// Turns the assistant on or off and sets its endpoint and model. An
    /// app setting, not part of the project, so it is not undoable.
    pub fn assistant_configure(&mut self, assistant: AssistantSettings) -> EngineStatus {
        self.settings.assistant = AssistantSettings {
            endpoint: assistant.endpoint.trim().to_string(),
            model: assistant.model.trim().to_string(),
            ..assistant
        };
        self.persist();
        self.publish()
    }

    /// Undoes an edit the assistant made, if it is still the last thing
    /// that happened to the project, at `revision`.
    pub fn assistant_undo(&mut self, revision: u64) -> Result<EngineStatus> {
        if self.project.revision != revision {
            bail!("Something else has changed since. Use Edit ▸ Undo to step back through it.");
        }
        self.undo()
    }

    /// Saves the API key in the Keychain, or removes it when empty.
    pub fn assistant_set_key(&mut self, key: &str) -> Result<EngineStatus> {
        crate::assistant::keychain::set_key(key)?;
        Ok(self.publish())
    }

    // ----- plugins -----

    /// What the scanner found. Asking for the list before any scan has
    /// run starts one, so the first launch fills it in by itself.
    pub fn plugin_list(&mut self) -> PluginList {
        if self.plugins.is_unscanned() && !self.scanning {
            self.plugin_rescan(false);
        }
        self.plugins.clone()
    }

    /// Scans in the background: new and changed sources only, or every
    /// source when `full`. The child is this binary run with
    /// `--scan-plugins`, so a plugin that crashes on load is recorded as
    /// failed and the app stays up. The status is pushed when it is done.
    pub fn plugin_rescan(&mut self, full: bool) -> EngineStatus {
        if self.scanning {
            return self.status();
        }
        self.scanning = true;
        let previous = self.plugins.clone();
        let app = self.app.clone();
        thread::Builder::new()
            .name("plugin-scan".into())
            .spawn(move || {
                let result = std::env::current_exe()
                    .context("finding the Cantina binary")
                    .and_then(|exe| {
                        Scanner::new(exe, vec![scan::CHILD_FLAG.into()]).scan(
                            scan::discover(),
                            &previous,
                            full,
                            |_| {},
                        )
                    });
                app.state::<EngineState>()
                    .lock()
                    .plugins_scanned(result.map(|o| o.list));
            })
            .expect("spawning the plugin scan thread");
        self.publish()
    }

    /// The scan thread's result.
    fn plugins_scanned(&mut self, result: Result<PluginList>) {
        self.scanning = false;
        match result {
            Ok(list) => {
                if let Some(path) = &self.plugins_path {
                    if let Err(e) = list.save(path) {
                        self.last_error = Some(format!("{e:#}"));
                    }
                }
                self.plugins = list;
            }
            Err(e) => self.last_error = Some(format!("scanning plugins: {e:#}")),
        }
        self.publish();
    }

    fn project_status(&self) -> ProjectStatus {
        let p = &self.project;
        ProjectStatus {
            name: p.name.clone(),
            dir: self.project_dir.display().to_string(),
            dirty: self.dirty,
            undo_label: self.history.undo_label().map(str::to_string),
            redo_label: self.history.redo_label().map(str::to_string),
            recovery: self.recovery.as_ref().map(|entries| RecoveryInfo {
                edits: entries.len(),
                last: entries
                    .last()
                    .map(|e| e.command.label())
                    .unwrap_or_default(),
            }),
            sample_rate: p.sample_rate,
            tempo: p.tempo.clone(),
            tracks: p
                .tracks
                .iter()
                .map(|t| TrackStatus {
                    id: t.id.0,
                    name: t.name.clone(),
                    armed: t.armed,
                    recording: self.recording.iter().any(|r| r.track == t.id),
                    kit: t.kit.as_deref().map(|id| self.kit_status(id)),
                    chain: t
                        .chain
                        .iter()
                        .map(|p| {
                            let mut info = self.catalog.describe(p);
                            info.has_editor = self.live.as_ref().is_some_and(|l| {
                                l.tracks
                                    .iter()
                                    .flat_map(|t| t.pedals.iter())
                                    .find(|lp| lp.id == p.id)
                                    .and_then(|lp| lp.handle.as_ref())
                                    .is_some_and(|h| h.has_editor())
                            });
                            info.editor_open = self.open_editors.contains(&p.id);
                            info.latency_frames = self.pedal_latency(p.id);
                            info
                        })
                        .collect(),
                    clips: t
                        .clips
                        .iter()
                        .map(|c| ClipInfo {
                            id: c.id.0,
                            name: c.name.clone(),
                            file: c.file.clone(),
                            start: c.start,
                            offset: c.offset,
                            length: c.length,
                            tab: c.tab.as_ref().map(|tab| TabSummary {
                                instrument: tab.instrument,
                                tuning: tab.tuning.clone(),
                                capo: tab.capo,
                                notes: tab.notes.len(),
                                transcribed: !tab.heard.is_empty(),
                            }),
                        })
                        .collect(),
                    pattern_clips: t.pattern_clips.iter().map(PatternClipInfo::of).collect(),
                    strip: t.strip.clone(),
                    latency_frames: t.chain.iter().map(|p| self.pedal_latency(p.id)).sum(),
                    pads: t.pads.clone(),
                })
                .collect(),
            patterns: p.patterns.iter().map(PatternInfo::of).collect(),
            master: p.master.clone(),
            loop_region: p.loop_region,
        }
    }

    /// A kit as a track's status shows it. The pads come from the loaded
    /// kit when it is loaded, else from its file, so the grid has rows
    /// before the stream opens.
    fn kit_status(&self, id: &str) -> KitStatus {
        if let Some(loaded) = self.kits.loaded.get(id) {
            return KitStatus {
                id: id.to_string(),
                name: loaded.name.clone(),
                pads: loaded
                    .pads()
                    .iter()
                    .map(|p| PadInfo {
                        id: p.id.clone(),
                        name: p.name.clone(),
                    })
                    .collect(),
                missing: self.kits.problem(id).is_some(),
            };
        }
        match self.kits.entry(id) {
            Some(entry) => {
                let pads = std::fs::read_to_string(entry.path.join(kit::KIT_FILE))
                    .ok()
                    .and_then(|text| cantina_dsp::Kit::from_json(&text).ok())
                    .map(|k| {
                        k.pads
                            .into_iter()
                            .map(|p| PadInfo {
                                id: p.id,
                                name: p.name,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                KitStatus {
                    id: id.to_string(),
                    name: entry.name.clone(),
                    pads,
                    missing: false,
                }
            }
            None => KitStatus {
                id: id.to_string(),
                name: id.to_string(),
                pads: Vec::new(),
                missing: true,
            },
        }
    }

    fn pedal_latency(&self, id: PedalId) -> usize {
        self.live
            .as_ref()
            .and_then(|l| {
                l.tracks
                    .iter()
                    .flat_map(|t| t.pedals.iter())
                    .find(|p| p.id == id)
            })
            .map_or(0, |p| p.latency)
    }

    // ----- the project -----

    /// Applies an edit to the project, records it for undo and brings
    /// the graph in line.
    pub fn apply(&mut self, command: Command) -> Result<EngineStatus> {
        self.apply_quietly(command)?;
        Ok(self.publish())
    }

    /// Reverts the last edit.
    pub fn undo(&mut self) -> Result<EngineStatus> {
        if self
            .history
            .undo(&mut self.project)
            .map_err(|e| anyhow!("{e}"))?
            .is_some()
        {
            self.dirty = true;
            let command = self
                .history
                .last_undone()
                .cloned()
                .expect("an undone step is on the redo stack");
            self.journal_entry(&command);
            self.react(&command)?;
        }
        Ok(self.publish())
    }

    /// Reapplies the last undone edit.
    pub fn redo(&mut self) -> Result<EngineStatus> {
        if self
            .history
            .redo(&mut self.project)
            .map_err(|e| anyhow!("{e}"))?
            .is_some()
        {
            self.dirty = true;
            let command = self
                .history
                .last_redone()
                .cloned()
                .expect("a redone step is on the undo stack");
            self.journal_entry(&command);
            self.react(&command)?;
        }
        Ok(self.publish())
    }

    /// Records an applied command in the journal at the next revision.
    /// A journal that cannot be written is reported, not fatal: the edit
    /// is already in the project.
    fn journal_entry(&mut self, command: &Command) {
        self.project.revision += 1;
        if let Err(e) = self.journal.append(self.project.revision, command) {
            self.last_error = Some(format!("autosave: {e}"));
        }
    }

    /// Reads the journal of the open project and holds anything newer
    /// than the saved file for the user to recover or discard.
    fn read_journal(&mut self) {
        self.recovery = None;
        match self.journal.read() {
            Ok(read) => {
                if !read.warnings.is_empty() {
                    self.notice = Some(read.warnings.join("; "));
                    self.notice_seq = self.notice_seq.wrapping_add(1);
                }
                let pending = Journal::since(read.entries, self.project.revision);
                if !pending.is_empty() {
                    self.recovery = Some(pending);
                }
            }
            Err(e) => self.last_error = Some(format!("reading the journal: {e}")),
        }
    }

    /// Replays the journal's unsaved edits onto the project. They are not
    /// undoable afterwards, and stay in the journal until the next save.
    pub fn project_recover(&mut self) -> Result<EngineStatus> {
        let Some(entries) = self.recovery.take() else {
            return Ok(self.status());
        };
        let outcome = cantina_project::replay(&mut self.project, &entries);
        self.dirty = true;
        self.history.clear();
        self.sync()?;
        if let Some(live) = &mut self.live {
            live.set_tempo_map(tempo_map(&self.project.tempo));
        }
        outcome.map_err(|e| anyhow!("{e}"))?;
        Ok(self.publish())
    }

    /// Forgets the journal's unsaved edits.
    pub fn project_discard_recovery(&mut self) -> Result<EngineStatus> {
        self.recovery = None;
        self.journal.clear()?;
        Ok(self.publish())
    }

    /// Makes the live graph follow a command that was just applied.
    /// Parameter and arm changes go over the queue; everything else
    /// resyncs the structure.
    fn react(&mut self, command: &Command) -> Result<()> {
        // A jam is heard in place of the project; edits wait for it to close.
        if self.jam.is_some() {
            return Ok(());
        }
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        match command {
            Command::SetParam {
                pedal,
                param,
                value,
            } => {
                let node = live.pedal_node(*pedal)?;
                if !live.controller.set_param(node, *param, *value) {
                    bail!("the audio thread is not taking parameter changes");
                }
                Ok(())
            }
            Command::ArmTrack { id, armed } => {
                if let Source::Audio { mute, .. } = &live.track(*id)?.source {
                    let mute = *mute;
                    live.controller
                        .set_param(mute, 0, if *armed { 1.0 } else { 0.0 });
                }
                Ok(())
            }
            Command::RenameTrack { .. } => Ok(()),
            Command::SetTempo { tempo } => {
                live.set_tempo_map(tempo_map(tempo));
                // Tab notes sit on the timeline by sample: their ticks move.
                live.refresh_tab_voices(&self.project, self.practice)
            }
            Command::SetGain { .. }
            | Command::SetPan { .. }
            | Command::SetMute { .. }
            | Command::SetSolo { .. } => {
                live.apply_strips(&self.project);
                Ok(())
            }
            Command::SetPadMix { .. } => {
                live.apply_pad_mixes(&self.project, &mut self.kits);
                Ok(())
            }
            Command::InsertPattern { .. }
            | Command::RemovePattern { .. }
            | Command::RenamePattern { .. }
            | Command::SetPatternStep { .. }
            | Command::SetPatternLength { .. }
            | Command::SetPatternResolution { .. }
            | Command::SetPatternSwing { .. }
            | Command::ReplacePattern { .. }
            | Command::InsertPatternClip { .. }
            | Command::RemovePatternClip { .. }
            | Command::MovePatternClip { .. }
            | Command::TrimPatternClip { .. }
            | Command::SplitPatternClip { .. }
            | Command::SetPatternClipFill { .. } => {
                self.check_audition();
                self.refresh_scores()
            }
            // A tab is heard only through the voice.
            Command::SetClipTab { .. } | Command::EditTabNotes { .. } => {
                live.refresh_tab_voices(&self.project, self.practice)
            }
            Command::Batch { commands } if commands.iter().all(is_light) => {
                for c in commands {
                    self.react(c)?;
                }
                Ok(())
            }
            _ => self.sync(),
        }
    }

    /// Rebuilds the graph's structure from the project, with an
    /// auditioned rig's pedals in place of its track's chain.
    fn sync(&mut self) -> Result<()> {
        self.check_audition();
        let heard = self.heard_project();
        let scores = self.jam_scores();
        let voices = self.jam_voices();
        let jam_map = self.jam_tempo_map();
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        // The loop, when it is on, wraps the transport; a jam never loops.
        let looping = self
            .project
            .loop_region
            .filter(|r| r.on && jam_map.is_none())
            .map(|r| LoopRange {
                start: r.start,
                end: r.end,
            });
        if live.transport().loop_range() != looping {
            live.transport().set_loop(looping);
        }
        if let Some(map) = jam_map {
            if live.transport().tempo_map() != &map {
                live.set_tempo_map(map);
            }
        }
        let capture = self
            .jam
            .is_some()
            .then_some((crate::jam::INPUT_TRACK, self.settings.capture_minutes));
        live.set_capture(capture)?;
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        let mut removed = Vec::new();
        let result = live.sync(
            SyncContext {
                project: heard.as_ref().unwrap_or(&self.project),
                catalog: &mut self.catalog,
                kits: &mut self.kits,
                irs: &mut self.irs,
                designs: &self.designs,
                audition: &self.audition,
                rows: &self.drum_rows,
                audio_dir: &audio_dir,
                states: &self.plugin_states,
                practice: self.practice,
                scores: &scores,
                voices: &voices,
            },
            &mut removed,
        );
        self.close_editors(removed);
        result
    }

    /// Sends every drum track its score after a pattern edit.
    fn refresh_scores(&mut self) -> Result<()> {
        if self.jam.is_some() {
            return Ok(());
        }
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        live.refresh_scores(
            &self.project,
            &mut self.kits,
            &self.audition,
            &self.drum_rows,
        )
    }

    /// Drops an audition whose track or pattern is gone.
    fn check_audition(&mut self) {
        if let Some((track, what)) = &self.audition {
            let valid = self.project.track(*track).is_some_and(|t| t.is_drums())
                && match what {
                    Audition::Pattern(id) => self.project.pattern(*id).is_some(),
                    Audition::Groove { .. } => true,
                };
            if !valid {
                self.audition = None;
            }
        }
        if let Some(a) = &self.rig_audition {
            if self.project.track(a.track).is_none_or(|t| t.is_drums()) {
                self.rig_audition = None;
                self.plugin_states.retain(|id, _| id.0 < AUDITION_IDS);
            }
        }
    }

    // ----- rigs -----

    /// The project as it is heard: with an auditioned rig's pedals in
    /// place of its track's chain, while the rig is the one heard.
    fn heard_project(&self) -> Option<Project> {
        if self.jam.is_some() {
            return self.jam_project();
        }
        let a = self.rig_audition.as_ref().filter(|a| a.hearing_rig)?;
        let mut project = self.project.clone();
        project.track_mut(a.track)?.chain = a.chain.clone();
        Some(project)
    }

    /// Runs `f`, which changes what a track's chain is, with the track's
    /// strip faded out around it, so the swap never clicks: the old
    /// chain's sound, tails and all, fades to silence, the new graph goes
    /// in at a block boundary, and the strip fades back to where the
    /// project has it.
    fn with_track_faded<T>(
        &mut self,
        track: TrackId,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let faded = self.live.as_mut().is_some_and(|live| {
            live.tracks
                .iter()
                .find(|t| t.id == track)
                .map(|t| t.strip)
                .is_some_and(|strip| live.controller.set_param(strip, MUTE, 1.0))
        });
        if faded {
            std::thread::sleep(SWAP_FADE);
        }
        let result = f(self);
        let heard = self.heard_project();
        if let Some(live) = &mut self.live {
            if faded {
                std::thread::sleep(SWAP_SETTLE);
            }
            live.apply_strips(heard.as_ref().unwrap_or(&self.project));
        }
        result
    }

    /// The audio track `track`, refusing a drum track.
    fn rig_track(&self, track: TrackId) -> Result<&Track> {
        let t = self
            .project
            .track(track)
            .with_context(|| format!("no track {track}"))?;
        if t.is_drums() {
            bail!("{} is a drum track; a rig goes on an audio track", t.name);
        }
        Ok(t)
    }

    /// The pedal format's definition of a spec, for its parameters' ids.
    fn definition_of(&self, spec: &str) -> Option<PedalDef> {
        if let Some(id) = spec.strip_prefix("pedal:") {
            return self.designs.get(id).cloned();
        }
        cantina_dsp::stock_definition(spec).cloned()
    }

    /// A rig's pedals as a chain's, ids from `first`: an input gain pedal
    /// first when the rig's input gain and the instrument profile's trim
    /// add up to any, the values its parameter names
    /// resolve to, the plugins' states to restore, and a line for each
    /// pedal left out because it is not installed. A name or value that
    /// is wrong fails the whole rig.
    #[allow(clippy::type_complexity)]
    fn rig_chain(
        &mut self,
        rig: &cantina_project::Rig,
        first: u64,
    ) -> Result<(Vec<Pedal>, Vec<(PedalId, String, Vec<u8>)>, Vec<String>)> {
        let mut pedals = Vec::new();
        let mut states = Vec::new();
        let mut missing = Vec::new();
        let mut problems = Vec::new();
        let mut next = first;
        let input = rig.input_gain_db + self.settings.instrument.trim_db();
        if input.abs() >= 0.05 {
            pedals.push(Pedal {
                id: PedalId(next),
                spec: "gain".into(),
                values: vec![input.clamp(-60.0, 24.0)],
                bypassed: false,
            });
            next += 1;
        }
        for (i, rp) in rig.chain.iter().enumerate() {
            let at = format!("pedal {} ({})", i + 1, rp.pedal);
            let spec = rp.pedal.as_str();
            let absent = if let Some(id) = spec.strip_prefix("cab:") {
                self.irs
                    .entry(id)
                    .is_none()
                    .then(|| format!("the impulse response {id:?} is not installed"))
            } else if let Some(id) = spec.strip_prefix("pedal:") {
                (!self.designs.contains_key(id) && self.library.entry(id).is_none())
                    .then(|| format!("the designed pedal {id:?} is not in your library"))
            } else {
                None
            };
            if let Some(why) = absent {
                missing.push(format!("{at}: {why}"));
                continue;
            }
            let id = PedalId(next);
            let base = match self.new_pedal(id, spec, None) {
                Ok(p) => p,
                Err(e) if rp.is_plugin() => {
                    missing.push(format!("{at}: the plugin is not installed ({e:#})"));
                    continue;
                }
                Err(e) => {
                    problems.push(format!("{at}: {e:#}"));
                    continue;
                }
            };
            let params = self
                .catalog
                .get(&base.spec, &mut self.irs, &self.designs)?
                .params
                .clone();
            let mut slots = cantina_cli::rig::slots_for(&params, self.definition_of(spec).as_ref());
            let state = rp.state_bytes();
            if let Some(state) = &state {
                // The state sets the plugin's parameters; `params` only
                // change the ones they name.
                let mut p = build_processor(&mut self.irs, &self.designs, spec, CHANNELS)?;
                if let Err(e) = p.set_state(state) {
                    missing.push(format!("{at}: the plugin refused its saved state ({e})"));
                    continue;
                }
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.default = p.param(i);
                }
                states.push((id, base.spec.clone(), state.clone()));
            }
            match cantina_project::resolve_values(rp, &slots) {
                Ok(values) => pedals.push(Pedal {
                    id,
                    spec: base.spec,
                    values,
                    bypassed: rp.bypassed,
                }),
                Err(p) => problems.extend(p.into_iter().map(|p| format!("{at}: {p}"))),
            }
            next += 1;
        }
        if !problems.is_empty() {
            bail!("{}: {}", rig.name, problems.join("; "));
        }
        Ok((pedals, states, missing))
    }

    /// Hears the rig at `path` on an audio track's live input in place
    /// of its chain, without changing the project; both `None` stops and
    /// puts the chain back. Starting another rig, or the same one on
    /// another track, replaces the audition.
    pub fn rig_audition(
        &mut self,
        track: Option<u64>,
        path: Option<String>,
    ) -> Result<EngineStatus> {
        let (Some(t), Some(path)) = (track, path) else {
            if let Some(a) = self.rig_audition.take() {
                self.plugin_states.retain(|id, _| id.0 < AUDITION_IDS);
                self.with_track_faded(a.track, |e| e.sync())?;
            }
            return Ok(self.publish());
        };
        let track = TrackId(t);
        self.rig_track(track)?;
        let rig = crate::rigs::read(Path::new(&path))?;
        let (chain, states, missing) = self.rig_chain(&rig, AUDITION_IDS)?;
        if let Some(old) = self.rig_audition.take() {
            self.plugin_states.retain(|id, _| id.0 < AUDITION_IDS);
            if old.track != track {
                self.with_track_faded(old.track, |e| e.sync())?;
            }
        }
        // The chain's plugins come back as they were when B is heard.
        self.capture_plugin_states();
        for (id, spec, state) in states {
            self.plugin_states.insert(id, (spec, state));
        }
        self.rig_audition = Some(RigAudition {
            track,
            path,
            chain,
            hearing_rig: true,
            missing,
        });
        self.with_track_faded(track, |e| e.sync())?;
        Ok(self.publish())
    }

    /// While auditioning, hears the rig (`true`, A) or the track's own
    /// chain (B), swapping between them without a click.
    pub fn rig_ab(&mut self, hear_rig: bool) -> Result<EngineStatus> {
        let a = self
            .rig_audition
            .as_mut()
            .context("no rig is being auditioned")?;
        if a.hearing_rig != hear_rig {
            a.hearing_rig = hear_rig;
            let track = a.track;
            self.capture_plugin_states();
            self.with_track_faded(track, |e| e.sync())?;
        }
        Ok(self.publish())
    }

    /// Replaces an audio track's chain with the rig at `path`, as one
    /// undoable edit, and ends any audition. Pedals that are not
    /// installed are left out and named in a notice.
    pub fn rig_apply(&mut self, track: u64, path: &str) -> Result<EngineStatus> {
        let track = TrackId(track);
        let existing: Vec<PedalId> = self.rig_track(track)?.chain.iter().map(|p| p.id).collect();
        let rig = crate::rigs::read(Path::new(path))?;
        let (pedals, states, missing) = self.rig_chain(&rig, self.project.fresh_id())?;
        let mut commands: Vec<Command> = existing
            .into_iter()
            .map(|id| Command::RemovePedal { id })
            .collect();
        commands.extend(pedals.into_iter().map(|pedal| Command::InsertPedal {
            track,
            index: usize::MAX,
            pedal,
        }));
        self.rig_audition = None;
        self.plugin_states.retain(|id, _| id.0 < AUDITION_IDS);
        for (id, spec, state) in states {
            self.plugin_states.insert(id, (spec, state));
        }
        self.with_track_faded(track, |e| e.apply_quietly(Command::Batch { commands }))?;
        if !missing.is_empty() {
            self.notice = Some(format!(
                "{} is missing pedals: {}",
                rig.name,
                missing.join("; ")
            ));
        }
        Ok(self.publish())
    }

    /// Writes an audio track's chain to the Rigs library folder as a rig
    /// called `draft.name`: every parameter by name and, for a plugin,
    /// its state. A Gain pedal first in the chain, as applying a rig puts
    /// there, is the rig's input gain less the instrument profile's trim,
    /// so the rig applies the same again. Refuses to overwrite a rig of
    /// that name unless asked.
    pub fn rig_save(&mut self, track: u64, draft: RigDraft) -> Result<EngineStatus> {
        let name = draft.name.trim().to_string();
        if name.is_empty() {
            bail!("a rig needs a name");
        }
        let t = self.rig_track(TrackId(track))?.clone();
        if t.chain.is_empty() {
            bail!("{} has no pedals to save", t.name);
        }
        self.capture_plugin_states();
        let trim = self.settings.instrument.trim_db();
        let (input_gain_db, pedals) = match t.chain.split_first() {
            Some((first, rest)) if first.spec == "gain" && !first.bypassed => {
                let gain = first.values.first().copied().unwrap_or(0.0) - trim;
                let max = cantina_project::MAX_INPUT_GAIN_DB;
                (((gain * 10.0).round() / 10.0).clamp(-max, max), rest)
            }
            _ => (0.0, &t.chain[..]),
        };
        let mut chain = Vec::new();
        let mut local = Vec::new();
        for p in pedals {
            let params = self
                .catalog
                .get(&p.spec, &mut self.irs, &self.designs)?
                .params
                .clone();
            let slots = cantina_cli::rig::slots_for(&params, self.definition_of(&p.spec).as_ref());
            let plugin = p.spec.starts_with("au:") || p.spec.starts_with("clap:");
            let state = plugin
                .then(|| self.plugin_states.get(&p.id))
                .flatten()
                .filter(|(spec, _)| *spec == p.spec)
                .map(|(_, s)| s.clone());
            chain.push(cantina_project::RigPedal::from_values(
                &p.spec,
                &p.values,
                &slots,
                state.as_deref(),
                p.bypassed,
            ));
            if let Some(id) = p.spec.strip_prefix("pedal:") {
                if self.library.entry(id).is_none() {
                    local.push(id.to_string());
                }
            }
        }
        let rig = cantina_project::Rig {
            version: cantina_project::RIG_VERSION,
            name: name.clone(),
            genre: draft.genre.trim().to_string(),
            description: draft.description.trim().to_string(),
            instrument: draft.instrument,
            input_gain_db,
            chain,
        };
        rig.validate().map_err(|e| anyhow!("{e}"))?;
        let path = crate::rigs::path_for(&self.folders.rigs, &name);
        if path.exists() && !draft.replace {
            bail!("there is already a rig called {name}");
        }
        crate::rigs::write(&path, &rig)?;
        self.library_status = self.folders.scan();
        self.notice = Some(if local.is_empty() {
            format!("Saved the rig {name}")
        } else {
            format!(
                "Saved the rig {name}. It uses {}, which only this project has: save {} to your \
                 library so the rig loads elsewhere",
                local.join(", "),
                if local.len() == 1 { "it" } else { "them" }
            )
        });
        Ok(self.publish())
    }

    /// Renames one of the user's rigs, and its file to match.
    pub fn rig_rename(&mut self, path: &str, name: &str) -> Result<EngineStatus> {
        let to = crate::rigs::rename(&self.folders.rigs, path, name)?;
        if let Some(a) = &mut self.rig_audition {
            if a.path == path {
                a.path = to.display().to_string();
            }
        }
        self.library_status = self.folders.scan();
        Ok(self.publish())
    }

    /// Deletes one of the user's rigs.
    pub fn rig_delete(&mut self, path: &str) -> Result<EngineStatus> {
        crate::rigs::delete(&self.folders.rigs, path)?;
        self.library_status = self.folders.scan();
        Ok(self.publish())
    }

    /// Copies a rig file into the Rigs library folder after checking it,
    /// numbered when its name is taken.
    pub fn rig_import(&mut self, path: &str) -> Result<EngineStatus> {
        let to = crate::rigs::import(&self.folders.rigs, Path::new(path))?;
        self.library_status = self.folders.scan();
        self.notice = Some(format!(
            "Added the rig {} to your rigs",
            to.file_name().unwrap_or_default().to_string_lossy()
        ));
        Ok(self.publish())
    }

    /// Writes a copy of the rig at `path` to `to`, for sharing.
    pub fn rig_export(&self, path: &str, to: &str) -> Result<()> {
        let rig = crate::rigs::read(Path::new(path))?;
        crate::rigs::write(Path::new(to), &rig)
    }

    // ----- drums -----

    /// Adds a drum track playing `kit`, or the first installed kit.
    pub fn track_add_drums(&mut self, kit: Option<String>) -> Result<EngineStatus> {
        let kit = match kit {
            Some(k) => k,
            None => self
                .kits
                .entries()
                .first()
                .map(|k| k.id.clone())
                .context("no drum kits are installed")?,
        };
        if self.kits.entry(&kit).is_none() {
            bail!("no kit {kit:?} is installed");
        }
        let id = TrackId(self.project.fresh_id());
        let drums = self.project.tracks.iter().filter(|t| t.is_drums()).count();
        let name = if drums == 0 {
            "Drums".to_string()
        } else {
            format!("Drums {}", drums + 1)
        };
        self.apply(Command::InsertTrack {
            index: usize::MAX,
            track: Track::drums(id, name, kit),
        })
    }

    /// Changes a drum track's kit.
    /// Points the stage's analysis at a track, or at the master with
    /// none. A track that is later removed leaves it on the master.
    pub fn analysis_set_source(&mut self, track: Option<u64>) -> Result<EngineStatus> {
        let track = track.map(TrackId);
        if let Some(id) = track {
            self.project
                .track(id)
                .with_context(|| format!("no track {}", id.0))?;
        }
        self.analysis_source = track;
        if let Some(live) = &mut self.live {
            live.analysis_source = track;
            live.wire()?;
        }
        Ok(self.publish())
    }

    pub fn track_set_kit(&mut self, id: u64, kit: String) -> Result<EngineStatus> {
        if self.kits.entry(&kit).is_none() {
            bail!("no kit {kit:?} is installed");
        }
        self.apply(Command::SetTrackKit {
            id: TrackId(id),
            kit,
        })
    }

    /// Looks for impulse responses again, for one dropped in while the
    /// app runs.
    pub fn irs_rescan(&mut self) -> EngineStatus {
        self.irs.rescan();
        self.publish()
    }

    /// Swaps what a pedal is. Only a cabinet's impulse (`cab:<id>`) can
    /// change this way, and the impulse must be installed.
    pub fn chain_set_spec(&mut self, id: u64, spec: &str) -> Result<EngineStatus> {
        let Some(ir_id) = spec.strip_prefix("cab:") else {
            bail!("only a cabinet's impulse response can be swapped");
        };
        if self.irs.entry(ir_id).is_none() {
            bail!("no impulse response {ir_id:?} is installed");
        }
        let Some((_, pedal)) = self.project.pedal(PedalId(id)) else {
            bail!("no pedal {id}");
        };
        if !(pedal.spec == "cab" || pedal.spec.starts_with("cab:")) {
            bail!("{} is not a cabinet", pedal.spec);
        }
        self.apply(Command::SetPedalSpec {
            id: PedalId(id),
            spec: spec.to_string(),
        })
    }

    /// Looks for kits again, for one installed while the app runs.
    pub fn kits_rescan(&mut self) -> EngineStatus {
        let mut problems = Vec::new();
        self.kits.rescan(&mut problems);
        if !problems.is_empty() {
            self.last_error = Some(format!("kits: {}", problems.join("; ")));
        }
        self.publish()
    }

    /// Adds an empty one-bar pattern of sixteenths.
    pub fn pattern_add(&mut self, name: Option<String>) -> Result<EngineStatus> {
        let id = PatternId(self.project.fresh_id());
        let name = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Pattern {}", self.project.patterns.len() + 1));
        self.apply(Command::InsertPattern {
            index: usize::MAX,
            pattern: Pattern::new(id, name),
        })
    }

    /// Plays a pad of a drum track now, for playing the kit live. Not an
    /// edit: recording what is played is the UI's, one pattern edit a pass.
    pub fn pad_hit(&mut self, track: u64, pad: &str, velocity: f32) -> Result<()> {
        let Some(live) = &mut self.live else {
            bail!("start the engine to play the pads");
        };
        if !live.pad_hit(TrackId(track), pad, velocity, &mut self.kits) {
            bail!("pad {pad:?} is not on that track's kit");
        }
        Ok(())
    }

    /// Sets a pad's mix on a drum track, or with `None` puts it back to
    /// how the kit has it. One undoable edit.
    pub fn pad_mix_set(
        &mut self,
        track: u64,
        pad: String,
        mix: Option<cantina_project::PadMix>,
    ) -> Result<EngineStatus> {
        self.apply(Command::SetPadMix {
            track: TrackId(track),
            pad,
            mix,
        })
    }

    /// Sets, or with `None` removes, a pattern clip's fill.
    pub fn pattern_clip_set_fill(
        &mut self,
        id: u64,
        fill: Option<cantina_project::Fill>,
    ) -> Result<EngineStatus> {
        self.apply(Command::SetPatternClipFill {
            id: PatternClipId(id),
            fill,
        })
    }

    /// Writes a fill for a pattern clip from its own pattern, on the
    /// track's kit, as a new pattern, and has the clip play it (every
    /// four bars and at its end, unless it already had a fill to follow):
    /// one undoable edit. The same seed writes the same fill.
    pub fn pattern_clip_generate_fill(&mut self, id: u64, seed: u64) -> Result<EngineStatus> {
        let clip_id = PatternClipId(id);
        let (track, clip) = self
            .project
            .tracks
            .iter()
            .find_map(|t| {
                t.pattern_clips
                    .iter()
                    .find(|c| c.id == clip_id)
                    .map(|c| (t, c))
            })
            .with_context(|| format!("no pattern clip {id}"))?;
        let groove = self
            .project
            .pattern(clip.pattern)
            .with_context(|| format!("no pattern {}", clip.pattern))?;
        let kit = self.kits.get(track.kit.as_deref().unwrap_or_default());
        let pads: Vec<&str> = kit.pads().iter().map(|p| p.id.as_str()).collect();
        let tempo = &self.project.tempo;
        let beats = u32::from(tempo.numerator.max(1)) * 4 / u32::from(tempo.denominator.max(1));
        let fill = cantina_project::generate_fill(groove, beats.max(1), &pads, seed);
        let taken: Vec<&str> = self
            .project
            .patterns
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        let fill = cantina_project::imported(fill, PatternId(self.project.fresh_id()), &taken);
        let settings = match &clip.fill {
            Some(f) => cantina_project::Fill {
                pattern: fill.id,
                ..f.clone()
            },
            None => cantina_project::Fill {
                pattern: fill.id,
                every: 4,
                at_end: true,
            },
        };
        self.apply(Command::Batch {
            commands: vec![
                Command::InsertPattern {
                    index: usize::MAX,
                    pattern: fill,
                },
                Command::SetPatternClipFill {
                    id: clip_id,
                    fill: Some(settings),
                },
            ],
        })
    }

    pub fn pattern_remove(&mut self, id: u64) -> Result<EngineStatus> {
        self.apply(Command::RemovePattern { id: PatternId(id) })
    }

    pub fn pattern_rename(&mut self, id: u64, name: String) -> Result<EngineStatus> {
        let name = name.trim().to_string();
        if name.is_empty() {
            bail!("a pattern needs a name");
        }
        self.apply(Command::RenamePattern {
            id: PatternId(id),
            name,
        })
    }

    /// Sets one cell of a pattern: a velocity from 0 (clear) to 1.
    pub fn pattern_set_step(
        &mut self,
        id: u64,
        pad: String,
        step: usize,
        velocity: f32,
    ) -> Result<EngineStatus> {
        self.apply(Command::SetPatternStep {
            pattern: PatternId(id),
            pad,
            step,
            velocity: velocity.clamp(0.0, 1.0),
        })
    }

    pub fn pattern_set_length(&mut self, id: u64, length: u16) -> Result<EngineStatus> {
        self.apply(Command::SetPatternLength {
            id: PatternId(id),
            length,
        })
    }

    pub fn pattern_set_resolution(&mut self, id: u64, steps_per_beat: u8) -> Result<EngineStatus> {
        self.apply(Command::SetPatternResolution {
            id: PatternId(id),
            steps_per_beat,
        })
    }

    /// Sets a pattern's swing. Not published: the slider is continuous
    /// and the UI shows what it sent.
    pub fn pattern_set_swing(&mut self, id: u64, swing: f32) -> Result<()> {
        self.apply_quietly(Command::SetPatternSwing {
            id: PatternId(id),
            swing: swing.clamp(0.0, 1.0),
        })
    }

    // ----- pattern clips -----

    /// Puts a pattern on a drum track at `start` ticks, for `length`
    /// ticks or one pass of the pattern.
    pub fn pattern_clip_add(
        &mut self,
        track: u64,
        pattern: u64,
        start: u64,
        length: Option<u64>,
    ) -> Result<EngineStatus> {
        let p = self
            .project
            .pattern(PatternId(pattern))
            .with_context(|| format!("no pattern {pattern}"))?;
        let length = length.unwrap_or_else(|| p.ticks());
        let clip = PatternClip {
            id: PatternClipId(self.project.fresh_id()),
            pattern: PatternId(pattern),
            start,
            offset: 0,
            length,
            fill: None,
        };
        self.apply(Command::InsertPatternClip {
            track: TrackId(track),
            clip,
        })
    }

    pub fn pattern_clip_remove(&mut self, id: u64) -> Result<EngineStatus> {
        self.apply(Command::RemovePatternClip {
            id: PatternClipId(id),
        })
    }

    pub fn pattern_clip_move(&mut self, id: u64, track: u64, start: u64) -> Result<EngineStatus> {
        self.apply(Command::MovePatternClip {
            id: PatternClipId(id),
            track: TrackId(track),
            start,
        })
    }

    pub fn pattern_clip_trim(
        &mut self,
        id: u64,
        start: u64,
        offset: u64,
        length: u64,
    ) -> Result<EngineStatus> {
        self.apply(Command::TrimPatternClip {
            id: PatternClipId(id),
            start,
            offset,
            length,
        })
    }

    /// Copies a pattern clip to the first space after it on its track
    /// that holds it.
    pub fn pattern_clip_duplicate(&mut self, id: u64) -> Result<EngineStatus> {
        let id = PatternClipId(id);
        let (track, clip) = self
            .project
            .pattern_clip(id)
            .with_context(|| format!("no pattern clip {id}"))?;
        let start = track.free_pattern_start(clip.end(), clip.length);
        let command = Command::InsertPatternClip {
            track: track.id,
            clip: clip.duplicate(PatternClipId(self.project.fresh_id()), start),
        };
        self.apply(command)
    }

    pub fn pattern_clip_split(&mut self, id: u64, at: u64) -> Result<EngineStatus> {
        let new_id = PatternClipId(self.project.fresh_id());
        self.apply(Command::SplitPatternClip {
            id: PatternClipId(id),
            at,
            new_id,
        })
    }

    /// Loops `pattern` on `track` from the start while the transport
    /// plays, for editing it by ear; `None` stops. Not an edit: nothing
    /// is recorded or saved.
    pub fn pattern_audition(
        &mut self,
        track: Option<u64>,
        pattern: Option<u64>,
    ) -> Result<EngineStatus> {
        self.audition = match (track, pattern) {
            (Some(t), Some(p)) => {
                let track = self
                    .project
                    .track(TrackId(t))
                    .with_context(|| format!("no track {t}"))?;
                if !track.is_drums() {
                    bail!("{} is not a drum track", track.name);
                }
                self.project
                    .pattern(PatternId(p))
                    .with_context(|| format!("no pattern {p}"))?;
                Some((TrackId(t), Audition::Pattern(PatternId(p))))
            }
            _ => None,
        };
        self.refresh_scores()?;
        Ok(self.publish())
    }

    /// Loops a stock groove (or any pattern file) on `track` to hear it
    /// before adding it, at the project's tempo through the track's kit;
    /// `None` stops. While playing it starts on the next bar line, and
    /// from a stop it starts at once, where the playhead is. Not an edit.
    pub fn groove_audition(
        &mut self,
        track: Option<u64>,
        path: Option<String>,
    ) -> Result<EngineStatus> {
        let (Some(t), Some(path)) = (track, path) else {
            if matches!(self.audition, Some((_, Audition::Groove { .. }))) {
                self.audition = None;
                self.refresh_scores()?;
            }
            return Ok(self.publish());
        };
        let track = self
            .project
            .track(TrackId(t))
            .with_context(|| format!("no track {t}"))?;
        if !track.is_drums() {
            bail!("{} is not a drum track", track.name);
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
        let (pattern, _) =
            cantina_project::groove_from_json(&text).map_err(|e| anyhow!("{path}: {e}"))?;
        let tempo = tempo_map(&self.project.tempo);
        let (playing, position, rate) = match &mut self.live {
            Some(live) => (
                live.transport().is_playing(),
                live.transport().position(),
                live.stream.info().sample_rate,
            ),
            None => (false, 0, self.project.sample_rate),
        };
        let tick = tempo.samples_to_ticks(position, rate);
        let signature = &self.project.tempo;
        let bar = u64::from(cantina_project::PPQ) * 4 / u64::from(signature.denominator.max(1))
            * u64::from(signature.numerator.max(1));
        let from = if playing {
            ((tick / bar as f64).floor() as u64 + 1) * bar
        } else {
            tick.round() as u64
        };
        self.audition = Some((
            TrackId(t),
            Audition::Groove {
                path,
                pattern: Arc::new(pattern),
                from,
            },
        ));
        self.refresh_scores()?;
        if !playing {
            if let Some(live) = &mut self.live {
                live.transport().play();
            }
        }
        Ok(self.publish())
    }

    /// Replaces a pattern whole, as one undoable edit: what every gesture
    /// of the grid (painting, pasting, shifting, doubling) sends.
    pub fn pattern_replace(&mut self, pattern: Pattern) -> Result<EngineStatus> {
        self.apply(Command::ReplacePattern { pattern })
    }

    /// Mutes and solos rows of a drum track's patterns, for listening
    /// while editing. Not an edit: nothing is recorded, saved or
    /// exported differently.
    pub fn drum_rows_set(&mut self, track: u64, rows: RowMix) -> Result<EngineStatus> {
        let id = TrackId(track);
        let t = self
            .project
            .track(id)
            .with_context(|| format!("no track {track}"))?;
        if !t.is_drums() {
            bail!("{} is not a drum track", t.name);
        }
        if rows.muted.is_empty() && rows.soloed.is_empty() {
            self.drum_rows.remove(&id);
        } else {
            self.drum_rows.insert(id, rows);
        }
        self.refresh_scores()?;
        Ok(self.publish())
    }

    /// Sets how to practice: hear the tabs, mute the recordings under
    /// them, and the speed from 50% to 100%. Not an edit: nothing is
    /// recorded or saved.
    pub fn practice_set(&mut self, practice: Practice) -> Result<EngineStatus> {
        if !(0.5..=1.0).contains(&practice.speed) {
            bail!(
                "a practice speed is 50% to 100%, not {:.0}%",
                practice.speed * 100.0
            );
        }
        if practice.speed < 1.0 && !self.recording.is_empty() {
            bail!("stop recording before slowing down");
        }
        self.practice = practice;
        if let Some(live) = &mut self.live {
            live.transport().set_rate(practice.speed);
        }
        self.sync()?;
        Ok(self.publish())
    }

    // ----- editor windows -----

    /// Runs `f` with the engine locked on the main thread, where AppKit
    /// lives. Queued on the event loop, so it runs after the current
    /// command returns and never deadlocks on the engine's lock.
    fn on_main_thread(&self, f: impl FnOnce(&mut Engine, MainThreadMarker) + Send + 'static) {
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            if let Some(state) = app.try_state::<EngineState>() {
                f(&mut state.lock(), mtm);
            }
        });
    }

    /// Opens a plugin's editor window, or brings it to the front. The
    /// window appears once the main thread gets to it; the status push
    /// then says so.
    pub fn editor_open(&mut self, id: u64) -> EngineStatus {
        self.on_main_thread(move |engine, mtm| engine.open_editor_now(PedalId(id), mtm));
        self.status()
    }

    pub fn editor_close(&mut self, id: u64) -> EngineStatus {
        self.on_main_thread(move |engine, mtm| engine.close_editor_now(PedalId(id), mtm));
        self.status()
    }

    fn close_editors(&self, ids: Vec<PedalId>) {
        let ids: Vec<PedalId> = ids
            .into_iter()
            .filter(|id| self.open_editors.contains(id))
            .collect();
        if ids.is_empty() {
            return;
        }
        self.on_main_thread(move |engine, mtm| {
            for id in ids {
                engine.close_editor_now(id, mtm);
            }
        });
    }

    fn open_editor_now(&mut self, id: PedalId, mtm: MainThreadMarker) {
        if let Some(editor) = self.editors.get_mut(mtm).get(&id) {
            editor.focus();
            return;
        }
        let Some((track, pedal)) = self.project.pedal(id) else {
            self.last_error = Some(format!("no pedal {id}"));
            self.publish();
            return;
        };
        let spec = pedal.spec.clone();
        let title = format!("{} · {}", self.catalog.describe(pedal).name, track.name);
        let parent: Option<NonNull<c_void>> = self
            .app
            .get_webview_window("main")
            .and_then(|w| w.ns_window().ok())
            .and_then(NonNull::new);
        let origin = self.settings.editor_positions.get(&spec).copied();
        let Some(live) = &mut self.live else {
            self.last_error = Some("start the engine before opening an editor".into());
            self.publish();
            return;
        };
        let handle = live
            .pedals_mut()
            .find(|p| p.id == id)
            .and_then(|p| p.handle.as_mut());
        let result = match handle {
            None => Err(anyhow!("this pedal has no editor")),
            Some(handle) => {
                let any = handle.as_any();
                if let Some(au) = any.downcast_ref::<AudioUnitHandle>() {
                    Editor::open_au(mtm, au, parent, origin, &title)
                } else if let Some(clap) = any.downcast_ref::<ClapHandle>() {
                    Editor::open_clap(mtm, clap, parent, origin, &title)
                } else {
                    Err(anyhow!("this pedal has no editor"))
                }
            }
        };
        match result {
            Ok(editor) => {
                self.editors.get_mut(mtm).insert(id, editor);
                self.open_editors.insert(id);
            }
            Err(e) => self.last_error = Some(format!("{e:#}")),
        }
        self.publish();
    }

    fn close_editor_now(&mut self, id: PedalId, mtm: MainThreadMarker) {
        let Some(editor) = self.editors.get_mut(mtm).remove(&id) else {
            return;
        };
        if let Some((_, pedal)) = self.project.pedal(id) {
            let spec = pedal.spec.clone();
            self.settings.editor_positions.insert(spec, editor.origin());
            self.persist();
        }
        editor.close(mtm);
        self.open_editors.remove(&id);
        self.publish();
    }

    /// Lets every open editor follow its plugin, and forgets the ones
    /// the user closed. Main thread, a few times a second.
    pub fn tick_editors(&mut self, mtm: MainThreadMarker) {
        let closed: Vec<PedalId> = self
            .editors
            .get_mut(mtm)
            .iter_mut()
            .filter_map(|(id, editor)| (!editor.tick(mtm)).then_some(*id))
            .collect();
        for id in closed {
            self.close_editor_now(id, mtm);
        }
    }

    // ----- plugin state -----

    /// Reads every running plugin's state into `plugin_states`.
    fn capture_plugin_states(&mut self) {
        let Some(live) = &mut self.live else {
            return;
        };
        for pedal in live.pedals_mut() {
            let Some(handle) = pedal.handle.as_mut() else {
                continue;
            };
            let Some((_, p)) = self.project.pedal(pedal.id) else {
                continue;
            };
            let state = handle.state();
            if !state.is_empty() {
                self.plugin_states.insert(pedal.id, (p.spec.clone(), state));
            }
        }
    }

    /// Writes `plugins/<pedal id>.state` for every pedal in the project
    /// that has state. Files for pedals that are gone stay, so undoing a
    /// removal finds the state again.
    fn write_plugin_states(&self) -> Result<()> {
        let dir = self.project_dir.join(PLUGINS_DIR);
        for track in &self.project.tracks {
            for pedal in &track.chain {
                let Some((spec, state)) = self.plugin_states.get(&pedal.id) else {
                    continue;
                };
                if *spec != pedal.spec {
                    continue;
                }
                std::fs::create_dir_all(&dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
                let path = state_path(&dir, pedal.id);
                std::fs::write(&path, encode_state(spec, state))
                    .with_context(|| format!("writing {}", path.display()))?;
            }
        }
        Ok(())
    }

    /// Takes the parameter changes the plugins made on their own (an
    /// editor, a preset) and records them as edits, so the generic UI
    /// and undo follow. Called from the meter pump a few times a second.
    pub fn poll_plugins(&mut self) {
        if !self.open_editors.is_empty() {
            self.on_main_thread(|engine, mtm| engine.tick_editors(mtm));
        }
        let Some(live) = &mut self.live else {
            return;
        };
        let mut commands = Vec::new();
        for pedal in live.pedals_mut() {
            let Some(handle) = pedal.handle.as_mut() else {
                continue;
            };
            for (param, value) in handle.take_changes() {
                let known = self
                    .project
                    .pedal(pedal.id)
                    .and_then(|(_, p)| p.values.get(param).copied());
                if known.is_some_and(|k| (k - value).abs() <= f32::EPSILON * k.abs().max(1.0)) {
                    continue;
                }
                commands.push(Command::SetParam {
                    pedal: pedal.id,
                    param,
                    value,
                });
            }
        }
        let latency_changed = live.refresh_latencies();
        if commands.is_empty() && !latency_changed {
            return;
        }
        for command in commands {
            if let Err(e) = self.apply_quietly(command) {
                self.last_error = Some(format!("{e:#}"));
            }
        }
        self.publish();
    }

    /// The projects in the projects folder, by name.
    pub fn project_list(&self) -> Result<Vec<String>> {
        self.projects.list()
    }

    /// Creates a project with one track and the default chain, saves it
    /// and opens it.
    pub fn project_new(&mut self, name: &str) -> Result<EngineStatus> {
        let (project, dir) = self
            .projects
            .create(name, self.settings.audio.sample_rate)?;
        self.switch_to(project, dir)?;
        Ok(self.publish())
    }

    /// Creates a project from the start dialog's options, saves it and
    /// opens it. Its rate, tempo and tracks are in the file it starts
    /// from, so it opens with nothing to undo. A drum track gets the
    /// first kit installed.
    pub fn project_new_with(&mut self, name: &str, options: &NewProject) -> Result<EngineStatus> {
        if !self.recording.is_empty() {
            bail!("stop recording before starting a new project");
        }
        let kit = self.kits.entries().first().map(|k| k.id.clone());
        let (project, dir) = self.projects.create_with(
            name,
            self.settings.audio.sample_rate,
            options,
            kit.as_deref(),
        )?;
        self.switch_to(project, dir)?;
        Ok(self.publish())
    }

    /// File ▸ Open Recent's projects for the start dialog, newest first,
    /// each with when it was last saved. One moved or deleted since is
    /// kept, marked missing, so the dialog can say so.
    pub fn project_recent(&self) -> Vec<RecentProject> {
        self.settings
            .recent_projects
            .iter()
            .map(|name| {
                let saved = self.projects.saved_at(name);
                RecentProject {
                    name: name.clone(),
                    saved,
                    missing: saved.is_none(),
                }
            })
            .collect()
    }

    pub fn project_open(&mut self, name: &str) -> Result<EngineStatus> {
        let (project, dir) = self.projects.open(name)?;
        self.switch_to(project, dir)?;
        Ok(self.publish())
    }

    /// Writes `project.json`, every plugin's state into `plugins/`, and
    /// clears the journal: everything in it is in the file now.
    pub fn project_save(&mut self) -> Result<EngineStatus> {
        self.capture_plugin_states();
        self.write_plugin_states()?;
        self.write_designs()?;
        cantina_project::save(&self.project, &self.project_dir)?;
        self.journal.clear()?;
        self.recovery = None;
        self.dirty = false;
        Ok(self.publish())
    }

    /// Renames the open project: saves it, then moves its folder to the
    /// new name. Like saving, it is not an edit, so there is nothing to
    /// undo; the undo history stays.
    pub fn project_rename(&mut self, name: &str) -> Result<EngineStatus> {
        if !self.recording.is_empty() {
            bail!("stop recording before renaming the project");
        }
        let (name, dir) = self.projects.free_dir(name)?;
        self.project_save()?;
        if let Some(live) = &mut self.live {
            live.transport().stop();
        }
        let old = self.project.name.clone();
        std::fs::rename(&self.project_dir, &dir).with_context(|| {
            format!(
                "renaming {} to {}",
                self.project_dir.display(),
                dir.display()
            )
        })?;
        self.project.name = name;
        self.project_dir = dir;
        self.journal = Journal::in_dir(&self.project_dir);
        cantina_project::save(&self.project, &self.project_dir)?;
        self.settings.recent_projects.retain(|n| *n != old);
        self.remember_project();
        self.sync()?;
        Ok(self.publish())
    }

    /// Copies the open project, as it is now, to a new name and switches
    /// to the copy, as Save As does. The original stays as it was last
    /// saved, and keeps any unsaved edits in its journal for recovery.
    pub fn project_duplicate(&mut self, name: &str) -> Result<EngineStatus> {
        if !self.recording.is_empty() {
            bail!("stop recording before duplicating the project");
        }
        let (name, dir) = self.projects.free_dir(name)?;
        if let Some(live) = &mut self.live {
            live.transport().stop();
        }
        self.capture_plugin_states();
        crate::project::copy_project_dir(&self.project_dir, &dir)?;
        self.project.name = name;
        self.project_dir = dir;
        self.journal = Journal::in_dir(&self.project_dir);
        self.write_plugin_states()?;
        self.write_designs()?;
        cantina_project::save(&self.project, &self.project_dir)?;
        self.recovery = None;
        self.dirty = false;
        self.remember_project();
        self.sync()?;
        Ok(self.publish())
    }

    /// Where a project folder or the library is, to show in Finder.
    pub fn reveal_path(&self, what: &str) -> Result<(PathBuf, bool)> {
        match what {
            // Select the project's folder in its parent.
            "project" => Ok((self.project_dir.clone(), true)),
            // Open the folder that holds the projects and the library.
            "library" => Ok((self.projects.root().to_path_buf(), false)),
            other => bail!("nothing to show called {other}"),
        }
    }

    /// Copies WAV files into the project's `audio/` folder and puts each
    /// on `track` at `start`, one after the other. A file at another
    /// sample rate is converted, and one with more than two channels is
    /// folded to stereo, in the copy; the original is never touched.
    /// Each file is one edit.
    pub fn audio_import(
        &mut self,
        paths: &[String],
        track: u64,
        start: u64,
    ) -> Result<EngineStatus> {
        let track = TrackId(track);
        let target = self
            .project
            .track(track)
            .with_context(|| format!("no track {track}"))?;
        if target.kit.is_some() {
            bail!("audio goes on an audio track, not a drum track");
        }
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        std::fs::create_dir_all(&audio_dir)
            .with_context(|| format!("creating {}", audio_dir.display()))?;
        let mut at = start;
        let mut status = None;
        for path in paths {
            let path = Path::new(path);
            let (info, channels) = wav::read_all(path).with_context(|| {
                format!("{} is not a WAV file Cantina can read", path.display())
            })?;
            let channels = fit_audio(
                channels,
                f64::from(info.sample_rate),
                self.project.sample_rate,
            );
            let length = channels.first().map_or(0, Vec::len) as u64;
            if length == 0 {
                bail!("{} holds no audio", path.display());
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "import".into());
            let (file, _) = take_name(&audio_dir, &stem);
            wav::write(&audio_dir.join(&file), self.project.sample_rate, &channels)?;
            let clip = Clip {
                id: ClipId(self.project.fresh_id()),
                name: stem,
                file,
                start: at,
                offset: 0,
                length,
                tab: None,
            };
            at += length;
            status = Some(self.apply(Command::InsertClip { track, clip })?);
        }
        Ok(status.unwrap_or_else(|| self.publish()))
    }

    /// Copies a kit folder into the user's kits, after checking that it
    /// loads, and makes it available to drum tracks.
    pub fn kit_import(&mut self, dir: &str) -> Result<EngineStatus> {
        let from = Path::new(dir);
        kit::load(from)
            .with_context(|| format!("{} is not a kit Cantina can load", from.display()))?;
        let name = from
            .file_name()
            .context("a kit is a folder with a name")?
            .to_os_string();
        let to = self.projects.kits_dir().join(&name);
        if to.exists() {
            bail!(
                "there is already a kit folder called {}",
                name.to_string_lossy()
            );
        }
        crate::project::copy_project_dir(from, &to)?;
        Ok(self.kits_rescan())
    }

    fn switch_to(&mut self, project: Project, dir: PathBuf) -> Result<()> {
        if !self.recording.is_empty() {
            self.transport_stop()?;
        }
        if let Some(live) = &mut self.live {
            live.transport().stop();
            live.transport().locate(0);
        }
        self.project = project;
        self.journal = Journal::in_dir(&dir);
        self.plugin_states = read_plugin_states(&dir);
        let mut problems = Vec::new();
        self.designs = pedals::load_dir(&dir.join(PEDALS_DIR), &mut problems);
        self.catalog = Catalog::default();
        self.notice_problems(problems);
        self.project_dir = dir;
        self.dirty = false;
        self.history.clear();
        self.remember_project();
        self.read_journal();
        if self.adopt_sample_rate() {
            self.rebuild(None);
            Ok(())
        } else {
            if let Some(live) = &mut self.live {
                live.set_tempo_map(tempo_map(&self.project.tempo));
            }
            self.sync()
        }
    }

    /// The project's audio is at one rate, so the engine runs at that
    /// rate. Returns true when the settings changed.
    fn adopt_sample_rate(&mut self) -> bool {
        if self.settings.audio.sample_rate == self.project.sample_rate {
            return false;
        }
        self.settings.audio.sample_rate = self.project.sample_rate;
        self.persist();
        true
    }

    fn remember_project(&mut self) {
        let name = self.project.name.clone();
        let recent = remember_recent(&self.settings.recent_projects, &name);
        if self.settings.last_project.as_deref() != Some(name.as_str())
            || recent != self.settings.recent_projects
        {
            self.settings.last_project = Some(name);
            self.settings.recent_projects = recent;
            self.persist();
        }
        crate::menu::set_recent(&self.app, &self.recent_projects());
    }

    /// The projects for File ▸ Open Recent, newest first, leaving out
    /// any that no longer exist.
    pub fn recent_projects(&self) -> Vec<String> {
        self.settings
            .recent_projects
            .iter()
            .filter(|name| {
                self.projects
                    .dir_for(name)
                    .join(cantina_project::PROJECT_FILE)
                    .is_file()
            })
            .cloned()
            .collect()
    }

    /// Empties File ▸ Open Recent.
    pub fn recent_clear(&mut self) {
        self.settings.recent_projects.clear();
        self.persist();
        crate::menu::set_recent(&self.app, &[]);
    }

    // ----- tracks -----

    pub fn track_add(&mut self, name: Option<String>) -> Result<EngineStatus> {
        let id = TrackId(self.project.fresh_id());
        let name = name.unwrap_or_else(|| format!("Track {}", self.project.tracks.len() + 1));
        let track = Track::new(id, name);
        self.apply(Command::InsertTrack {
            index: usize::MAX,
            track,
        })
    }

    pub fn track_remove(&mut self, id: u64) -> Result<EngineStatus> {
        let id = TrackId(id);
        if self.recording.iter().any(|r| r.track == id) {
            bail!("stop recording before removing the track");
        }
        self.apply(Command::RemoveTrack { id })
    }

    pub fn track_rename(&mut self, id: u64, name: String) -> Result<EngineStatus> {
        let name = name.trim().to_string();
        if name.is_empty() {
            bail!("a track needs a name");
        }
        self.apply(Command::RenameTrack {
            id: TrackId(id),
            name,
        })
    }

    pub fn track_move(&mut self, id: u64, to: usize) -> Result<EngineStatus> {
        self.apply(Command::MoveTrack {
            id: TrackId(id),
            to,
        })
    }

    /// Arms or disarms a track. Armed, its input is monitored through
    /// the chain and it records when the transport records.
    pub fn track_arm(&mut self, id: u64, armed: bool) -> Result<EngineStatus> {
        if let Some(track) = self.project.track(TrackId(id)) {
            if track.is_drums() && armed {
                bail!("{} is a drum track and has no input to record", track.name);
            }
        }
        self.apply(Command::ArmTrack {
            id: TrackId(id),
            armed,
        })
    }

    /// Takes a pedal out of the signal path, or puts it back.
    pub fn pedal_set_bypass(&mut self, id: u64, on: bool) -> Result<EngineStatus> {
        self.apply(Command::SetBypass {
            pedal: PedalId(id),
            on,
        })
    }

    /// Sets the loop region and whether playback loops, or clears it.
    pub fn loop_set(&mut self, region: Option<LoopRegion>) -> Result<EngineStatus> {
        self.apply(Command::SetLoop { region })
    }

    /// Bypasses every pedal on a track's chain, or turns them all on, as
    /// one edit.
    pub fn chain_bypass(&mut self, track: u64, on: bool) -> Result<EngineStatus> {
        let track = self
            .project
            .track(TrackId(track))
            .with_context(|| format!("no track {track}"))?;
        let commands: Vec<Command> = track
            .chain
            .iter()
            .filter(|p| p.bypassed != on)
            .map(|p| Command::SetBypass { pedal: p.id, on })
            .collect();
        if commands.is_empty() {
            return Ok(self.publish());
        }
        self.apply(Command::Batch { commands })
    }

    /// Switches the live input to one audio track's chain: arms it and
    /// disarms every other audio track, as one edit. On stage this is
    /// how a player moves between a clean and a lead sound.
    pub fn chain_select(&mut self, id: u64) -> Result<EngineStatus> {
        let id = TrackId(id);
        let track = self
            .project
            .track(id)
            .with_context(|| format!("no track {}", id.0))?;
        if track.is_drums() {
            bail!(
                "{} is a drum track and has no input to play through",
                track.name
            );
        }
        let commands: Vec<Command> = self
            .project
            .tracks
            .iter()
            .filter(|t| !t.is_drums() && t.armed != (t.id == id))
            .map(|t| Command::ArmTrack {
                id: t.id,
                armed: t.id == id,
            })
            .collect();
        if commands.is_empty() {
            return Ok(self.status());
        }
        self.apply(Command::Batch { commands })
    }

    // ----- clips -----

    pub fn clip_remove(&mut self, id: u64) -> Result<EngineStatus> {
        self.apply(Command::RemoveClip { id: ClipId(id) })
    }

    /// Puts a clip at `start` on `track`, which may be another track.
    pub fn clip_move(&mut self, id: u64, track: u64, start: u64) -> Result<EngineStatus> {
        self.apply(Command::MoveClip {
            id: ClipId(id),
            track: TrackId(track),
            start,
        })
    }

    /// Sets a clip's start, offset into its file and length. The region
    /// must lie within the file.
    pub fn clip_trim(
        &mut self,
        id: u64,
        start: u64,
        offset: u64,
        length: u64,
    ) -> Result<EngineStatus> {
        let id = ClipId(id);
        let (_, clip) = self
            .project
            .clip(id)
            .with_context(|| format!("no clip {id}"))?;
        let path = cantina_project::audio_dir(&self.project_dir).join(&clip.file);
        let frames = wav::info(&path)?.frames;
        if offset + length > frames {
            bail!(
                "{} has {frames} frames; the clip cannot reach {}",
                clip.file,
                offset + length
            );
        }
        self.apply(Command::TrimClip {
            id,
            start,
            offset,
            length,
        })
    }

    /// Copies a clip to the first space after it on its track that holds
    /// it, so the copy never plays over another clip.
    pub fn clip_duplicate(&mut self, id: u64) -> Result<EngineStatus> {
        let id = ClipId(id);
        let (track, clip) = self
            .project
            .clip(id)
            .with_context(|| format!("no clip {id}"))?;
        let start = track.free_clip_start(clip.end(), clip.length);
        let command = Command::InsertClip {
            track: track.id,
            clip: clip.duplicate(ClipId(self.project.fresh_id()), start),
        };
        self.apply(command)
    }

    /// Pastes a copied clip onto `track` at timeline sample `start`, or in
    /// the first space after it that holds the clip. The copy reads the
    /// same file in `audio/`, with the same trim, so a clip cut and
    /// deleted can still be pasted: deleting never removes the file.
    pub fn clip_paste(
        &mut self,
        track: u64,
        start: u64,
        name: &str,
        file: &str,
        offset: u64,
        length: u64,
    ) -> Result<EngineStatus> {
        if file.is_empty() || file.contains(['/', '\\']) || file.starts_with('.') {
            bail!("{file:?} is not a file in audio/");
        }
        if !cantina_project::audio_dir(&self.project_dir)
            .join(file)
            .is_file()
        {
            bail!("{file} is not in this project's audio; copy and paste within one project");
        }
        let target = self
            .project
            .track(TrackId(track))
            .with_context(|| format!("no track {track}"))?;
        if target.kit.is_some() {
            bail!(
                "{} is a drum track; paste audio on an audio track",
                target.name
            );
        }
        let start = target.free_clip_start(start, length);
        let clip = Clip {
            id: ClipId(self.project.fresh_id()),
            name: name.to_string(),
            file: file.to_string(),
            start,
            offset,
            length,
            tab: None,
        };
        self.apply(Command::InsertClip {
            track: TrackId(track),
            clip,
        })
    }

    /// Pastes a copied pattern clip onto drum track `track` at tick
    /// `start`, or in the first space after it that holds the clip.
    pub fn pattern_clip_paste(
        &mut self,
        track: u64,
        start: u64,
        pattern: u64,
        offset: u64,
        length: u64,
    ) -> Result<EngineStatus> {
        self.project
            .pattern(PatternId(pattern))
            .with_context(|| format!("no pattern {pattern}"))?;
        let target = self
            .project
            .track(TrackId(track))
            .with_context(|| format!("no track {track}"))?;
        if target.kit.is_none() {
            bail!(
                "{} is an audio track; paste a pattern on a drum track",
                target.name
            );
        }
        let start = target.free_pattern_start(start, length);
        let clip = PatternClip {
            id: PatternClipId(self.project.fresh_id()),
            pattern: PatternId(pattern),
            start,
            offset,
            length,
            fill: None,
        };
        self.apply(Command::InsertPatternClip {
            track: TrackId(track),
            clip,
        })
    }

    /// Cuts a clip at timeline sample `at`; the right part is a new clip.
    pub fn clip_split(&mut self, id: u64, at: u64) -> Result<EngineStatus> {
        let new_id = ClipId(self.project.fresh_id());
        self.apply(Command::SplitClip {
            id: ClipId(id),
            at,
            new_id,
        })
    }

    /// The samples of a file in `audio/`, mixed to mono, as little-endian
    /// f32 for the UI's peak cache. `file` is a name inside `audio/`, so
    /// no path can escape the project.
    pub fn audio_samples(&self, file: &str) -> Result<Vec<u8>> {
        if file.is_empty() || file.contains(['/', '\\']) || file.starts_with('.') {
            bail!("{file:?} is not a file in audio/");
        }
        let path = cantina_project::audio_dir(&self.project_dir).join(file);
        let (info, channels) = wav::read_all(&path)?;
        let frames = info.frames as usize;
        let mut bytes = Vec::with_capacity(frames * 4);
        let scale = 1.0 / channels.len().max(1) as f32;
        for i in 0..frames {
            let sum: f32 = channels
                .iter()
                .map(|c| c.get(i).copied().unwrap_or(0.0))
                .sum();
            bytes.extend_from_slice(&(sum * scale).to_le_bytes());
        }
        Ok(bytes)
    }

    // ----- the transport -----

    /// Rolls from the current position.
    pub fn transport_play(&mut self) -> Result<EngineStatus> {
        let Some(live) = &mut self.live else {
            bail!("start the engine before playing");
        };
        live.transport().play();
        Ok(self.publish())
    }

    /// Starts a take on every armed track from the current position:
    /// from a stopped transport the click counts in four beats with the
    /// timeline held, and it rolls with the take's first frame on that
    /// position. Takes are written to `audio/` and become clips on stop.
    pub fn transport_record(&mut self) -> Result<EngineStatus> {
        let armed: Vec<(TrackId, String)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.armed && !t.is_drums())
            .map(|t| (t.id, t.name.clone()))
            .collect();
        if self.jam.is_some() {
            bail!("leave the jam before recording");
        }
        if armed.is_empty() {
            bail!("arm a track before recording");
        }
        if self.practice.speed < 1.0 {
            bail!("set the practice speed back to 100% to record");
        }
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        cantina_project::create_layout(&self.project_dir)?;
        let Some(live) = &mut self.live else {
            bail!("start the engine before recording");
        };
        if !self.recording.is_empty() {
            return Ok(self.status());
        }
        let from = live.transport().position();
        let info = live.stream.info();
        let correction = live.compensation as u64
            + info.input_latency_frames as u64
            + info.output_latency_frames as u64;
        let sample_rate = info.sample_rate;
        for (track, track_name) in armed {
            let (file, name) = take_name(&audio_dir, &track_name);
            live.track_mut(track)?
                .recorder()
                .context("a drum track cannot record")?
                .start(audio_dir.join(&file), from)?;
            self.recording.push(Recording {
                track,
                file,
                name,
                from,
                correction,
            });
        }
        // Over a loop, a take is one pass: it stops at the loop's end
        // rather than wrapping and recording over itself.
        let stop = self
            .project
            .loop_region
            .filter(|r| r.on && r.end > from)
            .map(|r| r.end);
        live.transport().stop_at(stop);
        if live.transport().is_playing() || !self.settings.count_in {
            live.transport().play();
        } else {
            live.transport().count_in(sample_rate);
        }
        self.transport_state
            .recording
            .store(true, Ordering::Relaxed);
        Ok(self.publish())
    }

    /// Stops the transport, and any recording with it: each take is
    /// finalized and placed on its track as a clip.
    pub fn transport_stop(&mut self) -> Result<EngineStatus> {
        let Some(live) = &mut self.live else {
            self.recording.clear();
            return Ok(self.status());
        };
        live.transport().stop();
        if self.recording.is_empty() {
            return Ok(self.publish());
        }
        self.transport_state
            .recording
            .store(false, Ordering::Relaxed);
        let recordings = std::mem::take(&mut self.recording);
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        let mut commands = Vec::new();
        let mut problems = Vec::new();
        for take in recordings {
            match finish_take(live, &take, &audio_dir) {
                Ok(frames) if frames > 0 => {
                    let Some((start, offset, length)) =
                        place_take(take.from, frames, take.correction)
                    else {
                        continue;
                    };
                    let id = ClipId(self.project.fresh_id() + commands.len() as u64);
                    commands.push(Command::InsertClip {
                        track: take.track,
                        clip: Clip {
                            id,
                            name: take.name.clone(),
                            file: take.file.clone(),
                            start,
                            offset,
                            length,
                            tab: None,
                        },
                    });
                }
                // Stopped before a frame was recorded, as during the
                // count-in: nothing refers to the file and nothing will.
                Ok(_) => {
                    let _ = std::fs::remove_file(audio_dir.join(&take.file));
                }
                Err(e) => problems.push(format!("{}: {e:#}", take.name)),
            }
        }
        if !commands.is_empty() {
            let command = if commands.len() == 1 {
                commands.remove(0)
            } else {
                Command::Batch { commands }
            };
            self.apply(command)?;
        }
        if !problems.is_empty() {
            bail!("{}", problems.join("; "));
        }
        Ok(self.publish())
    }

    /// Moves the playhead. Allowed while rolling.
    pub fn transport_locate(&mut self, sample: u64) -> Result<EngineStatus> {
        let Some(live) = &mut self.live else {
            bail!("start the engine first");
        };
        live.transport().locate(sample);
        Ok(self.publish())
    }

    pub fn tempo_set(&mut self, tempo: Tempo) -> Result<EngineStatus> {
        self.apply(Command::SetTempo { tempo })
    }

    // ----- the mixer -----

    /// Sets a track's gain, or the master's with no track. Not
    /// published: fader drags are frequent and the UI shows what it sent.
    pub fn strip_gain(&mut self, track: Option<u64>, db: f32) -> Result<()> {
        let command = Command::SetGain {
            track: track.map(TrackId),
            db: db.clamp(-60.0, 12.0),
        };
        self.apply_quietly(command)
    }

    pub fn strip_pan(&mut self, track: u64, pan: f32) -> Result<()> {
        let command = Command::SetPan {
            track: TrackId(track),
            pan: pan.clamp(-1.0, 1.0),
        };
        self.apply_quietly(command)
    }

    pub fn strip_mute(&mut self, track: Option<u64>, on: bool) -> Result<EngineStatus> {
        self.apply(Command::SetMute {
            track: track.map(TrackId),
            on,
        })
    }

    pub fn strip_solo(&mut self, track: u64, on: bool) -> Result<EngineStatus> {
        self.apply(Command::SetSolo {
            track: TrackId(track),
            on,
        })
    }

    /// Applies a command without pushing a status: for continuous
    /// controls the UI already shows the value it sent.
    fn apply_quietly(&mut self, command: Command) -> Result<()> {
        self.history
            .apply(&mut self.project, command.clone())
            .map_err(|e| anyhow!("{e}"))?;
        // Editing on without deciding is deciding: the old session's
        // edits would collide with these in the journal.
        if self.recovery.take().is_some() {
            if let Err(e) = self.journal.clear() {
                self.last_error = Some(format!("clearing the journal: {e}"));
            }
        }
        self.dirty = true;
        self.journal_entry(&command);
        self.react(&command)
    }

    // ----- the chain -----

    /// Adds a pedal at the end of a track's chain, or at `index`.
    pub fn chain_add(
        &mut self,
        track: u64,
        spec: &str,
        index: Option<usize>,
    ) -> Result<EngineStatus> {
        self.chain_add_with(TrackId(track), spec, index, None)?;
        Ok(self.status())
    }

    /// Adds a pedal with these parameter values, or its defaults, as one
    /// edit, and returns its id.
    fn chain_add_with(
        &mut self,
        track: TrackId,
        spec: &str,
        index: Option<usize>,
        values: Option<Vec<f32>>,
    ) -> Result<PedalId> {
        let id = PedalId(self.project.fresh_id());
        let pedal = self.new_pedal(id, spec, values)?;
        self.apply(Command::InsertPedal {
            track,
            index: index.unwrap_or(usize::MAX),
            pedal,
        })?;
        Ok(id)
    }

    /// A pedal of `spec` with id `id`, ready to insert: a new cabinet on
    /// the first impulse installed, a library pedal copied into the
    /// project, and the values (held to range) or the defaults.
    fn new_pedal(&mut self, id: PedalId, spec: &str, values: Option<Vec<f32>>) -> Result<Pedal> {
        // A new cabinet starts on the first impulse installed.
        let spec = match (spec, self.irs.first_id()) {
            ("cab", Some(first)) => format!("cab:{first}"),
            _ => spec.to_string(),
        };
        // A library pedal comes in with its definition, and the project
        // gets its own copy, so the song keeps sounding the same if the
        // library pedal changes later.
        if let Some(id) = spec.strip_prefix("pedal:") {
            if !self.designs.contains_key(id) {
                let entry = self
                    .library
                    .entry(id)
                    .with_context(|| format!("no pedal {id:?} in the library"))?;
                let def = pedals::read(&entry.path)?;
                pedals::write(&self.project_pedal_path(id), &def)?;
                self.designs.insert(id.to_string(), def);
            }
        }
        let kind = self.catalog.get(&spec, &mut self.irs, &self.designs)?;
        let defaults: Vec<f32> = kind.params.iter().map(|p| p.default).collect();
        let values = match values {
            Some(v) if v.len() == defaults.len() => {
                kind.params.iter().zip(v).map(|(p, v)| p.clamp(v)).collect()
            }
            _ => defaults,
        };
        Ok(Pedal {
            id,
            spec: spec.clone(),
            values,
            bypassed: false,
        })
    }

    /// Gives a track these pedals as one edit, after its chain or in
    /// place of it; returns their ids.
    fn chain_set(
        &mut self,
        track: TrackId,
        pedals: Vec<(String, Vec<f32>)>,
        replace: bool,
    ) -> Result<Vec<PedalId>> {
        let existing: Vec<PedalId> = self
            .project
            .track(track)
            .with_context(|| format!("no track {track}"))?
            .chain
            .iter()
            .map(|p| p.id)
            .collect();
        let mut commands = Vec::new();
        if replace {
            commands.extend(existing.into_iter().map(|id| Command::RemovePedal { id }));
        }
        let first = self.project.fresh_id();
        let mut ids = Vec::new();
        for (n, (spec, values)) in pedals.into_iter().enumerate() {
            let id = PedalId(first + n as u64);
            commands.push(Command::InsertPedal {
                track,
                index: usize::MAX,
                pedal: self.new_pedal(id, &spec, Some(values))?,
            });
            ids.push(id);
        }
        self.apply(Command::Batch { commands })?;
        Ok(ids)
    }

    pub fn chain_remove(&mut self, id: u64) -> Result<EngineStatus> {
        self.apply(Command::RemovePedal { id: PedalId(id) })
    }

    /// Moves a pedal to position `to` in its chain.
    pub fn chain_move(&mut self, id: u64, to: usize) -> Result<EngineStatus> {
        self.apply(Command::MovePedal {
            id: PedalId(id),
            to,
        })
    }

    /// Sets a pedal parameter. Not published: knob drags are frequent and
    /// the UI already shows the value it sent.
    pub fn chain_set_param(&mut self, id: u64, param: usize, value: f32) -> Result<()> {
        let id = PedalId(id);
        let (_, pedal) = self
            .project
            .pedal(id)
            .with_context(|| format!("no pedal {id}"))?;
        let spec = pedal.spec.clone();
        let kind = self.catalog.get(&spec, &mut self.irs, &self.designs)?;
        let info = kind
            .params
            .get(param)
            .with_context(|| format!("{} has no parameter {param}", kind.name))?;
        let value = info.clamp(value);
        self.apply_quietly(Command::SetParam {
            pedal: id,
            param,
            value,
        })
    }

    // ----- the designer -----

    /// Starts a new design: an empty pedal (input wired to output) at
    /// the end of a track's chain, which the designer then edits with
    /// the signal running through it.
    pub fn designer_new(&mut self, track: u64) -> Result<EngineStatus> {
        let key = format!("draft-{}", self.project.fresh_id());
        self.designs
            .insert(key.clone(), PedalDef::empty("New pedal"));
        self.chain_add(track, &format!("pedal:{key}"), None)
    }

    /// The definition behind a designed pedal in a chain.
    pub fn pedal_definition(&self, id: u64) -> Result<serde_json::Value> {
        let (_, pedal) = self
            .project
            .pedal(PedalId(id))
            .with_context(|| format!("no pedal {id}"))?;
        let key = pedal
            .spec
            .strip_prefix("pedal:")
            .with_context(|| format!("{} is not a designed pedal", pedal.spec))?;
        let def = self
            .designs
            .get(key)
            .with_context(|| format!("no pedal design {key:?}"))?;
        Ok(serde_json::to_value(def)?)
    }

    /// A stock pedal's definition, for the designer to start from.
    pub fn pedal_stock_definition(&self, name: &str) -> Result<serde_json::Value> {
        let def = cantina_dsp::stock_definition(name)
            .with_context(|| format!("{name} is not a stock pedal in the pedal format"))?;
        Ok(serde_json::to_value(def)?)
    }

    /// Every problem with a definition, with the node and field named;
    /// empty when it would compile.
    pub fn pedal_check(definition: serde_json::Value) -> Vec<Problem> {
        match serde_json::from_value::<PedalDef>(definition) {
            Ok(def) => def
                .validate()
                .err()
                .map(|e| e.problems())
                .unwrap_or_default(),
            Err(e) => PedalError::Json(e.to_string()).problems(),
        }
    }

    /// Replaces a designed pedal's definition and parameter values and
    /// rebuilds its node, so the change is heard on the next block. A
    /// definition with problems changes nothing: the last valid pedal
    /// keeps playing and the problems come back for the designer to
    /// show.
    pub fn designer_update(
        &mut self,
        id: u64,
        definition: serde_json::Value,
        values: Vec<f32>,
    ) -> Result<DesignerResult> {
        let id = PedalId(id);
        let (_, pedal) = self
            .project
            .pedal(id)
            .with_context(|| format!("no pedal {id}"))?;
        let spec = pedal.spec.clone();
        let current = pedal.values.clone();
        let key = spec
            .strip_prefix("pedal:")
            .with_context(|| format!("{spec} is not a designed pedal"))?
            .to_string();
        let problems = |engine: &Self, problems: Vec<Problem>| DesignerResult {
            status: engine.status(),
            problems,
        };
        let def: PedalDef = match serde_json::from_value(definition) {
            Ok(def) => def,
            Err(e) => {
                return Ok(problems(self, PedalError::Json(e.to_string()).problems()));
            }
        };
        let compiled = match cantina_dsp::compile(&def) {
            Ok(compiled) => compiled,
            Err(e) => return Ok(problems(self, e.problems())),
        };
        let mut values = values;
        values.resize(compiled.params().len(), 0.0);
        for (value, info) in values.iter_mut().zip(compiled.params()) {
            if !value.is_finite() {
                *value = info.default;
            }
            *value = info.clamp(*value);
        }
        self.designs.insert(key, def);
        self.catalog.forget(&spec);
        self.catalog.get(&spec, &mut self.irs, &self.designs)?;
        if values != current {
            self.apply_quietly(Command::SetPedalValues { id, values })?;
        }
        self.rebuild_pedal(id)?;
        Ok(DesignerResult {
            status: self.publish(),
            problems: Vec::new(),
        })
    }

    /// Where the project keeps its copy of a designed pedal.
    fn project_pedal_path(&self, id: &str) -> PathBuf {
        self.project_dir
            .join(PEDALS_DIR)
            .join(pedals::file_name(id))
    }

    /// Writes the project's copy of every designed pedal it uses,
    /// drafts included, so the song opens the same on its own.
    fn write_designs(&self) -> Result<()> {
        for track in &self.project.tracks {
            for pedal in &track.chain {
                let Some(id) = pedal.spec.strip_prefix("pedal:") else {
                    continue;
                };
                if let Some(def) = self.designs.get(id) {
                    pedals::write(&self.project_pedal_path(id), def)?;
                }
            }
        }
        Ok(())
    }

    /// Saves a designed pedal to the library under an id made from its
    /// name, writes the project's copy, and points the pedal at that
    /// id if it was a draft.
    pub fn designer_save(&mut self, id: u64) -> Result<EngineStatus> {
        let pedal_id = PedalId(id);
        let (_, pedal) = self
            .project
            .pedal(pedal_id)
            .with_context(|| format!("no pedal {id}"))?;
        let key = pedal
            .spec
            .strip_prefix("pedal:")
            .with_context(|| format!("{} is not a designed pedal", pedal.spec))?
            .to_string();
        let def = self
            .designs
            .get(&key)
            .cloned()
            .with_context(|| format!("no pedal design {key:?}"))?;
        let slug = pedals::slug(&def.name);
        pedals::write(&self.library.dir().join(pedals::file_name(&slug)), &def)?;
        pedals::write(&self.project_pedal_path(&slug), &def)?;
        self.designs.insert(slug.clone(), def);
        let mut problems = Vec::new();
        self.library.rescan(&mut problems);
        self.notice_problems(problems);
        if slug != key {
            return self.apply(Command::SetPedalSpec {
                id: pedal_id,
                spec: format!("pedal:{slug}"),
            });
        }
        Ok(self.publish())
    }

    /// Writes a designed pedal's definition to `path`, for sharing.
    pub fn pedal_export(&self, id: u64, path: &str) -> Result<()> {
        let (_, pedal) = self
            .project
            .pedal(PedalId(id))
            .with_context(|| format!("no pedal {id}"))?;
        let key = pedal
            .spec
            .strip_prefix("pedal:")
            .with_context(|| format!("{} is not a designed pedal", pedal.spec))?;
        let def = self
            .designs
            .get(key)
            .with_context(|| format!("no pedal design {key:?}"))?;
        let mut path = PathBuf::from(path);
        if pedals::id_of(&path).is_none() {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.trim_end_matches(".pedal").to_string())
                .unwrap_or_else(|| key.to_string());
            path.set_file_name(pedals::file_name(&stem));
        }
        pedals::write(&path, def)
    }

    /// Copies a pedal file into the library and, with a track, adds it
    /// to that track's chain.
    pub fn pedal_import(&mut self, path: &str, track: Option<u64>) -> Result<EngineStatus> {
        let entry = pedals::import(Path::new(path), self.library.dir())?;
        let mut problems = Vec::new();
        self.library.rescan(&mut problems);
        self.notice_problems(problems);
        match track {
            Some(track) => self.chain_add(track, &format!("pedal:{}", entry.id), None),
            None => Ok(self.publish()),
        }
    }

    /// Looks for library pedals again.
    pub fn pedals_rescan(&mut self) -> EngineStatus {
        let mut problems = Vec::new();
        self.library.rescan(&mut problems);
        self.notice_problems(problems);
        self.publish()
    }

    /// Files that would not load become a notice.
    fn notice_problems(&mut self, problems: Vec<String>) {
        if !problems.is_empty() {
            self.notice = Some(problems.join("; "));
            self.notice_seq += 1;
        }
    }

    /// Drops a pedal's live node so the next sync builds it again from
    /// its spec and values.
    fn rebuild_pedal(&mut self, id: PedalId) -> Result<()> {
        if let Some(live) = &mut self.live {
            for track in &mut live.tracks {
                if let Some(i) = track.pedals.iter().position(|p| p.id == id) {
                    let pedal = track.pedals.remove(i);
                    live.controller.graph_mut().remove(pedal.node)?;
                }
            }
        }
        self.sync()
    }

    pub fn audio_settings(&self) -> AudioSettings {
        self.settings.audio.clone()
    }

    /// Every device, with what the picker needs to offer it.
    pub fn list_devices(&self) -> Result<Vec<DeviceEntry>> {
        let default_input = device::default_input();
        let default_output = device::default_output();
        let mut entries = Vec::new();
        for info in device::list()? {
            if info.input_channels == 0 && info.output_channels == 0 {
                continue;
            }
            let (buffer_min, buffer_max) = device::buffer_frame_size_range(info.id)
                .unwrap_or((info.buffer_frames, info.buffer_frames));
            entries.push(DeviceEntry {
                uid: info.uid,
                name: info.name,
                input_channels: info.input_channels,
                output_channels: info.output_channels,
                sample_rates: device::available_sample_rates(info.id)
                    .unwrap_or_else(|_| vec![info.sample_rate]),
                buffer_min,
                buffer_max,
                default_input: Some(info.id) == default_input,
                default_output: Some(info.id) == default_output,
            });
        }
        Ok(entries)
    }

    /// Saves new audio settings and, if the stream is open, rebuilds it
    /// to match. The sample rate is the project's: audio already in it
    /// is at that rate.
    pub fn set_audio_settings(&mut self, mut audio: AudioSettings) -> EngineStatus {
        if audio.sample_rate != self.project.sample_rate {
            if self.project.tracks.iter().any(|t| !t.clips.is_empty()) {
                self.notice = Some(format!(
                    "{} has audio at {} Hz, so the engine stays at that rate",
                    self.project.name, self.project.sample_rate
                ));
                self.notice_seq = self.notice_seq.wrapping_add(1);
                audio.sample_rate = self.project.sample_rate;
            } else {
                self.project.sample_rate = audio.sample_rate;
                self.dirty = true;
            }
        }
        self.settings.audio = audio;
        self.persist();
        if self.live.is_some() {
            self.rebuild(None);
        }
        self.publish()
    }

    /// Opens the stream. Starting an engine that is already running is a
    /// no-op.
    pub fn start(&mut self) -> EngineStatus {
        if self.live.is_none() {
            self.rebuild(None);
        }
        self.publish()
    }

    /// Closes the stream. The graph is dropped here, on this thread, never
    /// on the audio thread.
    pub fn stop(&mut self) -> EngineStatus {
        self.close();
        self.publish()
    }

    pub fn dismiss_notice(&mut self) -> EngineStatus {
        self.notice = None;
        self.status()
    }

    /// Adds a listener for binary frames. It is dropped when sending fails.
    pub fn subscribe_meters(&self, channel: Channel<InvokeResponseBody>) {
        self.meter_subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Subscriber {
                channel,
                in_flight: Arc::new(AtomicBool::new(false)),
            });
    }

    /// The UI has taken the last frame; the pump may send another.
    pub fn meter_ack(&self) {
        for sub in self
            .meter_subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            sub.in_flight.store(false, Ordering::Release);
        }
    }

    pub fn frames_dropped(&self) -> u32 {
        self.frames_dropped.load(Ordering::Relaxed)
    }

    /// Reacts to a hardware change delivered by the watcher thread.
    fn on_device_event(&mut self, event: DeviceEvent) {
        // Decide with shared borrows first, then act.
        let Some(live) = &self.live else {
            return;
        };
        let audio = &self.settings.audio;
        let (why, new_rate): (String, Option<f64>) = match event {
            DeviceEvent::DevicesChanged | DeviceEvent::DeviceGone(_) => {
                let lost = [
                    (live.input, &live.input_name),
                    (live.output, &live.output_name),
                ]
                .into_iter()
                .find(|(id, _)| !device::is_alive(*id));
                match lost {
                    Some((_, name)) => (format!("{name} was disconnected"), None),
                    None => return,
                }
            }
            DeviceEvent::SampleRateChanged(id, rate) => {
                let current = live.stream.info().sample_rate;
                if (id != live.input && id != live.output) || rate == current {
                    return;
                }
                let name = if id == live.input {
                    &live.input_name
                } else {
                    &live.output_name
                };
                (
                    format!("{name} changed to {rate} Hz outside Cantina"),
                    Some(rate),
                )
            }
            DeviceEvent::DefaultInputChanged => {
                if audio.input_uid.is_some() || device::default_input() == Some(live.input) {
                    return;
                }
                ("The system default input changed".to_string(), None)
            }
            DeviceEvent::DefaultOutputChanged => {
                if audio.output_uid.is_some() || device::default_output() == Some(live.output) {
                    return;
                }
                ("The system default output changed".to_string(), None)
            }
        };
        if let Some(rate) = new_rate {
            // A project with audio stays at its rate, which the rebuild
            // puts the device back to. One without follows the device.
            if self.project.tracks.iter().all(|t| t.clips.is_empty()) {
                self.project.sample_rate = rate;
                self.dirty = true;
                self.settings.audio.sample_rate = rate;
                self.persist();
            }
        }
        self.rebuild(Some(why));
    }

    /// Closes any open stream and opens one for the current settings and
    /// project. `why` is the start of the notice shown to the user when
    /// the engine did this on its own.
    fn rebuild(&mut self, why: Option<String>) {
        self.close();
        match self.open() {
            Ok((live, fallback)) => {
                self.last_error = None;
                let message = match (why, fallback) {
                    (Some(why), Some(fb)) => Some(format!("{why}; {fb}")),
                    (Some(why), None) => Some(format!("{why}; the stream was rebuilt")),
                    (None, Some(fb)) => Some(fb),
                    (None, None) => None,
                };
                if let Some(m) = message {
                    self.notice = Some(m);
                    self.notice_seq = self.notice_seq.wrapping_add(1);
                }
                self.live = Some(live);
                if let Err(e) = self.sync() {
                    self.last_error = Some(format!("{e:#}"));
                }
            }
            Err(e) => {
                self.last_error = Some(format!("{e:#}"));
                if let Some(why) = why {
                    self.notice = Some(format!("{why}; the stream could not be reopened"));
                    self.notice_seq = self.notice_seq.wrapping_add(1);
                }
            }
        }
        self.publish();
    }

    fn close(&mut self) {
        // Plugins are recreated by the next sync; keep what they hold.
        self.capture_plugin_states();
        self.close_editors(self.open_editors.iter().copied().collect());
        if let Some(mut live) = self.live.take() {
            live.pump_stop.store(true, Ordering::Relaxed);
            // A recording in progress ends with the stream; the writer
            // finalizes the file when the node goes away. It is not
            // placed on the track, since it was cut short.
            if !self.recording.is_empty() {
                self.recording.clear();
                self.transport_state
                    .recording
                    .store(false, Ordering::Relaxed);
                for track in &mut live.tracks {
                    if let Some(recorder) = track.recorder() {
                        let _ = recorder.stop();
                    }
                }
            }
            live.stream.close();
            live.controller.collect();
        }
    }

    /// Resolves a saved device, or falls back to the system default and
    /// says so.
    fn resolve(
        wanted: &Option<String>,
        default: Option<DeviceId>,
        what: &str,
    ) -> Result<(DeviceId, Option<String>)> {
        let default = || default.with_context(|| format!("no default {what} device"));
        match wanted {
            None => Ok((default()?, None)),
            Some(uid) => match device::find_by_uid(uid)? {
                Some(id) if device::is_alive(id) => Ok((id, None)),
                _ => {
                    let id = default()?;
                    let name = device::name(id).unwrap_or_else(|_| "the default".into());
                    Ok((
                        id,
                        Some(format!(
                            "the saved {what} device is not connected; using {name}"
                        )),
                    ))
                }
            },
        }
    }

    /// Opens a stream for the settings with an empty graph; `sync` then
    /// builds the project into it. Returns the stream and any fallback
    /// notice.
    fn open(&self) -> Result<(Live, Option<String>)> {
        let audio = &self.settings.audio;
        let (input, in_note) = Self::resolve(&audio.input_uid, device::default_input(), "input")?;
        let (output, out_note) =
            Self::resolve(&audio.output_uid, device::default_output(), "output")?;
        let fallback = match (in_note, out_note) {
            (Some(a), Some(b)) => Some(format!("{a}; {b}")),
            (a, b) => a.or(b),
        };
        let sample_rate = audio.sample_rate;

        let (runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate,
            max_block_frames: MAX_BLOCK_FRAMES,
            channels: CHANNELS,
        })?;
        controller
            .transport_mut()
            .set_tempo_map(tempo_map(&self.project.tempo));
        controller.transport_mut().set_rate(self.practice.speed);
        let graph = controller.graph_mut();
        let m = &self.project.master;
        let master = graph.add(Box::new(Strip::new(m.gain_db, 0.0, m.mute)));
        let (input_meter, input_levels) = Meter::new();
        let (output_meter, output_levels) = Meter::new();
        let input_meter = graph.add(Box::new(input_meter));
        let output_meter = graph.add(Box::new(output_meter));
        let (scope, scope_handle) = Scope::new();
        let scope = graph.add(Box::new(scope));
        let click = graph.add(Box::new(Click::default()));
        let metronome = &self.settings.metronome;
        graph.set_param(click, CLICK_ON, if metronome.on { 1.0 } else { 0.0 });
        graph.set_param(click, CLICK_SOUND, metronome.sound_index() as f32);
        graph.set_param(click, CLICK_LEVEL, metronome.level_db);

        let stream = Stream::open(
            StreamConfig {
                input,
                output,
                sample_rate,
                buffer_frames: audio.buffer_frames,
                input_channel: 0,
            },
            runtime,
        )
        .context("opening the audio stream")?;

        let (tx, rx) = mpsc::channel();
        let watcher = Watcher::new(tx, &[input, output])?;
        spawn_watcher_thread(self.app.clone(), rx);
        let pump_stop = Arc::new(AtomicBool::new(false));
        let meters: Meters = Arc::new(Mutex::new(vec![
            input_levels.clone(),
            output_levels.clone(),
        ]));
        let readouts: Readouts = Arc::new(Mutex::new(Vec::new()));
        let analysis = Arc::new(PumpAnalysis {
            scope: scope_handle,
            meter: AtomicU32::new(1),
            tempo: Mutex::new(tempo_map(&self.project.tempo)),
            output_latency: stream.info().output_latency_frames as u64,
        });
        spawn_meter_pump(
            self.app.clone(),
            Arc::clone(&self.meter_subscribers),
            Arc::clone(&meters),
            Arc::clone(&readouts),
            Arc::clone(&analysis),
            controller.transport().clock(),
            sample_rate,
            Arc::clone(&self.transport_state),
            Arc::clone(&self.frames_dropped),
            Arc::clone(&pump_stop),
        );

        let live = Live {
            stream,
            controller,
            tracks: Vec::new(),
            master,
            compensation: 0,
            input_meter,
            output_meter,
            input_levels,
            output_levels,
            meters,
            readouts,
            scope,
            click,
            analysis_source: self.analysis_source,
            analysis,
            input,
            output,
            input_name: device::name(input)?,
            output_name: device::name(output)?,
            capture: None,
            _watcher: watcher,
            pump_stop,
        };
        Ok((live, fallback))
    }

    fn persist(&mut self) {
        if let Some(path) = &self.settings_path {
            if let Err(e) = self.settings.save(path) {
                self.last_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Pushes the status to the UI and returns it.
    fn publish(&self) -> EngineStatus {
        let status = self.status();
        let _ = self.app.emit(STATUS_EVENT, &status);
        status
    }
}

/// Commands the graph follows without a structural resync.
fn is_light(command: &Command) -> bool {
    matches!(
        command,
        Command::SetParam { .. }
            | Command::SetPadMix { .. }
            | Command::SetClipTab { .. }
            | Command::EditTabNotes { .. }
            | Command::ArmTrack { .. }
            | Command::RenameTrack { .. }
            | Command::SetTempo { .. }
            | Command::SetGain { .. }
            | Command::SetPan { .. }
            | Command::SetMute { .. }
            | Command::SetSolo { .. }
            | Command::InsertPattern { .. }
            | Command::RemovePattern { .. }
            | Command::RenamePattern { .. }
            | Command::SetPatternStep { .. }
            | Command::SetPatternLength { .. }
            | Command::SetPatternResolution { .. }
            | Command::SetPatternSwing { .. }
            | Command::ReplacePattern { .. }
            | Command::InsertPatternClip { .. }
            | Command::RemovePatternClip { .. }
            | Command::MovePatternClip { .. }
            | Command::TrimPatternClip { .. }
            | Command::SplitPatternClip { .. }
            | Command::SetPatternClipFill { .. }
    )
}

/// Where kits live: the user's `Kits` folder next to the projects, which
/// wins, then the kits that ship with the app (`kits/` in the app's
/// resources, or the repository's `kits/` in a development build), and
/// anything `CANTINA_KITS` names.
fn kit_roots(app: &AppHandle, projects: &Projects) -> Vec<PathBuf> {
    let mut roots = vec![projects.kits_dir()];
    if let Some(extra) = std::env::var_os("CANTINA_KITS") {
        roots.extend(std::env::split_paths(&extra));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("kits"));
    }
    if cfg!(debug_assertions) {
        roots.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("kits"),
        );
    }
    roots
}

/// Where the stock grooves live: `patterns/` in the app's resources, or
/// the repository's `patterns/` in a development build, and anything
/// `CANTINA_GROOVES` names, which wins.
/// Where the stock rigs are: anything `CANTINA_RIGS` names, the bundle's
/// `rigs/`, and in a debug build the repository's.
fn rig_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(extra) = std::env::var_os("CANTINA_RIGS") {
        roots.extend(std::env::split_paths(&extra));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("rigs"));
    }
    if cfg!(debug_assertions) {
        roots.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("rigs"),
        );
    }
    roots
}

fn groove_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(extra) = std::env::var_os("CANTINA_GROOVES") {
        roots.extend(std::env::split_paths(&extra));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("patterns"));
    }
    if cfg!(debug_assertions) {
        roots.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("patterns"),
        );
    }
    roots
}

/// Where the stock jams are: the bundle's `jams/`, and the repository's
/// in a debug build.
fn jam_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(extra) = std::env::var_os("CANTINA_JAMS") {
        roots.extend(std::env::split_paths(&extra));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("jams"));
    }
    if cfg!(debug_assertions) {
        roots.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("jams"),
        );
    }
    roots
}

/// Where impulse responses live: the user's `IRs` folder next to the
/// projects, which wins, then the starter set that ships with the app
/// (`irs/` in the app's resources, or the repository's `irs/` in a
/// development build), and anything `CANTINA_IRS` names.
fn ir_roots(app: &AppHandle, projects: &Projects) -> Vec<PathBuf> {
    let mut roots = vec![projects.irs_dir()];
    if let Some(extra) = std::env::var_os("CANTINA_IRS") {
        roots.extend(std::env::split_paths(&extra));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("irs"));
    }
    if cfg!(debug_assertions) {
        roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("irs"));
    }
    roots
}

/// Where designed pedals live: the user's `Pedals` folder next to the
/// projects, which saves and imports go to, and anything
/// `CANTINA_PEDALS` names.
fn pedal_roots(app: &AppHandle, projects: &Projects) -> Vec<PathBuf> {
    let _ = app;
    let mut roots = vec![projects.pedals_dir()];
    if let Some(extra) = std::env::var_os("CANTINA_PEDALS") {
        roots.extend(std::env::split_paths(&extra));
    }
    roots
}

pub(crate) fn tempo_map(tempo: &Tempo) -> TempoMap {
    TempoMap::new(
        tempo.bpm,
        TimeSignature::new(tempo.numerator, tempo.denominator),
    )
}

/// File ▸ Open Recent after opening `name`: it first, no repeats, ten at
/// most.
fn remember_recent(recent: &[String], name: &str) -> Vec<String> {
    std::iter::once(name.to_string())
        .chain(recent.iter().filter(|n| *n != name).cloned())
        .take(10)
        .collect()
}

/// An imported file's channels made fit for the player: at the project's
/// rate, and at most stereo, with any further channels folded into left
/// and right in turn.
fn fit_audio(channels: Vec<Vec<f32>>, from: f64, to: f64) -> Vec<Vec<f32>> {
    let channels: Vec<Vec<f32>> = if from == to {
        channels
    } else {
        channels
            .iter()
            .map(|c| cantina_dsp::resample(c, from, to))
            .collect()
    };
    if channels.len() <= CHANNELS {
        return channels;
    }
    let len = channels.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = vec![vec![0.0; len]; CHANNELS];
    let per_side = channels.len().div_ceil(CHANNELS) as f32;
    for (i, channel) in channels.iter().enumerate() {
        for (o, x) in out[i % CHANNELS].iter_mut().zip(channel) {
            *o += x / per_side;
        }
    }
    out
}

/// A file name in `audio/` that is not taken, and the clip name to go
/// with it: `Guitar-3.wav` and `Guitar 3`.
fn take_name(audio_dir: &Path, track_name: &str) -> (String, String) {
    let stem: String = track_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let stem = if stem.is_empty() { "take".into() } else { stem };
    let mut n = 1;
    loop {
        let file = format!("{stem}-{n}.wav");
        if !audio_dir.join(&file).exists() {
            return (file, format!("{track_name} {n}"));
        }
        n += 1;
    }
}

/// Stops a track's recorder, waits for the writer to finalize the file
/// and returns its length in frames.
fn finish_take(live: &mut Live, take: &Recording, audio_dir: &Path) -> Result<u64> {
    let recorder = live
        .track_mut(take.track)?
        .recorder()
        .context("a drum track cannot record")?;
    let closed_before = recorder.stats().files_closed.load(Ordering::Relaxed);
    recorder.stop()?;
    let deadline = Instant::now() + TAKE_CLOSE_TIMEOUT;
    while recorder.stats().files_closed.load(Ordering::Relaxed) == closed_before {
        if Instant::now() > deadline {
            bail!("the take did not finish writing in time");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let info = wav::info(&audio_dir.join(&take.file))?;
    Ok(info.frames)
}

/// Where a take goes on the timeline: `correction` frames earlier than
/// the position it was recorded from. A take that would start before
/// zero is trimmed at the front instead; one entirely before zero is
/// dropped.
fn place_take(from: u64, frames: u64, correction: u64) -> Option<(u64, u64, u64)> {
    if from >= correction {
        return Some((from - correction, 0, frames));
    }
    let cut = correction - from;
    if cut >= frames {
        return None;
    }
    Some((0, cut, frames - cut))
}

/// `plugins/<id>.state`.
fn state_path(plugins_dir: &Path, id: PedalId) -> PathBuf {
    plugins_dir.join(format!("{id}.state"))
}

const STATE_MAGIC: &[u8; 4] = b"CNTS";

/// A state file: magic, the spec it belongs to, then the plugin's bytes.
fn encode_state(spec: &str, state: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + spec.len() + state.len());
    out.extend_from_slice(STATE_MAGIC);
    out.extend_from_slice(&(spec.len() as u32).to_le_bytes());
    out.extend_from_slice(spec.as_bytes());
    out.extend_from_slice(state);
    out
}

fn decode_state(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let rest = bytes.strip_prefix(STATE_MAGIC)?;
    let (len, rest) = rest.split_at_checked(4)?;
    let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
    let (spec, state) = rest.split_at_checked(len)?;
    Some((String::from_utf8(spec.to_vec()).ok()?, state.to_vec()))
}

/// Every readable state file in a project's `plugins/`.
fn read_plugin_states(project_dir: &Path) -> PluginStates {
    let mut states = PluginStates::new();
    let Ok(entries) = std::fs::read_dir(project_dir.join(PLUGINS_DIR)) else {
        return states;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "state") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if let Some(decoded) = std::fs::read(&path).ok().and_then(|b| decode_state(&b)) {
            states.insert(PedalId(id), decoded);
        }
    }
    states
}

/// Receives hardware events until the watcher is dropped.
fn spawn_watcher_thread(app: AppHandle, rx: mpsc::Receiver<DeviceEvent>) {
    thread::Builder::new()
        .name("device-watcher".into())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                // Coalesce a burst: CoreAudio fires several notifications
                // for one unplug.
                thread::sleep(Duration::from_millis(50));
                let mut last = event;
                while let Ok(next) = rx.try_recv() {
                    last = next;
                }
                app.state::<EngineState>().lock().on_device_event(last);
            }
        })
        .expect("spawning the device watcher thread");
}

/// Sends frames to every subscriber at display rate until stopped. A
/// subscriber that still holds the last frame is skipped and the frame
/// counted as dropped; a subscriber whose channel fails is removed.
/// Every few frames it also has the engine poll its plugins.
#[allow(clippy::too_many_arguments)]
fn spawn_meter_pump(
    app: AppHandle,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
    meters: Meters,
    readouts: Readouts,
    analysis: Arc<PumpAnalysis>,
    clock: cantina_engine::TransportClock,
    sample_rate: f64,
    state: Arc<TransportState>,
    dropped: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name("meter-pump".into())
        .spawn(move || {
            let period = Duration::from_secs(1) / METER_RATE;
            let mut sequence = 0u32;
            let mut bytes = Vec::new();
            let mut analyzer = Analyzer::new(sample_rate);
            let mut send = |meters: &[f32],
                            readouts: &[f32],
                            analyzer: &Analyzer,
                            playing: bool,
                            recording: bool| {
                let beat = playing.then(|| {
                    let heard = clock.position().saturating_sub(analysis.output_latency);
                    let tempo = analysis.tempo.lock().unwrap_or_else(|e| e.into_inner());
                    let b = beat_at(&tempo, heard, sample_rate);
                    FrameBeat {
                        bpm: b.bpm,
                        phase: b.phase,
                        bar: b.bar,
                        beat: b.beat,
                    }
                });
                let frame = Frame {
                    sequence,
                    dropped: dropped.load(Ordering::Relaxed),
                    position_seconds: clock.position() as f64 / sample_rate,
                    playing,
                    recording,
                    count_in: clock.count_in() as u32,
                    beat,
                    meters,
                    spectrum: &analyzer.spectrum,
                    waveform: &analyzer.waveform,
                    readouts,
                    analysis: Some(FrameAnalysis {
                        rms: analyzer.rms,
                        source: analysis.meter.load(Ordering::Relaxed),
                    }),
                };
                sequence = sequence.wrapping_add(1);
                let mut subs = subscribers.lock().unwrap_or_else(|e| e.into_inner());
                subs.retain(|sub| {
                    if sub.in_flight.swap(true, Ordering::AcqRel) {
                        dropped.fetch_add(1, Ordering::Relaxed);
                        return true;
                    }
                    frame.encode_into(&mut bytes);
                    sub.channel
                        .send(InvokeResponseBody::Raw(bytes.clone()))
                        .is_ok()
                });
            };
            let mut levels = Vec::new();
            let mut values = Vec::new();
            let mut frames = 0u32;
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(period);
                frames = frames.wrapping_add(1);
                if frames % PLUGIN_POLL_FRAMES == 0 {
                    if let Some(engine) = app.try_state::<EngineState>() {
                        engine.lock().poll_plugins();
                    }
                }
                levels.clear();
                for handle in meters.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                    // One pair per meter: the louder of its channels.
                    let (l, r) = (handle.take(0), handle.take(1));
                    levels.push(l.peak.max(r.peak));
                    levels.push(l.rms.max(r.rms));
                }
                values.clear();
                for readout in readouts.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                    values.extend(readout.values());
                }
                // The spectrum is worth working out only for a page that
                // will take the frame.
                let wanted = subscribers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .any(|sub| !sub.in_flight.load(Ordering::Acquire));
                if wanted {
                    analyzer.analyze(&analysis.scope);
                }
                send(
                    &levels,
                    &values,
                    &analyzer,
                    clock.is_playing(),
                    state.recording.load(Ordering::Relaxed),
                );
            }
            // Leave the meters at rest, even if that frame has to wait for
            // the UI's acknowledgement of the previous one.
            thread::sleep(period);
            for v in levels.iter_mut() {
                *v = 0.0;
            }
            analyzer.clear();
            send(&levels, &[], &analyzer, false, false);
        })
        .expect("spawning the meter pump thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_auditioned_groove_loops_from_its_bar_on_its_track_alone() {
        let (_, kit) =
            kit::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../kits/rock")).unwrap();
        let mut project = Project::new("P", 48_000.0);
        project
            .tracks
            .push(Track::drums(TrackId(1), "Drums", "rock"));
        project
            .tracks
            .push(Track::drums(TrackId(2), "More", "rock"));
        let mut groove = Pattern::new(PatternId(0), "Rock verse");
        groove.rows.push(cantina_project::PatternRow {
            pad: "kick".into(),
            steps: [vec![1.0], vec![0.0; 15]].concat(),
            ..Default::default()
        });
        let audition: Auditioning = Some((
            TrackId(1),
            Audition::Groove {
                path: "rock/Rock verse.pattern.json".into(),
                pattern: Arc::new(groove),
                from: 7680,
            },
        ));
        let on = score_for(&project.tracks[0], &project, &kit, audition.as_ref(), None);
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].start, 7680, "from the bar it was chosen on");
        assert_eq!(on[0].length, u64::MAX, "looping until stopped");
        assert!(score_for(&project.tracks[1], &project, &kit, audition.as_ref(), None).is_empty());
    }

    #[test]
    fn muted_and_soloed_rows_are_left_out_of_the_score() {
        let (_, kit) =
            kit::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../kits/rock")).unwrap();
        let mut project = Project::new("P", 48_000.0);
        project
            .tracks
            .push(Track::drums(TrackId(1), "Drums", "rock"));
        let mut beat = Pattern::new(PatternId(5), "Beat");
        for pad in ["kick", "snare", "hh-closed"] {
            beat.rows.push(cantina_project::PatternRow {
                pad: pad.into(),
                steps: [vec![1.0], vec![0.0; 15]].concat(),
                ..Default::default()
            });
        }
        project.patterns.push(beat);
        let audition: Auditioning = Some((TrackId(1), Audition::Pattern(PatternId(5))));
        let rows = |mix: &RowMix| {
            score_for(
                &project.tracks[0],
                &project,
                &kit,
                audition.as_ref(),
                Some(mix),
            )[0]
            .pattern
            .rows
            .len()
        };
        assert_eq!(rows(&RowMix::default()), 3);
        let muted = RowMix {
            muted: vec!["snare".into()],
            soloed: vec![],
        };
        assert_eq!(rows(&muted), 2);
        let soloed = RowMix {
            muted: vec![],
            soloed: vec!["kick".into()],
        };
        assert_eq!(rows(&soloed), 1);
        assert!(!soloed.plays("snare") && soloed.plays("kick"));
    }

    #[test]
    fn a_clip_with_a_fill_plays_it_on_its_fill_bars() {
        let (_, kit) =
            kit::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../kits/rock")).unwrap();
        let mut project = Project::new("P", 48_000.0);
        let mut drums = Track::drums(TrackId(1), "Drums", "rock");
        let bar = 3840;
        drums.pattern_clips.push(cantina_project::PatternClip {
            id: cantina_project::PatternClipId(4),
            pattern: PatternId(5),
            start: bar,
            offset: 0,
            length: bar * 8,
            fill: Some(cantina_project::Fill {
                pattern: PatternId(6),
                every: 4,
                at_end: true,
            }),
        });
        project.tracks.push(drums);
        project.patterns.push(Pattern::new(PatternId(5), "Beat"));
        project
            .patterns
            .push(Pattern::new(PatternId(6), "Beat fill"));
        let score = score_for(&project.tracks[0], &project, &kit, None, None);
        // Bars 1-3, the fill on bar 4, bars 5-7, the fill on bar 8.
        let spans: Vec<(u64, u64, u64)> = score
            .iter()
            .map(|p| (p.start, p.offset, p.length))
            .collect();
        assert_eq!(
            spans,
            vec![
                (bar, 0, 3 * bar),
                (4 * bar, 0, bar),
                (5 * bar, 4 * bar, 3 * bar),
                (8 * bar, 0, bar),
            ]
        );
        assert!(
            Arc::ptr_eq(&score[1].pattern, &score[3].pattern),
            "the same fill"
        );
        assert!(!Arc::ptr_eq(&score[0].pattern, &score[1].pattern));
    }

    #[test]
    fn the_status_carries_every_field_of_a_pattern_and_a_clip_back_to_the_ui() {
        let mut p = Pattern::new(PatternId(5), "Beat");
        p.humanize = Some(cantina_project::Humanize {
            timing: 0.3,
            velocity: 0.2,
        });
        p.seed = 9;
        let mut row = cantina_project::PatternRow::new("kick", vec![1.0; 16]);
        row.swing = Some(0.5);
        row.marks.push(cantina_project::StepMark {
            flam: true,
            ..cantina_project::StepMark::plain(0)
        });
        p.rows.push(row);
        // What the UI gets is what `pattern_replace` takes back: nothing lost.
        let json = serde_json::to_value(PatternInfo::of(&p)).unwrap();
        let back: Pattern = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
        let clip = cantina_project::PatternClip {
            id: cantina_project::PatternClipId(4),
            pattern: PatternId(5),
            start: 0,
            offset: 0,
            length: 3840,
            fill: Some(cantina_project::Fill {
                pattern: PatternId(6),
                every: 4,
                at_end: true,
            }),
        };
        let json = serde_json::to_value(PatternClipInfo::of(&clip)).unwrap();
        assert_eq!(json["fill"]["every"], 4);
        assert_eq!(json["fill"]["atEnd"], true);
    }

    #[test]
    fn open_recent_puts_the_latest_first_without_repeats() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(remember_recent(&[], "A"), names(&["A"]));
        assert_eq!(
            remember_recent(&names(&["A", "B", "C"]), "C"),
            names(&["C", "A", "B"])
        );
        let many: Vec<String> = (0..12).map(|i| i.to_string()).collect();
        let kept = remember_recent(&many, "new");
        assert_eq!(kept.len(), 10);
        assert_eq!(kept[0], "new");
    }

    #[test]
    fn imported_audio_is_fitted_to_the_project() {
        // Same rate and mono: untouched.
        let mono = vec![vec![0.5_f32; 100]];
        assert_eq!(fit_audio(mono.clone(), 48_000.0, 48_000.0), mono);
        // Another rate: resampled to the project's length.
        let fitted = fit_audio(vec![vec![0.0; 44_100]], 44_100.0, 48_000.0);
        assert_eq!(fitted[0].len(), 48_000);
        // Four channels: folded to stereo, the level kept.
        let quad = vec![vec![1.0_f32; 4]; 4];
        let folded = fit_audio(quad, 48_000.0, 48_000.0);
        assert_eq!(folded.len(), 2);
        assert!(folded
            .iter()
            .all(|c| c.iter().all(|x| (x - 1.0).abs() < 1e-6)));
    }

    #[test]
    fn takes_move_earlier_by_the_correction() {
        assert_eq!(place_take(1000, 500, 0), Some((1000, 0, 500)));
        assert_eq!(place_take(1000, 500, 300), Some((700, 0, 500)));
        assert_eq!(place_take(100, 500, 300), Some((0, 200, 300)));
        assert_eq!(place_take(0, 300, 300), None);
    }

    #[test]
    fn state_files_round_trip_and_reject_junk() {
        let bytes = encode_state("au:aufx:dely:appl", b"bplist00...");
        assert_eq!(
            decode_state(&bytes),
            Some(("au:aufx:dely:appl".to_string(), b"bplist00...".to_vec()))
        );
        assert!(decode_state(b"nope").is_none());
        assert!(decode_state(&bytes[..6]).is_none());

        let dir = std::env::temp_dir().join(format!("cantina-states-{}", std::process::id()));
        let plugins = dir.join(PLUGINS_DIR);
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(state_path(&plugins, PedalId(7)), &bytes).unwrap();
        std::fs::write(plugins.join("junk.state"), b"CNTS").unwrap();
        std::fs::write(plugins.join("8.state"), b"not a state").unwrap();
        let states = read_plugin_states(&dir);
        assert_eq!(states.len(), 1);
        assert_eq!(states[&PedalId(7)].0, "au:aufx:dely:appl");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn take_names_count_up_and_sanitize() {
        let dir = std::env::temp_dir().join(format!("cantina-takes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            take_name(&dir, "Guitar / lead"),
            ("Guitar---lead-1.wav".into(), "Guitar / lead 1".into())
        );
        std::fs::write(dir.join("Bass-1.wav"), b"").unwrap();
        assert_eq!(
            take_name(&dir, "Bass"),
            ("Bass-2.wav".into(), "Bass 2".into())
        );
        assert_eq!(
            take_name(&dir, "***"),
            ("take-1.wav".into(), "*** 1".into())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
