//! The shell's handle on the audio engine.
//!
//! One [`Engine`] lives in Tauri managed state as [`EngineState`]. It owns
//! the stream, the graph controller, the chain and the meters, and
//! everything the UI can do to the engine goes through methods here,
//! called from `commands.rs` while the mutex is held.
//!
//! Two helper threads exist while a stream is open: one receives
//! hardware change events from the engine's [`Watcher`] and asks the
//! engine to rebuild when its devices go away or change rate, and one
//! pumps meter levels to the UI at display rate.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use cantina_dsp::{ParamInfo, Unit, STOCK_NAMES};
use cantina_engine::{
    device, processors, wav, Clip, Controller, DeviceEvent, DeviceId, Endpoint, GraphConfig, Meter,
    MeterHandle, Mute, NodeId, Player, PlayerHandle, Recorder, RecorderHandle, Runtime, Stream,
    StreamConfig, TransportHandle, Watcher, MAX_BLOCK_FRAMES,
};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};

use crate::frame::Frame;
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
    pub track: TrackStatus,
    /// The pedal chain in order.
    pub chain: Vec<PedalInfo>,
    /// Specs `chain_add` accepts.
    pub pedal_kinds: Vec<String>,
}

/// The one track, as the UI sees it. Mirrors `TrackStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackStatus {
    /// Armed tracks monitor their input and can record.
    pub armed: bool,
    pub recording: bool,
    pub playing: bool,
    /// The last take, ready to play.
    pub take: Option<TakeInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeInfo {
    pub path: String,
    pub name: String,
    pub seconds: f64,
}

/// A pedal in the chain. Mirrors `PedalInfo` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalInfo {
    /// Stable for the pedal's life, across stream rebuilds.
    pub id: u32,
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

/// A pedal in the chain, as the engine rebuilds it: its spec for
/// `processors::create`, what it reported about itself, and the last
/// value of each parameter.
#[derive(Debug, Clone)]
struct ChainItem {
    id: u32,
    spec: String,
    name: String,
    params: Vec<ParamInfo>,
    values: Vec<f32>,
}

impl ChainItem {
    /// Builds the processor once to learn its name and parameters.
    fn new(id: u32, spec: &str) -> Result<Self> {
        let processor = processors::create(spec, 1)?;
        let params = processor.params().to_vec();
        let values = params.iter().map(|p| p.default).collect();
        Ok(Self {
            id,
            spec: spec.to_string(),
            name: processor.name().to_string(),
            params,
            values,
        })
    }

    fn info(&self) -> PedalInfo {
        PedalInfo {
            id: self.id,
            spec: self.spec.clone(),
            name: self.name.clone(),
            params: self
                .params
                .iter()
                .zip(&self.values)
                .map(|(p, v)| ParamOut {
                    name: p.name.to_string(),
                    unit: unit_label(p.unit),
                    min: p.min,
                    max: p.max,
                    default: p.default,
                    value: *v,
                })
                .collect(),
        }
    }
}

/// The last recorded take.
#[derive(Debug, Clone)]
struct Take {
    path: PathBuf,
    frames: u64,
    sample_rate: f64,
}

/// What is running while the stream is open.
struct Live {
    stream: Stream,
    controller: Controller,
    /// Pedal nodes, in the order of `Engine::chain`.
    pedal_nodes: Vec<NodeId>,
    /// The monitor gate: input passes when the track is armed.
    mute: NodeId,
    /// Where live input and playback meet before the pedals.
    bus: NodeId,
    player_node: NodeId,
    player: PlayerHandle,
    recorder_node: NodeId,
    recorder: RecorderHandle,
    input_meter: NodeId,
    output_meter: NodeId,
    input: DeviceId,
    output: DeviceId,
    input_name: String,
    output_name: String,
    /// Dropping it removes the hardware listeners and ends their thread.
    _watcher: Watcher,
    pump_stop: Arc<AtomicBool>,
    /// Set while a recording runs; cleared by `track_stop`.
    recording: Option<PathBuf>,
}

impl Live {
    /// Connects everything for the current pedal order and commits:
    ///
    /// ```text
    /// input -> mute -> bus -> pedals... -> output
    ///            |      ^                    |
    ///       recorder  player           output meter
    /// input -> input meter
    /// ```
    fn wire(&mut self) -> Result<()> {
        let graph = self.controller.graph_mut();
        let mut chain = vec![self.mute, self.bus];
        chain.extend(&self.pedal_nodes);
        graph.set_chain(&chain)?;
        graph.connect(Endpoint::Input, self.input_meter)?;
        graph.connect(self.mute, self.recorder_node)?;
        graph.connect(self.player_node, self.bus)?;
        let last = *chain.last().expect("the chain holds at least the bus");
        graph.connect(last, self.output_meter)?;
        self.controller.commit()?;
        Ok(())
    }

    fn transport(&mut self) -> &mut TransportHandle {
        self.controller.transport_mut()
    }
}

/// How long `track_stop` waits for the writer to finalize the take.
const TAKE_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// The engine as the shell drives it.
pub struct Engine {
    app: AppHandle,
    live: Option<Live>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    chain: Vec<ChainItem>,
    next_pedal_id: u32,
    armed: bool,
    take: Option<Take>,
    takes_dir: Option<PathBuf>,
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

/// The M0 stock pedals, so the first launch makes a sound.
const DEFAULT_CHAIN: &[&str] = &["gain", "overdrive"];

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
        let takes_dir = app.path().app_data_dir().ok().map(|dir| dir.join("takes"));
        let mut next_pedal_id = 1;
        let chain = DEFAULT_CHAIN
            .iter()
            .filter_map(|spec| {
                let item = ChainItem::new(next_pedal_id, spec).ok()?;
                next_pedal_id += 1;
                Some(item)
            })
            .collect();
        let engine = Self {
            app,
            live: None,
            settings,
            settings_path,
            chain,
            next_pedal_id,
            armed: false,
            take: None,
            takes_dir,
            last_error,
            notice: None,
            notice_seq: 0,
            meter_subscribers: Arc::new(Mutex::new(Vec::new())),
            transport_state: Arc::new(TransportState::default()),
            frames_dropped: Arc::new(AtomicU32::new(0)),
        };
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
            input,
            output,
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
            track: TrackStatus {
                armed: self.armed,
                recording: self.live.as_ref().is_some_and(|l| l.recording.is_some()),
                playing: self
                    .live
                    .as_ref()
                    .is_some_and(|l| l.controller.transport().is_playing()),
                take: self.take.as_ref().map(|t| TakeInfo {
                    path: t.path.display().to_string(),
                    name: t
                        .path
                        .file_stem()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    seconds: t.frames as f64 / t.sample_rate,
                }),
            },
            chain: self.chain.iter().map(ChainItem::info).collect(),
            pedal_kinds: STOCK_NAMES.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ----- the track -----

    /// Arms or disarms the track. Armed, the input is monitored through
    /// the chain and recording is possible.
    pub fn track_arm(&mut self, armed: bool) -> EngineStatus {
        self.armed = armed;
        if let Some(live) = &mut self.live {
            live.controller
                .set_param(live.mute, 0, if armed { 1.0 } else { 0.0 });
        }
        self.publish()
    }

    /// Starts a new take from the top of the timeline. Needs an armed
    /// track and a running engine.
    pub fn track_record(&mut self) -> Result<EngineStatus> {
        if !self.armed {
            bail!("arm the track before recording");
        }
        let dir = self.takes_dir.clone().context("no place to keep takes")?;
        let Some(live) = &mut self.live else {
            bail!("start the engine before recording");
        };
        if live.recording.is_some() {
            return Ok(self.status());
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("take-{stamp}.wav"));
        // The previous take stays audible under the new one would be a
        // surprise on a one-track studio, so playback is cleared.
        live.player.set_clips(Vec::new())?;
        live.transport().locate(0);
        live.recorder.start(path.clone(), 0)?;
        live.transport().play();
        live.recording = Some(path);
        self.transport_state
            .recording
            .store(true, Ordering::Relaxed);
        Ok(self.publish())
    }

    /// Plays the last take from the top.
    pub fn track_play(&mut self) -> Result<EngineStatus> {
        let Some(live) = &mut self.live else {
            bail!("start the engine before playing");
        };
        if live.recording.is_some() {
            return Ok(self.status());
        }
        live.transport().locate(0);
        live.transport().play();
        Ok(self.publish())
    }

    /// Stops the transport, and a recording with it: the file is
    /// finalized and becomes the take ready for playback.
    pub fn track_stop(&mut self) -> Result<EngineStatus> {
        let Some(live) = &mut self.live else {
            return Ok(self.status());
        };
        live.transport().stop();
        if let Some(path) = live.recording.take() {
            self.transport_state
                .recording
                .store(false, Ordering::Relaxed);
            let closed_before = live.recorder.stats().files_closed.load(Ordering::Relaxed);
            live.recorder.stop()?;
            let deadline = Instant::now() + TAKE_CLOSE_TIMEOUT;
            while live.recorder.stats().files_closed.load(Ordering::Relaxed) == closed_before {
                if Instant::now() > deadline {
                    bail!("the take did not finish writing in time");
                }
                thread::sleep(Duration::from_millis(10));
            }
            let info = wav::info(&path)?;
            let take = Take {
                path: path.clone(),
                frames: info.frames,
                sample_rate: live.stream.info().sample_rate,
            };
            live.player.set_clips(vec![Clip {
                path,
                start: 0,
                offset: 0,
                length: info.frames,
            }])?;
            self.take = Some(take);
        }
        Ok(self.publish())
    }

    // ----- the chain -----

    /// Adds a pedal at the end of the chain, or at `index`.
    pub fn chain_add(&mut self, spec: &str, index: Option<usize>) -> Result<EngineStatus> {
        let item = ChainItem::new(self.next_pedal_id, spec)?;
        self.next_pedal_id += 1;
        let index = index.unwrap_or(self.chain.len()).min(self.chain.len());
        if let Some(live) = &mut self.live {
            let mut processor = processors::create(&item.spec, 1)?;
            for (param, value) in item.values.iter().enumerate() {
                processor.set_param(param, *value);
            }
            let node = live.controller.graph_mut().add_processor(processor);
            live.pedal_nodes.insert(index, node);
            live.wire()?;
        }
        self.chain.insert(index, item);
        Ok(self.publish())
    }

    pub fn chain_remove(&mut self, id: u32) -> Result<EngineStatus> {
        let index = self.pedal_index(id)?;
        if let Some(live) = &mut self.live {
            let node = live.pedal_nodes.remove(index);
            live.controller.graph_mut().remove(node)?;
            live.wire()?;
            live.controller.collect();
        }
        self.chain.remove(index);
        Ok(self.publish())
    }

    /// Moves a pedal to position `to` in the chain.
    pub fn chain_move(&mut self, id: u32, to: usize) -> Result<EngineStatus> {
        let from = self.pedal_index(id)?;
        let to = to.min(self.chain.len() - 1);
        if from != to {
            if let Some(live) = &mut self.live {
                let node = live.pedal_nodes.remove(from);
                live.pedal_nodes.insert(to, node);
                live.wire()?;
            }
            let item = self.chain.remove(from);
            self.chain.insert(to, item);
        }
        Ok(self.publish())
    }

    /// Sets a pedal parameter. Not published: knob drags are frequent and
    /// the UI already shows the value it sent.
    pub fn chain_set_param(&mut self, id: u32, param: usize, value: f32) -> Result<()> {
        let index = self.pedal_index(id)?;
        let item = &mut self.chain[index];
        let info = item
            .params
            .get(param)
            .with_context(|| format!("{} has no parameter {param}", item.name))?;
        let value = info.clamp(value);
        item.values[param] = value;
        if let Some(live) = &mut self.live {
            let node = live.pedal_nodes[index];
            if !live.controller.set_param(node, param, value) {
                bail!("the audio thread is not taking parameter changes");
            }
        }
        Ok(())
    }

    fn pedal_index(&self, id: u32) -> Result<usize> {
        self.chain
            .iter()
            .position(|p| p.id == id)
            .with_context(|| format!("no pedal {id} in the chain"))
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
    /// to match.
    pub fn set_audio_settings(&mut self, audio: AudioSettings) -> EngineStatus {
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
            self.settings.audio.sample_rate = rate;
            self.persist();
        }
        self.rebuild(Some(why));
    }

    /// Closes any open stream and opens one for the current settings,
    /// keeping the chain and its parameter values. `why` is the start of
    /// the notice shown to the user when the engine did this on its own.
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
            // promoted to the take, since it was cut short.
            if live.recording.take().is_some() {
                self.transport_state
                    .recording
                    .store(false, Ordering::Relaxed);
                let _ = live.recorder.stop();
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

    /// Opens a stream for the settings. Returns it and any fallback notice.
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
            channels: 1,
        })?;
        let graph = controller.graph_mut();
        let mut pedal_nodes = Vec::with_capacity(self.chain.len());
        for item in &self.chain {
            let mut processor = processors::create(&item.spec, 1)?;
            for (param, value) in item.values.iter().enumerate() {
                processor.set_param(param, *value);
            }
            pedal_nodes.push(graph.add_processor(processor));
        }
        let mute = graph.add(Box::new(Mute::new(self.armed)));
        let bus = graph.add_processor(cantina_dsp::stock("gain").expect("gain is stock"));
        let (player, mut player_handle) = Player::new(sample_rate, 1);
        let player_node = graph.add(Box::new(player));
        let (recorder, recorder_handle) = Recorder::new(sample_rate, 1);
        let recorder_node = graph.add(Box::new(recorder));
        let (input_meter, input_levels) = Meter::new();
        let (output_meter, output_levels) = Meter::new();
        let input_meter = graph.add(Box::new(input_meter));
        let output_meter = graph.add(Box::new(output_meter));
        // A take recorded at another rate cannot play here; it is kept on
        // disk and reported, but not loaded.
        if let Some(take) = &self.take {
            if take.sample_rate == sample_rate {
                player_handle.set_clips(vec![Clip {
                    path: take.path.clone(),
                    start: 0,
                    offset: 0,
                    length: take.frames,
                }])?;
            }
        }

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
        spawn_meter_pump(
            Arc::clone(&self.meter_subscribers),
            input_levels,
            output_levels,
            controller.transport().clock(),
            sample_rate,
            Arc::clone(&self.transport_state),
            Arc::clone(&self.frames_dropped),
            Arc::clone(&pump_stop),
        );

        let mut live = Live {
            stream,
            controller,
            pedal_nodes,
            mute,
            bus,
            player_node,
            player: player_handle,
            recorder_node,
            recorder: recorder_handle,
            input_meter,
            output_meter,
            input,
            output,
            input_name: device::name(input)?,
            output_name: device::name(output)?,
            _watcher: watcher,
            pump_stop,
            recording: None,
        };
        live.wire()?;
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
    input: MeterHandle,
    output: MeterHandle,
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
            let mut send = |meters: [f32; 4], playing: bool, recording: bool| {
                let frame = Frame {
                    sequence,
                    dropped: dropped.load(Ordering::Relaxed),
                    position_seconds: clock.position() as f64 / sample_rate,
                    playing,
                    recording,
                    beat: None,
                    meters: &meters,
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
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(period);
                let (i, o) = (input.take(0), output.take(0));
                send(
                    [i.peak, i.rms, o.peak, o.rms],
                    clock.is_playing(),
                    state.recording.load(Ordering::Relaxed),
                );
            }
            // Leave the meters at rest, even if that frame has to wait for
            // the UI's acknowledgement of the previous one.
            thread::sleep(period);
            send([0.0; 4], false, false);
        })
        .expect("spawning the meter pump thread");
}
