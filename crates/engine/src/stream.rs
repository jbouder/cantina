//! A duplex CoreAudio stream: input device to output device through a
//! [`BlockProcessor`], or an output-only one that hands it silence.
//!
//! # Real-time rules
//!
//! The input and render callbacks below run on the CoreAudio I/O thread.
//! They never allocate, lock, block, log or do I/O. They talk to the rest
//! of the program only through the lock-free ring buffer that carries
//! audio, atomics that carry parameters and counters, and memory that was
//! allocated before the stream started.
//!
//! When the input and output are the same device, one HAL output unit runs
//! both directions in a single I/O cycle, so the ring buffer between the
//! callbacks adds no latency. When they differ, two units run on
//! unsynchronized clocks and one block of input is held back so the output
//! never starves; drift between the clocks shows up in the counters.
//! With no input, one output unit runs alone and the processor's input is
//! silence, so playback works on a Mac with no input device at all.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::audio_unit_from_device_id_uninitialized;
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat};
use objc2_audio_toolbox::{
    kAudioOutputUnitProperty_CurrentDevice, kAudioOutputUnitProperty_EnableIO,
};
use rtrb::RingBuffer;

use crate::device::{self, DeviceId};

/// Largest block the render callback can handle without allocating. Devices
/// are configured for far less; this is the ceiling for scratch space.
pub const MAX_BLOCK_FRAMES: usize = 4096;

/// How to open a stream.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// The device to take input from, or `None` to run output only with
    /// silence as the input.
    pub input: Option<DeviceId>,
    /// The device to play to; the same as `input` for one duplex unit.
    pub output: DeviceId,
    /// The rate to run at; the device is switched to it.
    pub sample_rate: f64,
    /// The I/O buffer size to ask the device for, in frames.
    pub buffer_frames: u32,
    /// Zero-based input channel to take the signal from.
    pub input_channel: usize,
}

/// Mono-in processing that runs inside the render callback, with a mono
/// or a stereo output.
///
/// Implementations obey the real-time rules: `process` never allocates,
/// locks, blocks or performs I/O. Anything the processor needs is created
/// before it is handed to [`Stream::open`]. Talk to the rest of the program
/// through atomics and lock-free queues only.
pub trait BlockProcessor: Send + 'static {
    /// Produces `output` from `input`. Both slices are the same length,
    /// never longer than [`MAX_BLOCK_FRAMES`].
    fn process(&mut self, input: &[f32], output: &mut [f32]);

    /// Produces a stereo pair from `input`, when the output device has
    /// two or more channels. All three slices are the same length. The
    /// default is the mono output on both sides.
    fn process_stereo(&mut self, input: &[f32], left: &mut [f32], right: &mut [f32]) {
        self.process(input, left);
        right.copy_from_slice(left);
    }
}

impl<F> BlockProcessor for F
where
    F: FnMut(&[f32], &mut [f32]) + Send + 'static,
{
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self(input, output)
    }
}

/// What the stream actually opened with.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    /// The rate the stream runs at.
    pub sample_rate: f64,
    /// The buffer size the device granted, in frames.
    pub buffer_frames: u32,
    /// Channels the input device has; zero with no input.
    pub input_channels: u32,
    /// Channels the output device has.
    pub output_channels: u32,
    /// True when one audio unit runs both directions on one device.
    pub duplex: bool,
    /// True when the stream runs without an input device.
    pub output_only: bool,
    /// Frames of input the ring buffer holds back at start, zero in duplex.
    pub ring_prefill_frames: u32,
    /// Latency CoreAudio reports on the way in: the device's own, the
    /// safety offset and one buffer. Zero when the device does not say.
    pub input_latency_frames: u32,
    /// The same on the way out, plus the ring prefill when there is one.
    pub output_latency_frames: u32,
}

/// Counters the callbacks bump. Read from any thread.
#[derive(Debug, Default)]
pub struct Stats {
    /// Input callbacks so far.
    pub input_callbacks: AtomicU64,
    /// Output callbacks so far.
    pub output_callbacks: AtomicU64,
    /// Input frames dropped because the ring buffer was full.
    pub input_overruns: AtomicU64,
    /// Output frames filled with silence because the ring buffer was empty.
    pub output_underruns: AtomicU64,
    /// Blocks larger than the scratch buffer, which should never happen.
    pub oversized_blocks: AtomicU64,
    /// The largest block a callback has handed over.
    pub max_block_frames: AtomicU32,
}

/// An open duplex stream. Dropping it stops the audio units.
pub struct Stream {
    units: Units,
    stats: Arc<Stats>,
    info: StreamInfo,
}

enum Units {
    Duplex(AudioUnit),
    Output(AudioUnit),
    Split { input: AudioUnit, output: AudioUnit },
}

impl Stream {
    /// Opens and starts a stream running `processor`. Device sample rate and
    /// buffer size are set first so both units agree on the block size.
    pub fn open(config: StreamConfig, mut processor: impl BlockProcessor) -> Result<Stream> {
        let StreamConfig {
            input,
            output,
            sample_rate,
            buffer_frames,
            input_channel,
        } = config;

        if let Some(input) = input {
            device::set_sample_rate(input, sample_rate)?;
        }
        if input != Some(output) {
            device::set_sample_rate(output, sample_rate)?;
        }
        let in_frames = match input {
            Some(input) => Some(device::set_buffer_frame_size(input, buffer_frames)?),
            None => None,
        };
        let out_frames = match in_frames {
            Some(frames) if input == Some(output) => frames,
            _ => device::set_buffer_frame_size(output, buffer_frames)?,
        };
        if let Some(in_frames) = in_frames {
            if in_frames != out_frames {
                bail!("devices disagree on buffer size: input {in_frames}, output {out_frames}");
            }
        }
        let buffer_frames = out_frames;

        let input_channels = match input {
            Some(input) => {
                let channels = device::channel_count(input, true)?;
                if channels == 0 {
                    bail!("device {} has no inputs", device::name(input)?);
                }
                if input_channel >= channels as usize {
                    bail!("input channel {input_channel} is out of range; device has {channels}");
                }
                channels
            }
            None => 0,
        };
        let output_channels = device::channel_count(output, false)?.min(2);
        if output_channels == 0 {
            bail!("device {} has no outputs", device::name(output)?);
        }

        let duplex = input == Some(output);
        // The unit that renders, and the separate input unit when there is one.
        let (mut output_unit, mut input_unit) = match input {
            Some(input) if duplex => (duplex_unit(input)?, None),
            Some(input) => (
                audio_unit_from_device_id_uninitialized(output, false)?,
                Some(audio_unit_from_device_id_uninitialized(input, true)?),
            ),
            None => (
                audio_unit_from_device_id_uninitialized(output, false)?,
                None,
            ),
        };

        let output_format = StreamFormat {
            sample_rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT
                | LinearPcmFlags::IS_PACKED
                | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: output_channels,
        };
        output_unit
            .set_stream_format(output_format, Scope::Input, Element::Output)
            .context("setting output stream format")?;
        if input.is_some() {
            let input_format = StreamFormat {
                sample_rate,
                sample_format: SampleFormat::F32,
                flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED,
                channels: input_channels,
            };
            input_unit
                .as_mut()
                .unwrap_or(&mut output_unit)
                .set_stream_format(input_format, Scope::Output, Element::Input)
                .context("setting input stream format")?;
        }
        if let Some(unit) = input_unit.as_mut() {
            unit.initialize().context("initializing input unit")?;
        }
        output_unit
            .initialize()
            .context("initializing output unit")?;

        // Audio travels from the input callback to the render callback
        // through this ring. Capacity is generous so a late render never
        // drops input; latency is governed by how much is held back, not
        // by capacity. With no input nothing writes to it and the render
        // callback reads silence.
        let capacity = (buffer_frames as usize * 16).max(MAX_BLOCK_FRAMES * 2);
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(capacity);
        let ring_prefill_frames = if duplex || input.is_none() {
            0
        } else {
            buffer_frames
        };
        for _ in 0..ring_prefill_frames {
            producer.push(0.0).expect("ring has room for the prefill");
        }

        let stats = Arc::new(Stats::default());

        // --- input callback: real-time, see module docs ---
        if input.is_some() {
            let input_stats = Arc::clone(&stats);
            let channels = input_channels as usize;
            input_unit
                .as_mut()
                .unwrap_or(&mut output_unit)
                .set_input_callback(move |args: Args<data::Interleaved<f32>>| {
                    let Args {
                        num_frames, data, ..
                    } = args;
                    input_stats.input_callbacks.fetch_add(1, Ordering::Relaxed);
                    let writable = num_frames.min(producer.slots());
                    if let Ok(chunk) = producer.write_chunk_uninit(writable) {
                        let samples = data
                            .buffer
                            .iter()
                            .skip(input_channel)
                            .step_by(channels)
                            .copied();
                        chunk.fill_from_iter(samples);
                    }
                    if writable < num_frames {
                        input_stats
                            .input_overruns
                            .fetch_add((num_frames - writable) as u64, Ordering::Relaxed);
                    }
                    Ok(())
                })?;
        }

        // --- render callback: real-time, see module docs ---
        let render_stats = Arc::clone(&stats);
        let mut scratch_in = vec![0.0f32; MAX_BLOCK_FRAMES];
        let mut scratch_out = vec![0.0f32; MAX_BLOCK_FRAMES];
        let mut scratch_right = vec![0.0f32; MAX_BLOCK_FRAMES];
        let stereo = output_channels >= 2;
        let has_input = input.is_some();
        let render = move |args: Args<data::NonInterleaved<f32>>| {
            let Args {
                num_frames,
                mut data,
                ..
            } = args;
            render_stats
                .output_callbacks
                .fetch_add(1, Ordering::Relaxed);
            render_stats
                .max_block_frames
                .fetch_max(num_frames as u32, Ordering::Relaxed);

            if num_frames > MAX_BLOCK_FRAMES {
                render_stats
                    .oversized_blocks
                    .fetch_add(1, Ordering::Relaxed);
                for channel in data.channels_mut() {
                    channel.fill(0.0);
                }
                return Ok(());
            }
            let input = &mut scratch_in[..num_frames];
            let output = &mut scratch_out[..num_frames];

            let available = num_frames.min(consumer.slots());
            if let Ok(chunk) = consumer.read_chunk(available) {
                let (first, second) = chunk.as_slices();
                input[..first.len()].copy_from_slice(first);
                input[first.len()..available].copy_from_slice(second);
                chunk.commit_all();
            }
            if available < num_frames {
                input[available..].fill(0.0);
            }
            // Output only: the input is silence, not a starved ring.
            if has_input && available < num_frames {
                render_stats
                    .output_underruns
                    .fetch_add((num_frames - available) as u64, Ordering::Relaxed);
            }

            if stereo {
                let right = &mut scratch_right[..num_frames];
                processor.process_stereo(input, output, right);
                for (i, channel) in data.channels_mut().enumerate() {
                    channel.copy_from_slice(if i == 0 { &*output } else { &*right });
                }
            } else {
                processor.process(input, output);
                for channel in data.channels_mut() {
                    channel.copy_from_slice(output);
                }
            }
            Ok(())
        };

        output_unit.set_render_callback(render)?;
        let units = match input_unit {
            None if duplex => {
                output_unit.start().context("starting duplex unit")?;
                Units::Duplex(output_unit)
            }
            None => {
                output_unit.start().context("starting output unit")?;
                Units::Output(output_unit)
            }
            Some(mut input) => {
                input.start().context("starting input unit")?;
                output_unit.start().context("starting output unit")?;
                Units::Split {
                    input,
                    output: output_unit,
                }
            }
        };

        Ok(Stream {
            units,
            stats,
            info: StreamInfo {
                sample_rate,
                buffer_frames,
                input_channels,
                output_channels,
                duplex,
                output_only: input.is_none(),
                ring_prefill_frames,
                input_latency_frames: input
                    .and_then(|input| device::reported_latency(input, true).ok())
                    .map_or(0, |(d, s)| d + s + buffer_frames),
                output_latency_frames: device::reported_latency(output, false)
                    .map_or(0, |(d, s)| d + s + buffer_frames + ring_prefill_frames),
            },
        })
    }

    /// Counters the callbacks bump.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// What the stream opened with.
    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    /// Stops the stream. Dropping does the same.
    pub fn close(self) {
        drop(self);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        match &mut self.units {
            Units::Duplex(unit) | Units::Output(unit) => {
                let _ = unit.stop();
            }
            Units::Split { input, output } => {
                let _ = output.stop();
                let _ = input.stop();
            }
        }
    }
}

/// A HAL output unit with both input and output enabled on one device.
fn duplex_unit(device: DeviceId) -> Result<AudioUnit> {
    let mut unit = AudioUnit::new_uninitialized(IOType::HalOutput)?;
    let on = 1u32;
    unit.set_property(
        kAudioOutputUnitProperty_EnableIO,
        Scope::Input,
        Element::Input,
        Some(&on),
    )
    .context("enabling input on duplex unit")?;
    unit.set_property(
        kAudioOutputUnitProperty_EnableIO,
        Scope::Output,
        Element::Output,
        Some(&on),
    )
    .context("enabling output on duplex unit")?;
    unit.set_property(
        kAudioOutputUnitProperty_CurrentDevice,
        Scope::Global,
        Element::Output,
        Some(&device),
    )
    .context("binding duplex unit to device")?;
    Ok(unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the default output with no input and checks the render
    /// callback runs on silence. Needs a real output device, so it is
    /// run by hand: `cargo test -p cantina-engine stream -- --ignored`.
    #[test]
    #[ignore = "needs an audio output device"]
    fn an_output_only_stream_renders_without_an_input() {
        let output = device::default_output().expect("a default output device");
        let heard = Arc::new(AtomicU64::new(0));
        let loud = Arc::clone(&heard);
        let stream = Stream::open(
            StreamConfig {
                input: None,
                output,
                sample_rate: device::nominal_sample_rate(output).unwrap(),
                buffer_frames: 256,
                input_channel: 0,
            },
            move |input: &[f32], output: &mut [f32]| {
                if input.iter().any(|&s| s != 0.0) {
                    loud.fetch_add(1, Ordering::Relaxed);
                }
                output.fill(0.0);
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let info = stream.info().clone();
        let stats = stream.stats();
        assert!(info.output_only && !info.duplex);
        assert_eq!(info.input_channels, 0);
        assert_eq!(info.input_latency_frames, 0);
        assert_eq!(info.ring_prefill_frames, 0);
        assert!(stats.output_callbacks.load(Ordering::Relaxed) > 0);
        assert_eq!(stats.input_callbacks.load(Ordering::Relaxed), 0);
        assert_eq!(stats.output_underruns.load(Ordering::Relaxed), 0);
        assert_eq!(heard.load(Ordering::Relaxed), 0, "the input is silence");
    }

    /// The default input and output still open as before, one unit or
    /// two. Needs real devices, so it is run by hand.
    #[test]
    #[ignore = "needs audio input and output devices"]
    fn a_stream_with_an_input_still_takes_it() {
        let input = device::default_input().expect("a default input device");
        let output = device::default_output().expect("a default output device");
        let stream = Stream::open(
            StreamConfig {
                input: Some(input),
                output,
                sample_rate: device::nominal_sample_rate(output).unwrap(),
                buffer_frames: 256,
                input_channel: 0,
            },
            |_: &[f32], output: &mut [f32]| output.fill(0.0),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let info = stream.info();
        assert!(!info.output_only);
        assert!(info.input_channels > 0);
        assert_eq!(info.duplex, input == output);
        assert!(stream.stats().input_callbacks.load(Ordering::Relaxed) > 0);
        assert!(stream.stats().output_callbacks.load(Ordering::Relaxed) > 0);
    }
}
