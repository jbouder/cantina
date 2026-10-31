//! A duplex CoreAudio stream: input device to output device through a
//! [`BlockProcessor`].
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
//! unsynchronised clocks and one block of input is held back so the output
//! never starves; drift between the clocks shows up in the counters.

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
    pub input: DeviceId,
    pub output: DeviceId,
    pub sample_rate: f64,
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
    pub sample_rate: f64,
    pub buffer_frames: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    /// True when one audio unit runs both directions on one device.
    pub duplex: bool,
    /// Frames of input the ring buffer holds back at start, zero in duplex.
    pub ring_prefill_frames: u32,
}

/// Counters the callbacks bump. Read from any thread.
#[derive(Debug, Default)]
pub struct Stats {
    pub input_callbacks: AtomicU64,
    pub output_callbacks: AtomicU64,
    /// Input frames dropped because the ring buffer was full.
    pub input_overruns: AtomicU64,
    /// Output frames filled with silence because the ring buffer was empty.
    pub output_underruns: AtomicU64,
    /// Blocks larger than the scratch buffer, which should never happen.
    pub oversized_blocks: AtomicU64,
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

        device::set_sample_rate(input, sample_rate)?;
        if output != input {
            device::set_sample_rate(output, sample_rate)?;
        }
        let in_frames = device::set_buffer_frame_size(input, buffer_frames)?;
        let out_frames = if output != input {
            device::set_buffer_frame_size(output, buffer_frames)?
        } else {
            in_frames
        };
        if in_frames != out_frames {
            bail!("devices disagree on buffer size: input {in_frames}, output {out_frames}");
        }
        let buffer_frames = in_frames;

        let input_channels = device::channel_count(input, true)?;
        if input_channels == 0 {
            bail!("device {} has no inputs", device::name(input)?);
        }
        if input_channel >= input_channels as usize {
            bail!("input channel {input_channel} is out of range; device has {input_channels}");
        }
        let output_channels = device::channel_count(output, false)?.min(2);
        if output_channels == 0 {
            bail!("device {} has no outputs", device::name(output)?);
        }

        let duplex = input == output;
        let (mut input_unit, mut output_unit) = if duplex {
            (duplex_unit(input)?, None)
        } else {
            (
                audio_unit_from_device_id_uninitialized(input, true)?,
                Some(audio_unit_from_device_id_uninitialized(output, false)?),
            )
        };

        let input_format = StreamFormat {
            sample_rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED,
            channels: input_channels,
        };
        let output_format = StreamFormat {
            sample_rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT
                | LinearPcmFlags::IS_PACKED
                | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: output_channels,
        };
        input_unit
            .set_stream_format(input_format, Scope::Output, Element::Input)
            .context("setting input stream format")?;
        output_unit
            .as_mut()
            .unwrap_or(&mut input_unit)
            .set_stream_format(output_format, Scope::Input, Element::Output)
            .context("setting output stream format")?;
        input_unit.initialize().context("initialising input unit")?;
        if let Some(unit) = output_unit.as_mut() {
            unit.initialize().context("initialising output unit")?;
        }

        // Audio travels from the input callback to the render callback
        // through this ring. Capacity is generous so a late render never
        // drops input; latency is governed by how much is held back, not
        // by capacity.
        let capacity = (buffer_frames as usize * 16).max(MAX_BLOCK_FRAMES * 2);
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(capacity);
        let ring_prefill_frames = if duplex { 0 } else { buffer_frames };
        for _ in 0..ring_prefill_frames {
            producer.push(0.0).expect("ring has room for the prefill");
        }

        let stats = Arc::new(Stats::default());

        // --- input callback: real-time, see module docs ---
        let input_stats = Arc::clone(&stats);
        let channels = input_channels as usize;
        input_unit.set_input_callback(move |args: Args<data::Interleaved<f32>>| {
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

        // --- render callback: real-time, see module docs ---
        let render_stats = Arc::clone(&stats);
        let mut scratch_in = vec![0.0f32; MAX_BLOCK_FRAMES];
        let mut scratch_out = vec![0.0f32; MAX_BLOCK_FRAMES];
        let mut scratch_right = vec![0.0f32; MAX_BLOCK_FRAMES];
        let stereo = output_channels >= 2;
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

        let units = match output_unit {
            None => {
                input_unit.set_render_callback(render)?;
                input_unit.start().context("starting duplex unit")?;
                Units::Duplex(input_unit)
            }
            Some(mut output) => {
                output.set_render_callback(render)?;
                input_unit.start().context("starting input unit")?;
                output.start().context("starting output unit")?;
                Units::Split {
                    input: input_unit,
                    output,
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
                ring_prefill_frames,
            },
        })
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

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
            Units::Duplex(unit) => {
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
