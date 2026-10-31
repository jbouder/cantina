//! CLAP hosting through [`clack_host`].
//!
//! # Threading
//!
//! CLAP splits a plugin into a main-thread half and an audio-thread half,
//! and clack enforces that with types. Cantina's [`Processor`] is one
//! object that moves wholesale between the UI thread and the audio thread,
//! so this wrapper is `Send` on the promise that main-thread operations
//! (`prepare`, `state`, `set_state`, parameter info) only run while the
//! processor is not installed in a running graph. Parameter changes on the
//! audio thread are delivered as CLAP events at the next `process`, which
//! is the sanctioned real-time path.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{Buffer, ParamId, ParamInfo, Processor, StateError, Unit};
use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::latency::PluginLatency;
use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_extensions::state::PluginState;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;

/// How many parameter changes can wait between two `process` calls.
const PENDING_EVENTS: usize = 256;

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
    // in-process hosting; scanning out of process is M4.
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

/// The handler set for every plugin instance.
pub struct CantinaHost;

impl HostHandlers for CantinaHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

/// A CLAP plugin as a [`Processor`].
pub struct ClapProcessor {
    // Drop order matters: the processor and instance before the entry.
    processor: Option<PluginAudioProcessor<CantinaHost>>,
    instance: PluginInstance<CantinaHost>,
    _entry: PluginEntry,
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
}

// See the module docs: main-thread operations only happen while the
// processor is not in a running graph.
unsafe impl Send for ClapProcessor {}

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
            |_| (),
            &entry,
            &id,
            &host_info,
        )
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("instantiating {}", info.name))?;

        let plugin_channels = main_port_channels(&mut instance).unwrap_or(2);
        let (params, param_ids, values) = read_params(&mut instance);

        Ok(Self {
            processor: None,
            instance,
            _entry: entry,
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
            self.instance.deactivate(stopped);
        }
    }

    fn activate(&mut self, sample_rate: f64, max_frames: usize) -> Result<()> {
        self.deactivate();
        let config = PluginAudioConfiguration {
            sample_rate,
            min_frames_count: 1,
            max_frames_count: max_frames as u32,
        };
        let processor = self
            .instance
            .activate(|_, _| (), config)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("activating {}", self.info.name))?;
        self.processor = Some(processor.into());
        self.scratch_in = vec![vec![0.0; max_frames]; self.plugin_channels];
        self.scratch_out = vec![vec![0.0; max_frames]; self.plugin_channels];
        self.max_frames = max_frames;
        self.steady_time = 0;
        self.latency_frames = self
            .instance
            .plugin_shared_handle()
            .get_extension::<PluginLatency>()
            .map(|ext| ext.get(&self.instance.plugin_handle()) as usize)
            .unwrap_or(0);
        Ok(())
    }

    /// Delivers queued parameter changes to an inactive plugin on the main
    /// thread, so `prepare` sees them.
    fn flush_inactive(&mut self) {
        if self.pending_count == 0 || self.processor.is_some() {
            return;
        }
        if let Some(params) = self
            .instance
            .plugin_shared_handle()
            .get_extension::<PluginParams>()
        {
            if let Some(mut handle) = self.instance.inactive_plugin_handle() {
                let input = self.pending.as_input();
                let mut output = self.output_events.as_output();
                params.flush(&mut handle, &input, &mut output);
            }
        }
        self.pending.clear();
        self.output_events.clear();
        self.pending_count = 0;
    }
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
    for index in 0..count {
        let Some(info) = params.get_info(&handle, index, &mut buffer) else {
            continue;
        };
        let name = String::from_utf8_lossy(info.name).into_owned();
        let unit = guess_unit(&name);
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

/// CLAP parameters carry no unit; the name is the only hint.
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
        self.output_events.clear();
        self.pending_count = 0;
        if result.is_err() {
            self.process_errors += 1;
            return;
        }
        for ch in 0..present {
            buffer.channel_mut(ch)[..frames].copy_from_slice(&self.scratch_out[ch][..frames]);
        }
    }

    fn state(&mut self) -> Vec<u8> {
        // Main thread only; see the module docs.
        let Some(state) = self
            .instance
            .plugin_shared_handle()
            .get_extension::<PluginState>()
        else {
            return (0..self.params.len())
                .flat_map(|id| self.param(id).to_le_bytes())
                .collect();
        };
        let mut bytes = Vec::new();
        let handle = self.instance.plugin_handle();
        if state.save(&handle, &mut bytes).is_err() {
            bytes.clear();
        }
        bytes
    }

    fn set_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let shared = self.instance.plugin_shared_handle();
        let Some(state) = shared.get_extension::<PluginState>() else {
            let expected = self.params.len() * 4;
            if bytes.len() != expected {
                return Err(StateError::WrongLength {
                    expected,
                    found: bytes.len(),
                });
            }
            for (id, chunk) in bytes.chunks_exact(4).enumerate() {
                let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                self.set_param(id, value);
            }
            return Ok(());
        };
        let handle = self.instance.plugin_handle();
        let mut reader = bytes;
        state
            .load(&handle, &mut reader)
            .map_err(|e| StateError::Invalid(e.to_string()))?;
        // The plugin's values changed under us; refresh the cache.
        let (_, _, values) = read_params(&mut self.instance);
        if values.len() == self.values.len() {
            self.values = values;
        }
        Ok(())
    }
}
