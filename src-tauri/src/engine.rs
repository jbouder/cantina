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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{ParamInfo, Unit, STOCK_NAMES};
use cantina_engine::strip::{GAIN, MUTE, PAN};
use cantina_engine::{
    device, processors, wav, Controller, DeviceEvent, DeviceId, Endpoint, GraphConfig, Meter,
    MeterHandle, Mute, NodeId, Player, PlayerHandle, Recorder, RecorderHandle, Runtime, Stream,
    StreamConfig, Strip, TempoMap, TimeSignature, TransportHandle, Watcher, MAX_BLOCK_FRAMES,
};
use cantina_project::{
    Clip, ClipId, Command, Pedal, PedalId, Project, Strip as StripModel, Tempo, Track, TrackId,
};
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
    /// Specs `chain_add` accepts.
    pub pedal_kinds: Vec<String>,
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
    pub sample_rate: f64,
    pub tempo: Tempo,
    pub tracks: Vec<TrackStatus>,
    /// The master strip; pan and solo are unused on it.
    pub master: StripModel,
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
    /// The pedal chain in order.
    pub chain: Vec<PedalInfo>,
    pub clips: Vec<ClipInfo>,
    pub strip: StripModel,
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

/// A pedal in a chain. Mirrors `PedalInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalInfo {
    /// Stable for the pedal's life, across stream rebuilds.
    pub id: u64,
    pub spec: String,
    pub name: String,
    pub params: Vec<ParamOut>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamOut {
    pub name: String,
    /// A unit suffix for display: "dB", "Hz", "s", "%" or "".
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub value: f32,
}

fn unit_label(unit: Unit) -> &'static str {
    match unit {
        Unit::Generic => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
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

/// The graph runs in stereo so strips can pan; the mono input is copied
/// to both channels on the way in.
const CHANNELS: usize = 2;

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
                .map(|(i, p)| ParamOut {
                    name: p.name.to_string(),
                    unit: unit_label(p.unit),
                    min: p.min,
                    max: p.max,
                    default: p.default,
                    value: pedal.values.get(i).copied().unwrap_or(p.default),
                })
                .collect(),
        }
    }
}

/// The nodes one track owns while the stream is open:
///
/// ```text
/// input -> mute -> bus -> pedals... -> strip -> (master)
///            |      ^                    |
///       recorder  player               meter
/// ```
struct LiveTrack {
    id: TrackId,
    /// The monitor gate: input passes when the track is armed.
    mute: NodeId,
    /// Where live input and playback meet before the pedals.
    bus: NodeId,
    pedals: Vec<(PedalId, NodeId)>,
    player_node: NodeId,
    player: PlayerHandle,
    recorder_node: NodeId,
    recorder: RecorderHandle,
    /// Gain, pan and mute after the pedals.
    strip: NodeId,
    /// Post-fader, a tap off the strip.
    meter: NodeId,
    levels: MeterHandle,
}

impl LiveTrack {
    fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        [
            self.mute,
            self.bus,
            self.player_node,
            self.recorder_node,
            self.strip,
            self.meter,
        ]
        .into_iter()
        .chain(self.pedals.iter().map(|(_, n)| *n))
    }
}

/// The meters the pump reads, in frame order: input, master, then one
/// per track in the project's order. Replaced whole on each sync.
type Meters = Arc<Mutex<Vec<MeterHandle>>>;

/// What is running while the stream is open.
struct Live {
    stream: Stream,
    controller: Controller,
    /// In the project's track order.
    tracks: Vec<LiveTrack>,
    /// The master strip: where the tracks sum before the output.
    master: NodeId,
    input_meter: NodeId,
    output_meter: NodeId,
    input_levels: MeterHandle,
    output_levels: MeterHandle,
    meters: Meters,
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
            .find(|(p, _)| *p == id)
            .map(|(_, n)| *n)
            .with_context(|| format!("pedal {id} is not in the graph"))
    }

    /// Brings the graph in line with the project: tracks and pedals are
    /// created, dropped and reordered to match, each player gets its
    /// track's clips, every gate follows its track's arm state, and the
    /// result is wired and committed. Nodes that survive keep their
    /// state. Clips that cannot load are reported but do not stop the
    /// rest.
    fn sync(&mut self, project: &Project, catalog: &mut Catalog, audio_dir: &Path) -> Result<()> {
        let sample_rate = self.stream.info().sample_rate;
        let mut problems = Vec::new();

        // Tracks that went away, with every node they held.
        let mut gone = Vec::new();
        self.tracks.retain(|t| {
            if project.track(t.id).is_some() {
                true
            } else {
                gone.extend(t.nodes());
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
                None => Self::create_track(&mut self.controller, track, sample_rate),
            };
            let graph = self.controller.graph_mut();
            // Pedals that left this chain.
            let mut removed = Vec::new();
            live.pedals.retain(|(id, node)| {
                if track.pedal(*id).is_some() {
                    true
                } else {
                    removed.push(*node);
                    false
                }
            });
            for node in removed {
                graph.remove(node)?;
            }
            // Pedals in the chain's order, creating the new ones.
            let mut pedals = Vec::with_capacity(track.chain.len());
            for pedal in &track.chain {
                let node = match live.pedals.iter().find(|(id, _)| *id == pedal.id) {
                    Some((_, node)) => *node,
                    None => {
                        let mut processor = processors::create(&pedal.spec, CHANNELS)?;
                        catalog.learn(&pedal.spec, processor.as_ref());
                        for (param, value) in pedal.values.iter().enumerate() {
                            processor.set_param(param, *value);
                        }
                        graph.add_processor(processor)
                    }
                };
                pedals.push((pedal.id, node));
            }
            live.pedals = pedals;
            self.controller
                .set_param(live.mute, 0, if track.armed { 1.0 } else { 0.0 });
            let clips = engine_clips(track, audio_dir);
            if live.player.clips() != clips.as_slice() {
                if let Err(e) = live.player.set_clips(clips) {
                    problems.push(format!("{}: {e:#}", track.name));
                }
            }
            ordered.push(live);
        }
        self.tracks = ordered;
        self.apply_strips(project);
        self.wire()?;
        self.controller.collect();
        let mut meters = vec![self.input_levels.clone(), self.output_levels.clone()];
        meters.extend(self.tracks.iter().map(|t| t.levels.clone()));
        *self.meters.lock().unwrap_or_else(|e| e.into_inner()) = meters;
        if problems.is_empty() {
            Ok(())
        } else {
            bail!("{}", problems.join("; "))
        }
    }

    fn create_track(controller: &mut Controller, track: &Track, sample_rate: f64) -> LiveTrack {
        let graph = controller.graph_mut();
        let mute = graph.add(Box::new(Mute::new(track.armed)));
        let bus = graph.add_processor(cantina_dsp::stock("gain").expect("gain is stock"));
        let (player, player_handle) = Player::new(sample_rate, CHANNELS);
        let player_node = graph.add(Box::new(player));
        // Takes are mono: the input is one channel.
        let (recorder, recorder_handle) = Recorder::new(sample_rate, 1);
        let recorder_node = graph.add(Box::new(recorder));
        let s = &track.strip;
        let strip = graph.add(Box::new(Strip::new(s.gain_db, s.pan, s.mute)));
        let (meter, levels) = Meter::new();
        let meter = graph.add(Box::new(meter));
        LiveTrack {
            id: track.id,
            mute,
            bus,
            pedals: Vec::new(),
            player_node,
            player: player_handle,
            recorder_node,
            recorder: recorder_handle,
            strip,
            meter,
            levels,
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

    /// Connects everything for the current tracks and commits:
    ///
    /// ```text
    /// input -> mute -> bus -> pedals... -> strip -> master -> output
    ///            |      ^                    |         |
    ///       recorder  player               meter   output meter
    /// input -> input meter
    /// ```
    fn wire(&mut self) -> Result<()> {
        let graph = self.controller.graph_mut();
        graph.clear_edges();
        graph.connect(Endpoint::Input, self.input_meter)?;
        for track in &self.tracks {
            graph.connect(Endpoint::Input, track.mute)?;
            graph.connect(track.mute, track.bus)?;
            graph.connect(track.mute, track.recorder_node)?;
            graph.connect(track.player_node, track.bus)?;
            let mut previous = track.bus;
            for (_, node) in &track.pedals {
                graph.connect(previous, *node)?;
                previous = *node;
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
        });
    }

    /// What `spec` builds, building one to find out the first time.
    fn get(&mut self, spec: &str) -> Result<&PedalKind> {
        if !self.0.contains_key(spec) {
            let processor = processors::create(spec, 1)?;
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
    catalog: Catalog,
    recording: Vec<Recording>,
    last_error: Option<String>,
    notice: Option<String>,
    notice_seq: u32,
    meter_subscribers: Arc<Mutex<Vec<Subscriber>>>,
    /// Shared with the meter pump so frames carry the transport state.
    transport_state: Arc<TransportState>,
    /// Frames skipped because the UI had not taken the previous one.
    frames_dropped: Arc<AtomicU32>,
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
        let mut last_error = None;
        let settings = match settings_path.as_deref().map(Settings::load) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                last_error = Some(format!("{e:#}"));
                Settings::default()
            }
            None => Settings::default(),
        };
        let projects = Projects::locate(&app);
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
        let mut engine = Self {
            app,
            live: None,
            settings,
            settings_path,
            projects,
            project,
            project_dir,
            dirty: false,
            catalog: Catalog::default(),
            recording: Vec::new(),
            last_error,
            notice: None,
            notice_seq: 0,
            meter_subscribers: Arc::new(Mutex::new(Vec::new())),
            transport_state: Arc::new(TransportState::default()),
            frames_dropped: Arc::new(AtomicU32::new(0)),
        };
        engine.adopt_sample_rate();
        engine.remember_project();
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
            Arc::clone(&self.meter_subscribers),
            Arc::new(Mutex::new(vec![input, output])),
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
            pedal_kinds: STOCK_NAMES.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn project_status(&self) -> ProjectStatus {
        let p = &self.project;
        ProjectStatus {
            name: p.name.clone(),
            dir: self.project_dir.display().to_string(),
            dirty: self.dirty,
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
                    chain: t.chain.iter().map(|p| self.catalog.describe(p)).collect(),
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
                    strip: t.strip.clone(),
                })
                .collect(),
            master: p.master.clone(),
        }
    }

    // ----- the project -----

    /// Applies an edit to the project and brings the graph in line.
    pub fn apply(&mut self, command: Command) -> Result<EngineStatus> {
        command
            .apply(&mut self.project)
            .map_err(|e| anyhow!("{e}"))?;
        self.dirty = true;
        self.react(&command)?;
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
                let mute = live.track(*id)?.mute;
                live.controller
                    .set_param(mute, 0, if *armed { 1.0 } else { 0.0 });
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
        let Some(live) = &mut self.live else {
            return Ok(());
        };
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        live.sync(&self.project, &mut self.catalog, &audio_dir)
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

    pub fn project_save(&mut self) -> Result<EngineStatus> {
        cantina_project::save(&self.project, &self.project_dir)?;
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
        self.project_dir = dir;
        self.dirty = false;
        self.remember_project();
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
            .filter(|t| t.armed)
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
        for (track, track_name) in armed {
            let (file, name) = take_name(&audio_dir, &track_name);
            live.track_mut(track)?
                .recorder
                .start(audio_dir.join(&file), from)?;
            self.recording.push(Recording {
                track,
                file,
                name,
                from,
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
                    let id = ClipId(self.project.fresh_id() + commands.len() as u64);
                    commands.push(Command::InsertClip {
                        track: take.track,
                        clip: Clip {
                            id,
                            name: take.name.clone(),
                            file: take.file.clone(),
                            start: take.from,
                            offset: 0,
                            length: frames,
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
        command
            .apply(&mut self.project)
            .map_err(|e| anyhow!("{e}"))?;
        self.dirty = true;
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
        let kind = self.catalog.get(spec)?;
        let pedal = Pedal {
            id: PedalId(self.project.fresh_id()),
            spec: spec.to_string(),
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
        let kind = self.catalog.get(&spec)?;
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
                    let _ = track.recorder.stop();
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
        spawn_meter_pump(
            Arc::clone(&self.meter_subscribers),
            Arc::clone(&meters),
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
            input_meter,
            output_meter,
            input_levels,
            output_levels,
            meters,
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
    )
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
    let track = live.track_mut(take.track)?;
    let closed_before = track.recorder.stats().files_closed.load(Ordering::Relaxed);
    track.recorder.stop()?;
    let deadline = Instant::now() + TAKE_CLOSE_TIMEOUT;
    while track.recorder.stats().files_closed.load(Ordering::Relaxed) == closed_before {
        if Instant::now() > deadline {
            bail!("the take did not finish writing in time");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let info = wav::info(&audio_dir.join(&take.file))?;
    Ok(info.frames)
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
#[allow(clippy::too_many_arguments)]
fn spawn_meter_pump(
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
    meters: Meters,
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
            let mut send = |meters: &[f32], playing: bool, recording: bool| {
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
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(period);
                levels.clear();
                for handle in meters.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                    // One pair per meter: the louder of its channels.
                    let (l, r) = (handle.take(0), handle.take(1));
                    levels.push(l.peak.max(r.peak));
                    levels.push(l.rms.max(r.rms));
                }
                send(
                    &levels,
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
            send(&levels, false, false);
        })
        .expect("spawning the meter pump thread");
}

#[cfg(test)]
mod tests {
    use super::*;

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
