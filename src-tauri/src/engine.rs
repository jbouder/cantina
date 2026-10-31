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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cantina_engine::{
    device, processors, Controller, DeviceEvent, DeviceId, Endpoint, GraphConfig, Level, Meter,
    MeterHandle, NodeId, Runtime, Stream, StreamConfig, Watcher, MAX_BLOCK_FRAMES,
};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};

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

/// One meter update. Mirrors `MeterFrame` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterFrame {
    pub input: LevelOut,
    pub output: LevelOut,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LevelOut {
    pub peak: f32,
    pub rms: f32,
}

impl From<Level> for LevelOut {
    fn from(l: Level) -> Self {
        Self {
            peak: l.peak,
            rms: l.rms,
        }
    }
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

/// A processor in the chain, as the engine rebuilds it: its spec for
/// `processors::create` and the last value of each parameter.
#[derive(Debug, Clone)]
struct ChainItem {
    spec: String,
    values: Vec<f32>,
}

/// What is running while the stream is open.
struct Live {
    stream: Stream,
    controller: Controller,
    chain_nodes: Vec<NodeId>,
    input: DeviceId,
    output: DeviceId,
    input_name: String,
    output_name: String,
    /// Dropping it removes the hardware listeners and ends their thread.
    _watcher: Watcher,
    pump_stop: Arc<AtomicBool>,
}

/// The engine as the shell drives it.
pub struct Engine {
    app: AppHandle,
    live: Option<Live>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    chain: Vec<ChainItem>,
    last_error: Option<String>,
    notice: Option<String>,
    notice_seq: u32,
    meter_subscribers: Arc<Mutex<Vec<Channel<MeterFrame>>>>,
}

/// The M0 stock pedals, until the track UI builds chains itself.
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
        Self {
            app,
            live: None,
            settings,
            settings_path,
            chain: DEFAULT_CHAIN
                .iter()
                .map(|spec| ChainItem {
                    spec: spec.to_string(),
                    values: Vec::new(),
                })
                .collect(),
            last_error,
            notice: None,
            notice_seq: 0,
            meter_subscribers: Arc::new(Mutex::new(Vec::new())),
        }
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
        }
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

    /// Adds a listener for meter frames. It is dropped when sending fails.
    pub fn subscribe_meters(&self, channel: Channel<MeterFrame>) {
        self.meter_subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(channel);
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
            // Remember where the knobs were for the next stream.
            for (item, node) in self.chain.iter_mut().zip(&live.chain_nodes) {
                if let Some(info) = live.controller.graph().node(*node) {
                    item.values = info.values.clone();
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
        let mut chain_nodes = Vec::with_capacity(self.chain.len());
        for item in &self.chain {
            let mut processor = processors::create(&item.spec, 1)?;
            for (param, value) in item.values.iter().enumerate() {
                processor.set_param(param, *value);
            }
            chain_nodes.push(graph.add_processor(processor));
        }
        graph.set_chain(&chain_nodes)?;
        // Meters tap the chain's input and its output as sinks.
        let (input_meter, input_levels) = Meter::new();
        let (output_meter, output_levels) = Meter::new();
        let input_meter = graph.add(Box::new(input_meter));
        let output_meter = graph.add(Box::new(output_meter));
        graph.connect(Endpoint::Input, input_meter)?;
        let last = chain_nodes
            .last()
            .map_or(Endpoint::Input, |n| Endpoint::Node(*n));
        graph.connect(last, output_meter)?;
        controller.commit()?;

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
            Arc::clone(&pump_stop),
        );

        Ok((
            Live {
                stream,
                controller,
                chain_nodes,
                input,
                output,
                input_name: device::name(input)?,
                output_name: device::name(output)?,
                _watcher: watcher,
                pump_stop,
            },
            fallback,
        ))
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

/// Sends meter frames to every subscriber at display rate until stopped.
fn spawn_meter_pump(
    subscribers: Arc<Mutex<Vec<Channel<MeterFrame>>>>,
    input: MeterHandle,
    output: MeterHandle,
    stop: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name("meter-pump".into())
        .spawn(move || {
            let period = Duration::from_secs(1) / METER_RATE;
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(period);
                let frame = MeterFrame {
                    input: input.take(0).into(),
                    output: output.take(0).into(),
                };
                let mut subs = subscribers.lock().unwrap_or_else(|e| e.into_inner());
                subs.retain(|channel| channel.send(frame.clone()).is_ok());
            }
            // Leave the meters at rest.
            let silent = MeterFrame {
                input: Level::default().into(),
                output: Level::default().into(),
            };
            let mut subs = subscribers.lock().unwrap_or_else(|e| e.into_inner());
            subs.retain(|channel| channel.send(silent.clone()).is_ok());
        })
        .expect("spawning the meter pump thread");
}
