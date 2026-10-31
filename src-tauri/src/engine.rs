//! The shell's handle on the audio engine.
//!
//! One [`Engine`] lives in Tauri managed state as [`EngineState`]. It owns
//! the stream, the graph controller and the chain, and everything the UI
//! can do to the engine goes through methods here, called from
//! `commands.rs` while the mutex is held.

use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result};
use cantina_engine::{
    device, processors, Controller, GraphConfig, Runtime, Stream, StreamConfig, MAX_BLOCK_FRAMES,
};
use serde::Serialize;

/// Where the engine is, as the UI sees it. Mirrors `EngineStatus` in
/// `ui/src/lib/engine.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub sample_rate: f64,
    pub buffer_frames: u32,
    pub input_name: Option<String>,
    pub output_name: Option<String>,
    /// Why the last start or stop failed, if it did.
    pub error: Option<String>,
}

/// The managed-state wrapper. `Stream` and `Controller` are `Send` but not
/// `Sync`, so the mutex is what lets Tauri share them between commands.
#[derive(Default)]
pub struct EngineState(Mutex<Engine>);

impl EngineState {
    /// Locks the engine. A poisoned lock is recovered rather than
    /// propagated: the engine's state is simple enough that a panic in one
    /// command leaves nothing half-done.
    pub fn lock(&self) -> MutexGuard<'_, Engine> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What is running while the stream is open.
struct Live {
    stream: Stream,
    controller: Controller,
    input: device::DeviceId,
    output: device::DeviceId,
}

/// The engine as the shell drives it.
#[derive(Default)]
pub struct Engine {
    live: Option<Live>,
    sample_rate: Option<f64>,
    buffer_frames: Option<u32>,
    last_error: Option<String>,
}

const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;
const DEFAULT_BUFFER_FRAMES: u32 = 128;
/// The M0 stock pedals, until the UI builds chains itself.
const DEFAULT_CHAIN: &[&str] = &["gain", "overdrive"];

impl Engine {
    pub fn status(&self) -> EngineStatus {
        let names = |live: &Live| {
            (
                device::name(live.input).ok(),
                device::name(live.output).ok(),
            )
        };
        let (input_name, output_name) = self.live.as_ref().map(names).unwrap_or_default();
        EngineStatus {
            running: self.live.is_some(),
            sample_rate: self
                .live
                .as_ref()
                .map_or(self.sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE), |l| {
                    l.stream.info().sample_rate
                }),
            buffer_frames: self
                .live
                .as_ref()
                .map_or(self.buffer_frames.unwrap_or(DEFAULT_BUFFER_FRAMES), |l| {
                    l.stream.info().buffer_frames
                }),
            input_name,
            output_name,
            error: self.last_error.clone(),
        }
    }

    /// Opens the stream on the default devices with the default chain.
    /// Starting an engine that is already running is a no-op.
    pub fn start(&mut self) -> EngineStatus {
        if self.live.is_none() {
            match self.open() {
                Ok(live) => {
                    self.live = Some(live);
                    self.last_error = None;
                }
                Err(e) => self.last_error = Some(format!("{e:#}")),
            }
        }
        self.status()
    }

    /// Closes the stream. The old graph is dropped here, on this thread,
    /// never on the audio thread.
    pub fn stop(&mut self) -> EngineStatus {
        if let Some(mut live) = self.live.take() {
            live.stream.close();
            live.controller.collect();
        }
        self.status()
    }

    fn open(&self) -> Result<Live> {
        let input = device::default_input().context("no default input device")?;
        let output = device::default_output().context("no default output device")?;
        let sample_rate = self.sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE);
        let buffer_frames = self.buffer_frames.unwrap_or(DEFAULT_BUFFER_FRAMES);

        let (runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate,
            max_block_frames: MAX_BLOCK_FRAMES,
            channels: 1,
        })?;
        // The chain itself is wired by the track UI in a later issue; for
        // now the M0 pedals go in so starting the engine makes a sound.
        let mut chain = Vec::with_capacity(DEFAULT_CHAIN.len());
        for spec in DEFAULT_CHAIN {
            let processor = processors::create(spec, 1)?;
            chain.push(controller.graph_mut().add_processor(processor));
        }
        controller.graph_mut().set_chain(&chain)?;
        controller.commit()?;

        let stream = Stream::open(
            StreamConfig {
                input,
                output,
                sample_rate,
                buffer_frames,
                input_channel: 0,
            },
            runtime,
        )
        .context("opening the audio stream")?;

        Ok(Live {
            stream,
            controller,
            input,
            output,
        })
    }
}
