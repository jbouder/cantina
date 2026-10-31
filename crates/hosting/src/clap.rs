//! CLAP hosting through [`clack_host`].
//!
//! # Threading
//!
//! CLAP splits a plugin into a main-thread half and an audio-thread half,
//! and clack enforces that with types. The audio half, clack's
//! `PluginAudioProcessor`, lives in [`ClapProcessor`] and moves with it
//! between the UI thread and the audio thread as the graph swaps. The
//! main half, clack's `PluginInstance`, lives behind a mutex in
//! [`ClapShared`], which the processor and its [`ClapHandle`] both hold.
//! The processor locks it only in `prepare`, `reset` and `drop`, which run
//! off the audio thread; the handle locks it for state, latency and
//! parameter flushes. Nothing locks it on the audio thread.
//!
//! Parameter changes from the UI are delivered as CLAP events at the next
//! `process`, which is the sanctioned real-time path. Changes the plugin
//! makes on its own come back as output events from `process` and cross
//! to the handle over a lock-free ring, or, while the plugin is inactive,
//! through a main-thread flush when the plugin asks for one.

use std::any::Any;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{Buffer, Handle, ParamId, ParamInfo, Processor, StateError, Unit};
use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamInfoBuffer,
    ParamRescanFlags, PluginParams,
};
use clack_extensions::state::PluginState;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use rtrb::{Consumer, Producer, RingBuffer};

/// How many parameter changes can wait between two `process` calls.
const PENDING_EVENTS: usize = 256;
/// How many changes the plugin made can wait for the handle to collect.
const CHANGE_QUEUE: usize = 256;

/// Where macOS keeps CLAP bundles.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
    }
    paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    paths
}

/// `.clap` bundles directly inside the default search paths.
pub fn installed_bundles() -> Vec<PathBuf> {
    let mut bundles = Vec::new();
    for dir in default_search_paths() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "clap") {
                bundles.push(path);
            }
        }
    }
    bundles.sort();
    bundles
}

/// A plugin a bundle offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
}

/// Lists the plugins inside one bundle or library.
pub fn list_plugins(path: &Path) -> Result<Vec<PluginInfo>> {
    let entry = load_entry(path)?;
    Ok(describe(&entry))
}

fn load_entry(path: &Path) -> Result<PluginEntry> {
    // SAFETY: loading a plugin library runs its code. That is the deal with
    // in-process hosting; the scanner does it in a child process first.
    unsafe { PluginEntry::load(path) }
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("loading CLAP bundle {}", path.display()))
}

fn describe(entry: &PluginEntry) -> Vec<PluginInfo> {
    let Some(factory) = entry.get_plugin_factory() else {
        return Vec::new();
    };
    factory
        .plugin_descriptors()
        .filter_map(|d| {
            Some(PluginInfo {
                id: d.id()?.to_string_lossy().into_owned(),
                name: d
                    .name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                vendor: d
                    .vendor()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// Our side of the host: the plugin's requests land in flags the engine
/// can poll. Nothing here blocks.
#[derive(Default)]
pub struct Shared {
    pub restart_requested: AtomicBool,
    pub process_requested: AtomicBool,
    pub callback_requested: AtomicBool,
    /// The plugin wants a parameter flush while inactive.
    pub flush_requested: AtomicBool,
    /// The plugin's parameter list changed.
    pub rescan_requested: AtomicBool,
    /// The plugin's latency changed and it should be re-activated.
    pub latency_changed: AtomicBool,
}

impl SharedHandler<'_> for Shared {
    fn request_restart(&self) {
        self.restart_requested.store(true, Ordering::Relaxed);
    }

    fn request_process(&self) {
        self.process_requested.store(true, Ordering::Relaxed);
    }

    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Relaxed);
    }
}

impl HostParamsImplShared for Shared {
    fn request_flush(&self) {
        self.flush_requested.store(true, Ordering::Relaxed);
    }
}

/// The host's main-thread half: what the plugin tells us on our thread.
pub struct MainThreadHost<'a> {
    shared: &'a Shared,
}

impl<'a> MainThreadHost<'a> {
    pub fn new(shared: &'a Shared) -> Self {
        Self { shared }
    }
}

impl<'a> MainThreadHandler<'a> for MainThreadHost<'a> {}

impl HostParamsImplMainThread for MainThreadHost<'_> {
    fn rescan(&self, _flags: ParamRescanFlags) {
        self.shared.rescan_requested.store(true, Ordering::Relaxed);
    }

    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostLatencyImpl for MainThreadHost<'_> {
    fn changed(&self) {
        self.shared.latency_changed.store(true, Ordering::Relaxed);
    }
}

/// The handler set for every plugin instance.
pub struct CantinaHost;

impl HostHandlers for CantinaHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThreadHost<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostParams>().register::<HostLatency>();
    }
}

/// The main-thread half of a plugin, shared by its processor and its
/// handle. See the module docs for who locks it when.
pub struct ClapShared {
    // Drop order matters: the instance before the entry.
    instance: Mutex<PluginInstance<CantinaHost>>,
    _entry: PluginEntry,
}

// The instance is only ever used under the lock, and never on the audio
// thread, so one thread at a time holds it: CLAP's main-thread contract.
unsafe impl Send for ClapShared {}
unsafe impl Sync for ClapShared {}

impl ClapShared {
    fn lock(&self) -> MutexGuard<'_, PluginInstance<CantinaHost>> {
        self.instance.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A CLAP plugin as a [`Processor`].
pub struct ClapProcessor {
    shared: Arc<ClapShared>,
    processor: Option<PluginAudioProcessor<CantinaHost>>,
    info: PluginInfo,
    plugin_channels: usize,
    params: Vec<ParamInfo>,
    param_ids: Vec<ClapId>,
    values: Vec<f32>,
    pending: EventBuffer,
    pending_count: usize,
    output_events: EventBuffer,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    scratch_in: Vec<Vec<f32>>,
    scratch_out: Vec<Vec<f32>>,
    max_frames: usize,
    steady_time: u64,
    latency_frames: usize,
    process_errors: u64,
    /// Changes the plugin reported in its output events, for the handle.
    changes: Producer<(ParamId, f32)>,
    /// The handle's end of `changes`, until the handle is taken.
    changes_out: Option<Consumer<(ParamId, f32)>>,
}

impl ClapProcessor {
    /// Loads `path` and instantiates `plugin_id`, or the first plugin in the
    /// bundle when `plugin_id` is `None`. `channels` is what the chain will
    /// hand it; the plugin's own main port width is read from the bundle.
    pub fn load(path: &Path, plugin_id: Option<&str>, channels: usize) -> Result<Self> {
        if channels == 0 {
            bail!("channel count must be at least 1");
        }
        let entry = load_entry(path)?;
        let plugins = describe(&entry);
        let info = match plugin_id {
            Some(id) => plugins
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .with_context(|| format!("{} has no plugin {id:?}", path.display()))?,
            None => plugins
                .first()
                .cloned()
                .with_context(|| format!("{} contains no plugins", path.display()))?,
        };
        let host_info = HostInfo::new(
            "Cantina",
            "Cantina",
            "https://github.com/jbouder/cantina",
            env!("CARGO_PKG_VERSION"),
        )?;
        let id = CString::new(info.id.clone())?;
        let mut instance = PluginInstance::<CantinaHost>::new(
            |_| Shared::default(),
            |shared| MainThreadHost::new(shared),
            &entry,
            &id,
            &host_info,
        )
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("instantiating {}", info.name))?;

        let plugin_channels = main_port_channels(&mut instance).unwrap_or(2);
        let (params, param_ids, values) = read_params(&mut instance);
        let (changes, changes_out) = RingBuffer::new(CHANGE_QUEUE);

        Ok(Self {
            shared: Arc::new(ClapShared {
                instance: Mutex::new(instance),
                _entry: entry,
            }),
            processor: None,
            info,
            plugin_channels,
            params,
            param_ids,
            values,
            pending: EventBuffer::with_capacity(PENDING_EVENTS),
            pending_count: 0,
            output_events: EventBuffer::with_capacity(PENDING_EVENTS),
            input_ports: AudioPorts::with_capacity(plugin_channels, 1),
            output_ports: AudioPorts::with_capacity(plugin_channels, 1),
            scratch_in: Vec::new(),
            scratch_out: Vec::new(),
            max_frames: 0,
            steady_time: 0,
            latency_frames: 0,
            process_errors: 0,
            changes,
            changes_out: Some(changes_out),
        })
    }

    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    /// Channels on the plugin's main port.
    pub fn plugin_channels(&self) -> usize {
        self.plugin_channels
    }

    /// Process calls the plugin has failed. Non-zero means audio bypassed.
    pub fn process_errors(&self) -> u64 {
        self.process_errors
    }

    fn deactivate(&mut self) {
        if let Some(processor) = self.processor.take() {
            let stopped = processor.into_stopped();
            self.shared.lock().deactivate(stopped);
        }
    }

    fn activate(&mut self, sample_rate: f64, max_frames: usize) -> Result<()> {
        self.deactivate();
        let config = PluginAudioConfiguration {
            sample_rate,
            min_frames_count: 1,
            max_frames_count: max_frames as u32,
        };
        let mut instance = self.shared.lock();
        let processor = instance
            .activate(|_, _| (), config)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("activating {}", self.info.name))?;
        self.processor = Some(processor.into());
        self.scratch_in = vec![vec![0.0; max_frames]; self.plugin_channels];
        self.scratch_out = vec![vec![0.0; max_frames]; self.plugin_channels];
        self.max_frames = max_frames;
        self.steady_time = 0;
        self.latency_frames = latency_of(&mut instance);
        instance.access_shared_handler(|s| s.latency_changed.store(false, Ordering::Relaxed));
        Ok(())
    }

    /// Delivers queued parameter changes to an inactive plugin on the main
    /// thread, so `prepare` sees them.
    fn flush_inactive(&mut self) {
        if self.pending_count == 0 || self.processor.is_some() {
            return;
        }
        let mut instance = self.shared.lock();
        if let Some(params) = instance
            .plugin_shared_handle()
            .get_extension::<PluginParams>()
        {
            if let Some(mut handle) = instance.inactive_plugin_handle() {
                let input = self.pending.as_input();
                let mut output = self.output_events.as_output();
                params.flush(&mut handle, &input, &mut output);
            }
        }
        self.pending.clear();
        self.output_events.clear();
        self.pending_count = 0;
    }

    /// Forwards the parameter changes in the output events to the handle.
    /// Audio thread: no allocation; a full ring drops the oldest news.
    fn collect_output_changes(&mut self) {
        for event in &self.output_events {
            let Some(change) = event.as_event::<ParamValueEvent>() else {
                continue;
            };
            let Some(clap_id) = change.param_id() else {
                continue;
            };
            let Some(id) = self.param_ids.iter().position(|p| *p == clap_id) else {
                continue;
            };
            let value = change.value() as f32;
            self.values[id] = value;
            let _ = self.changes.push((id, value));
        }
    }
}

fn latency_of(instance: &mut PluginInstance<CantinaHost>) -> usize {
    instance
        .plugin_shared_handle()
        .get_extension::<PluginLatency>()
        .map(|ext| ext.get(&instance.plugin_handle()) as usize)
        .unwrap_or(0)
}

fn main_port_channels(instance: &mut PluginInstance<CantinaHost>) -> Option<usize> {
    let ports = instance
        .plugin_shared_handle()
        .get_extension::<PluginAudioPorts>()?;
    let handle = instance.plugin_handle();
    let mut buffer = AudioPortInfoBuffer::new();
    let info = ports.get(&handle, 0, true, &mut buffer)?;
    Some(info.channel_count as usize)
}

fn read_params(
    instance: &mut PluginInstance<CantinaHost>,
) -> (Vec<ParamInfo>, Vec<ClapId>, Vec<f32>) {
    let Some(params) = instance
        .plugin_shared_handle()
        .get_extension::<PluginParams>()
    else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let handle = instance.plugin_handle();
    let count = params.count(&handle);
    let mut infos = Vec::with_capacity(count as usize);
    let mut ids = Vec::with_capacity(count as usize);
    let mut values = Vec::with_capacity(count as usize);
    let mut buffer = ParamInfoBuffer::new();
    let mut text = [0u8; 64];
    for index in 0..count {
        let Some(info) = params.get_info(&handle, index, &mut buffer) else {
            continue;
        };
        let name = String::from_utf8_lossy(info.name).into_owned();
        // CLAP parameters carry no unit, but the plugin can format a
        // value; the suffix it writes is the unit.
        let formatted = params
            .value_to_text(&handle, info.id, info.default_value, &mut text)
            .ok()
            .map(|t| String::from_utf8_lossy(t).into_owned());
        let unit = formatted
            .as_deref()
            .and_then(unit_from_text)
            .unwrap_or_else(|| guess_unit(&name));
        infos.push(ParamInfo {
            name: name.into(),
            unit,
            min: info.min_value as f32,
            max: info.max_value as f32,
            default: info.default_value as f32,
        });
        ids.push(info.id);
        values.push(
            params
                .get_value(&handle, info.id)
                .map(|v| v as f32)
                .unwrap_or(info.default_value as f32),
        );
    }
    (infos, ids, values)
}

/// The unit a plugin's own formatting of a value ends with, if any.
pub fn unit_from_text(text: &str) -> Option<Unit> {
    let suffix = text
        .trim_end()
        .rsplit(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == ' ')
        .next()?
        .trim();
    match suffix.to_lowercase().as_str() {
        "db" => Some(Unit::Decibels),
        "hz" | "khz" => Some(Unit::Hertz),
        "s" | "sec" | "ms" => Some(Unit::Seconds),
        "%" => Some(Unit::Percent),
        _ => None,
    }
}

/// When the plugin cannot format a value, the name is the only hint.
fn guess_unit(name: &str) -> Unit {
    let lower = name.to_lowercase();
    if lower.contains("gain") || lower.contains("level") || lower.ends_with("db") {
        Unit::Decibels
    } else if lower.contains("freq") || lower.contains("cutoff") || lower.contains("hz") {
        Unit::Hertz
    } else if lower.contains("time") || lower.contains("attack") || lower.contains("release") {
        Unit::Seconds
    } else if lower.contains("mix") || lower.contains("%") {
        Unit::Percent
    } else {
        Unit::Generic
    }
}

/// Saves the plugin's state through the state extension, or the
/// parameter values when it has none.
fn save_state(instance: &mut PluginInstance<CantinaHost>, values: &[f32]) -> Vec<u8> {
    let Some(state) = instance
        .plugin_shared_handle()
        .get_extension::<PluginState>()
    else {
        return values.iter().flat_map(|v| v.to_le_bytes()).collect();
    };
    let mut bytes = Vec::new();
    let handle = instance.plugin_handle();
    if state.save(&handle, &mut bytes).is_err() {
        bytes.clear();
    }
    bytes
}

/// Restores state saved by [`save_state`]. Returns the plugin's values
/// afterwards when it has a state extension, so the caller can refresh
/// its cache.
fn load_state(
    instance: &mut PluginInstance<CantinaHost>,
    bytes: &[u8],
    param_count: usize,
) -> Result<Option<Vec<f32>>, StateError> {
    let shared = instance.plugin_shared_handle();
    let Some(state) = shared.get_extension::<PluginState>() else {
        let expected = param_count * 4;
        if bytes.len() != expected {
            return Err(StateError::WrongLength {
                expected,
                found: bytes.len(),
            });
        }
        return Ok(None);
    };
    let handle = instance.plugin_handle();
    let mut reader = bytes;
    state
        .load(&handle, &mut reader)
        .map_err(|e| StateError::Invalid(e.to_string()))?;
    let (_, _, values) = read_params(instance);
    Ok(Some(values))
}

impl Drop for ClapProcessor {
    fn drop(&mut self) {
        self.deactivate();
    }
}

impl Processor for ClapProcessor {
    fn name(&self) -> &str {
        &self.info.name
    }

    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn param(&self, id: ParamId) -> f32 {
        self.values.get(id).copied().unwrap_or(0.0)
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let (Some(&clap_id), Some(info)) = (self.param_ids.get(id), self.params.get(id)) else {
            return;
        };
        let value = info.clamp(value);
        self.values[id] = value;
        if self.pending_count < PENDING_EVENTS {
            let event = ParamValueEvent::new(0, clap_id, Pckn::match_all(), value as f64);
            self.pending.push(&event);
            self.pending_count += 1;
        }
    }

    fn latency(&self) -> usize {
        self.latency_frames
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        self.flush_inactive();
        if let Err(e) = self.activate(sample_rate, max_block_frames) {
            eprintln!("{}: {e:#}", self.info.name);
        }
    }

    fn reset(&mut self) {
        if let Some(processor) = self.processor.as_mut() {
            processor.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let Some(processor) = self.processor.as_mut() else {
            return;
        };
        if frames == 0 || frames > self.max_frames {
            return;
        }
        let Ok(started) = processor.ensure_processing_started() else {
            self.process_errors += 1;
            return;
        };

        let present = buffer.channels().min(self.plugin_channels);
        for ch in 0..self.plugin_channels {
            let source = buffer.channel(ch.min(present - 1));
            self.scratch_in[ch][..frames].copy_from_slice(&source[..frames]);
        }

        let input_events = self.pending.as_input();
        let mut output_events = self.output_events.as_output();
        let input_audio = self.input_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                self.scratch_in
                    .iter_mut()
                    .map(|c| InputChannel::variable(&mut c[..frames])),
            ),
        }]);
        let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                self.scratch_out.iter_mut().map(|c| &mut c[..frames]),
            ),
        }]);
        let result = started.process(
            &input_audio,
            &mut output_audio,
            &input_events,
            &mut output_events,
            Some(self.steady_time),
            None,
        );
        self.steady_time += frames as u64;
        self.pending.clear();
        self.pending_count = 0;
        self.collect_output_changes();
        self.output_events.clear();
        if result.is_err() {
            self.process_errors += 1;
            return;
        }
        for ch in 0..present {
            buffer.channel_mut(ch)[..frames].copy_from_slice(&self.scratch_out[ch][..frames]);
        }
    }

    fn state(&mut self) -> Vec<u8> {
        // Off the audio thread; see the module docs.
        save_state(&mut self.shared.lock(), &self.values)
    }

    fn set_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let refreshed = load_state(&mut self.shared.lock(), bytes, self.params.len())?;
        match refreshed {
            Some(values) if values.len() == self.values.len() => self.values = values,
            Some(_) => {}
            None => {
                for (id, chunk) in bytes.chunks_exact(4).enumerate() {
                    self.set_param(
                        id,
                        f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                    );
                }
            }
        }
        Ok(())
    }

    fn handle(&mut self) -> Option<Box<dyn Handle>> {
        let changes = self.changes_out.take()?;
        Some(Box::new(ClapHandle {
            shared: Arc::clone(&self.shared),
            info: self.info.clone(),
            params: self.params.clone(),
            param_ids: self.param_ids.clone(),
            values: self.values.clone(),
            changes,
            flush_output: EventBuffer::with_capacity(PENDING_EVENTS),
        }))
    }
}

/// The UI-thread side of a [`ClapProcessor`]. See the module docs.
pub struct ClapHandle {
    shared: Arc<ClapShared>,
    info: PluginInfo,
    params: Vec<ParamInfo>,
    param_ids: Vec<ClapId>,
    /// Values as last seen here.
    values: Vec<f32>,
    changes: Consumer<(ParamId, f32)>,
    flush_output: EventBuffer,
}

impl ClapHandle {
    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    /// The last value seen here for a parameter.
    pub fn param(&self, id: ParamId) -> f32 {
        self.values.get(id).copied().unwrap_or(0.0)
    }

    /// The plugin's main-thread half, for the editor.
    pub fn shared(&self) -> &Arc<ClapShared> {
        &self.shared
    }

    /// Whether the plugin asked to be re-activated because its latency
    /// changed. Cleared by the next `prepare`.
    pub fn latency_changed(&self) -> bool {
        self.shared
            .lock()
            .access_shared_handler(|s| s.latency_changed.load(Ordering::Relaxed))
    }

    /// Whether the plugin's parameter list changed under us.
    pub fn rescan_requested(&self) -> bool {
        self.shared
            .lock()
            .access_shared_handler(|s| s.rescan_requested.load(Ordering::Relaxed))
    }

    /// Runs a main-thread flush when the plugin asked for one and is
    /// inactive, collecting what it reports.
    fn flush_if_requested(&mut self, out: &mut Vec<(ParamId, f32)>) {
        let mut instance = self.shared.lock();
        let wanted =
            instance.access_shared_handler(|s| s.flush_requested.swap(false, Ordering::Relaxed));
        if !wanted || instance.is_active() {
            return;
        }
        let Some(params) = instance
            .plugin_shared_handle()
            .get_extension::<PluginParams>()
        else {
            return;
        };
        let Some(mut handle) = instance.inactive_plugin_handle() else {
            return;
        };
        let input = EventBuffer::new();
        let mut output = self.flush_output.as_output();
        params.flush(&mut handle, &input.as_input(), &mut output);
        for event in &self.flush_output {
            if let Some(change) = event.as_event::<ParamValueEvent>() {
                if let Some(id) = change
                    .param_id()
                    .and_then(|c| self.param_ids.iter().position(|p| *p == c))
                {
                    out.push((id, change.value() as f32));
                }
            }
        }
        self.flush_output.clear();
    }
}

impl Handle for ClapHandle {
    fn state(&mut self) -> Vec<u8> {
        save_state(&mut self.shared.lock(), &self.values)
    }

    fn set_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        let refreshed = load_state(&mut self.shared.lock(), state, self.params.len())?;
        if let Some(values) = refreshed {
            if values.len() == self.values.len() {
                self.values = values;
            }
        }
        Ok(())
    }

    fn latency(&self) -> usize {
        latency_of(&mut self.shared.lock())
    }

    fn take_changes(&mut self) -> Vec<(ParamId, f32)> {
        let mut changes = Vec::new();
        while let Ok(change) = self.changes.pop() {
            changes.push(change);
        }
        self.flush_if_requested(&mut changes);
        for (id, value) in &changes {
            if let Some(slot) = self.values.get_mut(*id) {
                *slot = *value;
            }
        }
        changes
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_come_from_formatted_values() {
        assert_eq!(unit_from_text("-6.0 dB"), Some(Unit::Decibels));
        assert_eq!(unit_from_text("440 Hz"), Some(Unit::Hertz));
        assert_eq!(unit_from_text("1.2 kHz"), Some(Unit::Hertz));
        assert_eq!(unit_from_text("12 ms"), Some(Unit::Seconds));
        assert_eq!(unit_from_text("50%"), Some(Unit::Percent));
        assert_eq!(unit_from_text("7 samples"), None);
        assert_eq!(unit_from_text("Off"), None);
        assert_eq!(guess_unit("Output Gain"), Unit::Decibels);
        assert_eq!(guess_unit("Rate"), Unit::Generic);
    }
}
