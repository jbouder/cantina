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
use cantina_dsp::{Cabinet, Handle, LoadedKit, ParamInfo, Processor, Readout, Unit, STOCK_NAMES};
use cantina_engine::strip::{GAIN, MUTE, PAN};
use cantina_engine::{
    device, ir, kit, plan, processors, wav, Controller, Delay, DeviceEvent, DeviceId, Endpoint,
    GraphConfig, Impulse, IrEntry, KitEntry, Meter, MeterHandle, Mute, NodeId, PatternData,
    Placement, Player, PlayerHandle, Recorder, RecorderHandle, Runtime, Score, Sequencer,
    SequencerHandle, Stream, StreamConfig, Strip, TempoMap, TimeSignature, TransportHandle,
    Watcher, MAX_BLOCK_FRAMES,
};
use cantina_hosting::scan::{self, PluginList, Scanner};
use cantina_hosting::{AudioUnitHandle, ClapHandle, Editor};
use cantina_project::{
    Clip, ClipId, Command, History, Journal, JournalEntry, Pattern, PatternClip, PatternClipId,
    PatternId, PatternRow, Pedal, PedalId, Project, Strip as StripModel, Tempo, Track, TrackId,
    PLUGINS_DIR,
};
use objc2::MainThreadMarker;
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};

use crate::frame::Frame;
use crate::project::Projects;
use crate::settings::{AudioSettings, Settings};

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

/// A pattern looping on a drum track. Mirrors `AuditionStatus` in the UI.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditionStatus {
    pub track: u64,
    pub pattern: u64,
}

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
                ..
            } => vec![*mute, *player_node, *recorder_node],
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
}

impl LiveTrack {
    /// Frames the chain delays the signal by.
    fn latency(&self) -> usize {
        self.pedals.iter().map(|p| p.latency).sum()
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
    audition: Option<(TrackId, PatternId)>,
    audio_dir: &'a Path,
    states: &'a PluginStates,
}

/// The meters the pump reads, in frame order: input, master, then one
/// per track in the project's order. Replaced whole on each sync.
type Meters = Arc<Mutex<Vec<MeterHandle>>>;

/// The pedal readouts the pump reads, in track and chain order, as the
/// status lists them. Replaced whole on each sync.
type Readouts = Arc<Mutex<Vec<Readout>>>;

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
    input: DeviceId,
    output: DeviceId,
    input_name: String,
    output_name: String,
    /// Dropping it removes the hardware listeners and ends their thread.
    _watcher: Watcher,
    pump_stop: Arc<AtomicBool>,
}

impl Live {
    fn transport(&mut self) -> &mut TransportHandle {
        self.controller.transport_mut()
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
            audition,
            audio_dir,
            states,
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
            // old one's voices go with it.
            if let (Some(wanted), Source::Drums { kit: have, .. }) = (&track.kit, &live.source) {
                if wanted != have {
                    for node in live.source.nodes() {
                        self.controller.graph_mut().remove(node)?;
                    }
                    live.source =
                        Self::create_source(&mut self.controller, track, sample_rate, kits);
                }
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
                        let mut processor = build_processor(irs, &pedal.spec, CHANNELS)?;
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
                        }
                    }
                };
                pedals.push(live_pedal);
            }
            live.pedals = pedals;
            match &mut live.source {
                Source::Audio { mute, player, .. } => {
                    self.controller
                        .set_param(*mute, 0, if track.armed { 1.0 } else { 0.0 });
                    let clips = engine_clips(track, audio_dir);
                    if player.clips() != clips.as_slice() {
                        if let Err(e) = player.set_clips(clips) {
                            problems.push(format!("{}: {e:#}", track.name));
                        }
                    }
                }
                Source::Drums { sequencer, kit, .. } => {
                    let loaded = kits.get(kit);
                    let score = score_for(track, project, &loaded, audition);
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
                Source::Audio {
                    mute,
                    player_node,
                    player: player_handle,
                    recorder_node,
                    recorder: recorder_handle,
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
        audition: Option<(TrackId, PatternId)>,
    ) -> Result<()> {
        for live in &mut self.tracks {
            let Source::Drums { sequencer, kit, .. } = &mut live.source else {
                continue;
            };
            let Some(track) = project.track(live.id) else {
                continue;
            };
            let loaded = kits.get(kit);
            let score = score_for(track, project, &loaded, audition);
            if sequencer.score() != &score && !sequencer.set_score(score) {
                bail!("{}: the audio thread is not taking patterns", track.name);
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

    /// Connects everything for the current tracks and commits:
    ///
    /// ```text
    /// input -> mute -> bus -> pedals... -> strip -> master -> output
    ///            |      ^                    |         |
    ///       recorder  delay <- player      meter   output meter
    /// input -> input meter
    /// ```
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
                    ..
                } => {
                    graph.connect(Endpoint::Input, *mute)?;
                    graph.connect(*mute, track.bus)?;
                    graph.connect(*mute, *recorder_node)?;
                    graph.connect(*player_node, track.delay)?;
                }
                Source::Drums { sequencer_node, .. } => {
                    graph.connect(*sequencer_node, track.delay)?;
                }
            }
            graph.connect(track.delay, track.bus)?;
            let mut previous = track.bus;
            for pedal in &track.pedals {
                graph.connect(previous, pedal.node)?;
                previous = pedal.node;
            }
            graph.connect(previous, track.strip)?;
            graph.connect(track.strip, track.meter)?;
            graph.connect(track.strip, self.master)?;
        }
        graph.connect(self.master, self.output_meter)?;
        graph.connect(self.master, Endpoint::Output)?;
        self.controller.commit()?;
        Ok(())
    }
}

/// What a drum track plays: its pattern clips, or, while a pattern is
/// auditioned on it, that pattern alone looping from the start.
fn score_for(
    track: &Track,
    project: &Project,
    kit: &LoadedKit,
    audition: Option<(TrackId, PatternId)>,
) -> Score {
    if let Some((t, id)) = audition {
        if t == track.id {
            return project
                .pattern(id)
                .map(|p| vec![Placement::forever(pattern_data(p, kit))])
                .unwrap_or_default();
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
        let pattern = data
            .entry(clip.pattern)
            .or_insert_with(|| pattern_data(pattern, kit))
            .clone();
        score.push(Placement {
            pattern,
            start: clip.start,
            offset: clip.offset,
            length: clip.length,
        });
    }
    score
}

/// A pattern with its pads resolved against a kit. Rows for pads the
/// kit does not have are left out.
fn pattern_data(pattern: &Pattern, kit: &LoadedKit) -> Arc<PatternData> {
    Arc::new(PatternData {
        steps_per_beat: pattern.steps_per_beat as u32,
        length: pattern.length as u32,
        swing: pattern.swing,
        rows: pattern
            .rows
            .iter()
            .filter_map(|row| kit.pad_index(&row.pad).map(|i| (i, row.steps.clone())))
            .collect(),
    })
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

/// Builds a processor for a spec: a cabinet with its impulse response
/// loaded (`cab:<id>`), or whatever the engine's `processors` make.
fn build_processor(irs: &mut Irs, spec: &str, channels: usize) -> Result<Box<dyn Processor>> {
    if let Some(id) = spec.strip_prefix("cab:") {
        let impulse = irs.load(id)?;
        let mut cab = Cabinet::default();
        cab.set_impulse(&impulse.samples, impulse.sample_rate);
        return Ok(Box::new(cab));
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

/// The project's clips for one track as the player wants them.
fn engine_clips(track: &Track, audio_dir: &Path) -> Vec<cantina_engine::Clip> {
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
    fn get(&mut self, spec: &str, irs: &mut Irs) -> Result<&PedalKind> {
        if !self.0.contains_key(spec) {
            let processor = build_processor(irs, spec, 1)?;
            self.learn(spec, processor.as_ref());
        }
        Ok(&self.0[spec])
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
    /// The pattern looping on a drum track for editing. Session state,
    /// not part of the project.
    audition: Option<(TrackId, PatternId)>,
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
        if !kit_problems.is_empty() {
            last_error = Some(format!("kits: {}", kit_problems.join("; ")));
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
            kits,
            irs,
            audition: None,
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
        spawn_meter_pump(
            self.app.clone(),
            Arc::clone(&self.meter_subscribers),
            Arc::new(Mutex::new(vec![input, output])),
            Arc::new(Mutex::new(Vec::new())),
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
            pedal_kinds: stock_kinds().to_vec(),
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
            audition: self.audition.map(|(track, pattern)| AuditionStatus {
                track: track.0,
                pattern: pattern.0,
            }),
        }
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
                        })
                        .collect(),
                    pattern_clips: t
                        .pattern_clips
                        .iter()
                        .map(|c| PatternClipInfo {
                            id: c.id.0,
                            pattern: c.pattern.0,
                            start: c.start,
                            offset: c.offset,
                            length: c.length,
                        })
                        .collect(),
                    strip: t.strip.clone(),
                    latency_frames: t.chain.iter().map(|p| self.pedal_latency(p.id)).sum(),
                })
                .collect(),
            patterns: p
                .patterns
                .iter()
                .map(|q| PatternInfo {
                    id: q.id.0,
                    name: q.name.clone(),
                    steps_per_beat: q.steps_per_beat,
                    length: q.length,
                    swing: q.swing,
                    rows: q.rows.clone(),
                })
                .collect(),
            master: p.master.clone(),
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
            live.transport()
                .set_tempo_map(tempo_map(&self.project.tempo));
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
            Command::RenameTrack { .. } | Command::SetTempo { .. } => {
                if let Command::SetTempo { tempo } = command {
                    live.transport().set_tempo_map(tempo_map(tempo));
                }
                Ok(())
            }
            Command::SetGain { .. }
            | Command::SetPan { .. }
            | Command::SetMute { .. }
            | Command::SetSolo { .. } => {
                live.apply_strips(&self.project);
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
            | Command::SplitPatternClip { .. } => {
                self.check_audition();
                self.refresh_scores()
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

    /// Rebuilds the graph's structure from the project.
    fn sync(&mut self) -> Result<()> {
        self.check_audition();
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        let mut removed = Vec::new();
        let result = live.sync(
            SyncContext {
                project: &self.project,
                catalog: &mut self.catalog,
                kits: &mut self.kits,
                irs: &mut self.irs,
                audition: self.audition,
                audio_dir: &audio_dir,
                states: &self.plugin_states,
            },
            &mut removed,
        );
        self.close_editors(removed);
        result
    }

    /// Sends every drum track its score after a pattern edit.
    fn refresh_scores(&mut self) -> Result<()> {
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        live.refresh_scores(&self.project, &mut self.kits, self.audition)
    }

    /// Drops an audition whose track or pattern is gone.
    fn check_audition(&mut self) {
        if let Some((track, pattern)) = self.audition {
            let valid = self.project.track(track).is_some_and(|t| t.is_drums())
                && self.project.pattern(pattern).is_some();
            if !valid {
                self.audition = None;
            }
        }
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
                Some((TrackId(t), PatternId(p)))
            }
            _ => None,
        };
        self.refresh_scores()?;
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
        cantina_project::save(&self.project, &self.project_dir)?;
        self.journal.clear()?;
        self.recovery = None;
        self.dirty = false;
        Ok(self.publish())
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
                live.transport()
                    .set_tempo_map(tempo_map(&self.project.tempo));
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
        if self.settings.last_project.as_deref() != Some(self.project.name.as_str()) {
            self.settings.last_project = Some(self.project.name.clone());
            self.persist();
        }
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

    /// Starts a take on every armed track from the current position and
    /// rolls. Takes are written to `audio/` and become clips on stop.
    pub fn transport_record(&mut self) -> Result<EngineStatus> {
        let armed: Vec<(TrackId, String)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.armed && !t.is_drums())
            .map(|t| (t.id, t.name.clone()))
            .collect();
        if armed.is_empty() {
            bail!("arm a track before recording");
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
        live.transport().play();
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
                        },
                    });
                }
                Ok(_) => {}
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
        // A new cabinet starts on the first impulse installed.
        let spec = match (spec, self.irs.first_id()) {
            ("cab", Some(first)) => format!("cab:{first}"),
            _ => spec.to_string(),
        };
        let kind = self.catalog.get(&spec, &mut self.irs)?;
        let pedal = Pedal {
            id: PedalId(self.project.fresh_id()),
            spec: spec.clone(),
            values: kind.params.iter().map(|p| p.default).collect(),
        };
        self.apply(Command::InsertPedal {
            track: TrackId(track),
            index: index.unwrap_or(usize::MAX),
            pedal,
        })
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
        let kind = self.catalog.get(&spec, &mut self.irs)?;
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
        let graph = controller.graph_mut();
        let m = &self.project.master;
        let master = graph.add(Box::new(Strip::new(m.gain_db, 0.0, m.mute)));
        let (input_meter, input_levels) = Meter::new();
        let (output_meter, output_levels) = Meter::new();
        let input_meter = graph.add(Box::new(input_meter));
        let output_meter = graph.add(Box::new(output_meter));

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
        spawn_meter_pump(
            self.app.clone(),
            Arc::clone(&self.meter_subscribers),
            Arc::clone(&meters),
            Arc::clone(&readouts),
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
            input,
            output,
            input_name: device::name(input)?,
            output_name: device::name(output)?,
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

fn tempo_map(tempo: &Tempo) -> TempoMap {
    TempoMap::new(
        tempo.bpm,
        TimeSignature::new(tempo.numerator, tempo.denominator),
    )
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
            let mut send = |meters: &[f32], readouts: &[f32], playing: bool, recording: bool| {
                let frame = Frame {
                    sequence,
                    dropped: dropped.load(Ordering::Relaxed),
                    position_seconds: clock.position() as f64 / sample_rate,
                    playing,
                    recording,
                    beat: None,
                    meters,
                    spectrum: &[],
                    waveform: &[],
                    readouts,
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
                send(
                    &levels,
                    &values,
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
            send(&levels, &[], false, false);
        })
        .expect("spawning the meter pump thread");
}

#[cfg(test)]
mod tests {
    use super::*;

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
