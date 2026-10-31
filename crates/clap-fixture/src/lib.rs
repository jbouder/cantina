//! A deliberately small CLAP effect for exercising the host: stereo in and
//! out, a `Gain` parameter in decibels, and a `Delay` parameter in samples
//! that it both applies and reports as latency. Saved state is the two
//! values as little-endian `f32`s.
//!
//! The bundle carries a second plugin, [`CRASH_ID`], that aborts the
//! process the moment a host instantiates it. It exists so the scanner's
//! child process has something real to die on; listing it is harmless.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::entry::prelude::*;
use clack_plugin::events::event_types::ParamValueEvent;
use clack_plugin::prelude::*;
use clack_plugin::process::audio::ChannelPair;
use clack_plugin::stream::{InputStream, OutputStream};

pub const ID: &str = "com.cantina.test-gain";
pub const NAME: &str = "Cantina Test Gain";
pub const VENDOR: &str = "Cantina";
/// A plugin that aborts on instantiation.
pub const CRASH_ID: &str = "com.cantina.test-crash";
pub const CRASH_NAME: &str = "Cantina Test Crash";
pub const MAX_DELAY: usize = 64;
const GAIN: u32 = 0;
const DELAY: u32 = 1;
const CHANNELS: usize = 2;

pub struct TestGain;

/// Parameter values live here so every thread sees the same numbers.
#[derive(Default)]
pub struct Shared {
    gain_db_bits: AtomicU32,
    delay: AtomicU32,
}

impl Shared {
    fn new() -> Self {
        Self {
            gain_db_bits: AtomicU32::new(0f32.to_bits()),
            delay: AtomicU32::new(7),
        }
    }

    fn gain_db(&self) -> f32 {
        f32::from_bits(self.gain_db_bits.load(Ordering::Relaxed))
    }

    fn delay(&self) -> usize {
        self.delay.load(Ordering::Relaxed) as usize
    }

    fn apply(&self, events: &InputEvents) {
        for event in events {
            if let Some(e) = event.as_event::<ParamValueEvent>() {
                match e.param_id().map(|id| id.get()) {
                    Some(GAIN) => self.gain_db_bits.store(
                        (e.value() as f32).clamp(-60.0, 24.0).to_bits(),
                        Ordering::Relaxed,
                    ),
                    Some(DELAY) => self.delay.store(
                        (e.value().round().clamp(0.0, MAX_DELAY as f64)) as u32,
                        Ordering::Relaxed,
                    ),
                    _ => {}
                }
            }
        }
    }
}

impl PluginShared<'_> for Shared {}

pub struct MainThread<'a> {
    shared: &'a Shared,
}

impl<'a> PluginMainThread<'a, Shared> for MainThread<'a> {}

pub struct Processor<'a> {
    shared: &'a Shared,
    history: Vec<[f32; MAX_DELAY]>,
    write: usize,
}

impl Plugin for TestGain {
    type AudioProcessor<'a> = Processor<'a>;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginLatency>()
            .register::<PluginState>();
    }
}

impl TestGain {
    fn new_shared(_host: HostSharedHandle<'_>) -> Result<Shared, PluginError> {
        Ok(Shared::new())
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a Shared,
    ) -> Result<MainThread<'a>, PluginError> {
        Ok(MainThread { shared })
    }
}

/// Aborts when instantiated. See the module docs.
pub struct Crash;

impl Plugin for Crash {
    type AudioProcessor<'a> = ();
    type Shared<'a> = ();
    type MainThread<'a> = ();
}

pub struct Entry {
    factory: PluginFactoryWrapper<Factory>,
}

pub struct Factory {
    gain: PluginDescriptor,
    crash: PluginDescriptor,
}

impl clack_plugin::entry::Entry for Entry {
    fn new(_bundle_path: Option<&std::ffi::CStr>) -> Result<Self, EntryLoadError> {
        Ok(Self {
            factory: PluginFactoryWrapper::new(Factory {
                gain: PluginDescriptor::new(ID, NAME)
                    .with_vendor(VENDOR)
                    .with_version(env!("CARGO_PKG_VERSION")),
                crash: PluginDescriptor::new(CRASH_ID, CRASH_NAME)
                    .with_vendor(VENDOR)
                    .with_version(env!("CARGO_PKG_VERSION")),
            }),
        })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
    }
}

impl PluginFactoryImpl for Factory {
    fn plugin_count(&self) -> u32 {
        2
    }

    fn plugin_descriptor(&self, index: u32) -> Option<&PluginDescriptor> {
        match index {
            0 => Some(&self.gain),
            1 => Some(&self.crash),
            _ => None,
        }
    }

    fn create_plugin<'a>(
        &'a self,
        host_info: HostInfo<'a>,
        plugin_id: &std::ffi::CStr,
    ) -> Option<PluginInstance<'a>> {
        if plugin_id == self.gain.id().unwrap_or_default() {
            Some(PluginInstance::new::<TestGain>(
                host_info,
                &self.gain,
                TestGain::new_shared,
                TestGain::new_main_thread,
            ))
        } else if plugin_id == self.crash.id().unwrap_or_default() {
            eprintln!("{CRASH_NAME}: aborting on purpose");
            std::process::abort();
        } else {
            None
        }
    }
}

impl<'a> PluginAudioProcessor<'a, Shared, MainThread<'a>> for Processor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &MainThread<'a>,
        shared: &'a Shared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            history: vec![[0.0; MAX_DELAY]; CHANNELS],
            write: 0,
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        self.shared.apply(events.input);
        let gain = 10f32.powf(self.shared.gain_db() / 20.0);
        let delay = self.shared.delay();

        for mut port in audio.port_pairs() {
            let mut channels = port
                .channels()
                .map_err(|_| PluginError::Message("unexpected buffer layout"))?
                .into_f32()
                .ok_or(PluginError::Message("only f32 audio is supported"))?;
            let mut write = self.write;
            for (ch, pair) in channels.iter_mut().enumerate().take(CHANNELS) {
                write = self.write;
                let history = &mut self.history[ch];
                match pair {
                    ChannelPair::InPlace(io) => {
                        for sample in io.iter_mut() {
                            *sample = step(history, &mut write, delay, gain, *sample);
                        }
                    }
                    ChannelPair::InputOutput(input, output) => {
                        for (x, y) in input.iter().zip(output.iter_mut()) {
                            *y = step(history, &mut write, delay, gain, *x);
                        }
                    }
                    ChannelPair::OutputOnly(output) => {
                        for y in output.iter_mut() {
                            *y = step(history, &mut write, delay, gain, 0.0);
                        }
                    }
                    ChannelPair::InputOnly(_) => {}
                }
            }
            self.write = write;
        }
        Ok(ProcessStatus::Continue)
    }

    fn reset(&mut self) {
        for h in &mut self.history {
            h.fill(0.0);
        }
    }
}

/// Pushes one sample through the delay line and returns the output.
#[inline]
fn step(history: &mut [f32; MAX_DELAY], write: &mut usize, delay: usize, gain: f32, x: f32) -> f32 {
    history[*write % MAX_DELAY] = x;
    let read = (*write + MAX_DELAY - delay) % MAX_DELAY;
    *write += 1;
    history[read] * gain
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, _is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: b"main",
                channel_count: CHANNELS as u32,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&self) -> u32 {
        2
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        match index {
            GAIN => info.set(&ParamInfo {
                id: ClapId::new(GAIN),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Gain",
                module: b"",
                min_value: -60.0,
                max_value: 24.0,
                default_value: 0.0,
            }),
            DELAY => info.set(&ParamInfo {
                id: ClapId::new(DELAY),
                flags: ParamInfoFlags::IS_AUTOMATABLE | ParamInfoFlags::IS_STEPPED,
                cookie: Default::default(),
                name: b"Delay",
                module: b"",
                min_value: 0.0,
                max_value: MAX_DELAY as f64,
                default_value: 7.0,
            }),
            _ => {}
        }
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        match id.get() {
            GAIN => Some(self.shared.gain_db() as f64),
            DELAY => Some(self.shared.delay() as f64),
            _ => None,
        }
    }

    fn value_to_text(
        &self,
        id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        use std::fmt::Write;
        match id.get() {
            GAIN => write!(writer, "{value:.1} dB"),
            DELAY => write!(writer, "{} samples", value.round()),
            _ => Err(std::fmt::Error),
        }
    }

    fn text_to_value(&self, _id: ClapId, text: &std::ffi::CStr) -> Option<f64> {
        text.to_str().ok()?.split_whitespace().next()?.parse().ok()
    }

    fn flush(&self, input: &InputEvents, _output: &mut OutputEvents) {
        self.shared.apply(input);
    }
}

impl PluginAudioProcessorParams for Processor<'_> {
    fn flush(&mut self, input: &InputEvents, _output: &mut OutputEvents) {
        self.shared.apply(input);
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.delay() as u32
    }
}

impl PluginStateImpl for MainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        output.write_all(&self.shared.gain_db().to_le_bytes())?;
        output.write_all(&(self.shared.delay() as f32).to_le_bytes())?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut bytes = [0u8; 8];
        input.read_exact(&mut bytes)?;
        let gain = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let delay = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        self.shared
            .gain_db_bits
            .store(gain.to_bits(), Ordering::Relaxed);
        self.shared
            .delay
            .store(delay.round() as u32, Ordering::Relaxed);
        Ok(())
    }
}

clack_export_entry!(Entry);
