//! CoreAudio device enumeration and configuration.
//!
//! Nothing here runs on the audio thread. These calls may block briefly
//! while the HAL talks to a driver.

use std::mem::{size_of, MaybeUninit};
use std::ptr::{null, NonNull};

use anyhow::{anyhow, bail, Context, Result};
use coreaudio::audio_unit::macos_helpers;
use objc2_core_audio::{
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioDevicePropertyLatency, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertySafetyOffset, kAudioDevicePropertyStreamConfiguration,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectPropertyScope, AudioObjectSetPropertyData,
};
use objc2_core_audio_types::{AudioBufferList, AudioValueRange};

/// A CoreAudio device identifier. Stable for the life of the device
/// connection, not across unplug and replug.
pub type DeviceId = AudioObjectID;

/// What the UI needs to know about a device to offer it in a picker.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub input_channels: u32,
    pub output_channels: u32,
    pub sample_rate: f64,
    pub buffer_frames: u32,
}

/// Every device CoreAudio knows about, with channel counts and the current
/// sample rate and buffer size.
pub fn list() -> Result<Vec<DeviceInfo>> {
    let ids = macos_helpers::get_audio_device_ids().context("listing audio devices")?;
    ids.into_iter().map(info).collect()
}

/// Describes one device.
pub fn info(id: DeviceId) -> Result<DeviceInfo> {
    Ok(DeviceInfo {
        id,
        name: name(id)?,
        input_channels: channel_count(id, true)?,
        output_channels: channel_count(id, false)?,
        sample_rate: nominal_sample_rate(id)?,
        buffer_frames: buffer_frame_size(id)?,
    })
}

/// The system default input device, if any.
pub fn default_input() -> Option<DeviceId> {
    macos_helpers::get_default_device_id(true)
}

/// The system default output device, if any.
pub fn default_output() -> Option<DeviceId> {
    macos_helpers::get_default_device_id(false)
}

/// Finds a device by its display name, matching case-insensitively on a
/// prefix so "Scarlett" finds "Scarlett 2i2 USB".
pub fn find_by_name(query: &str) -> Result<Option<DeviceId>> {
    let query = query.to_lowercase();
    let mut matches = list()?
        .into_iter()
        .filter(|d| d.name.to_lowercase().starts_with(&query));
    let first = matches.next();
    if first.is_some() && matches.next().is_some() {
        bail!("more than one device matches {query:?}; use a longer name");
    }
    Ok(first.map(|d| d.id))
}

/// The device's display name.
pub fn name(id: DeviceId) -> Result<String> {
    macos_helpers::get_device_name(id).with_context(|| format!("reading name of device {id}"))
}

/// Number of input or output channels across all of the device's streams.
pub fn channel_count(id: DeviceId, input: bool) -> Result<u32> {
    let scope = scope_for(input);
    let address = address(kAudioDevicePropertyStreamConfiguration, scope);
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
        )
    };
    check(status, "reading stream configuration size")?;
    if size == 0 {
        return Ok(0);
    }
    // AudioBufferList is variable length and 8-byte aligned; a Vec<u64>
    // gives us correctly aligned storage of the right size.
    let words = size.div_ceil(size_of::<u64>() as u32) as usize;
    let mut storage = vec![0u64; words];
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::new(storage.as_mut_ptr()).unwrap().cast(),
        )
    };
    check(status, "reading stream configuration")?;
    let list = storage.as_ptr() as *const AudioBufferList;
    let channels = unsafe {
        let count = (*list).mNumberBuffers as usize;
        let buffers = std::slice::from_raw_parts((*list).mBuffers.as_ptr(), count);
        buffers.iter().map(|b| b.mNumberChannels).sum()
    };
    Ok(channels)
}

/// The device's current nominal sample rate.
pub fn nominal_sample_rate(id: DeviceId) -> Result<f64> {
    get(
        id,
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
    )
}

/// Sets the device's nominal sample rate if it differs, waiting for the
/// change to take effect. Fails if the device does not support the rate.
pub fn set_sample_rate(id: DeviceId, rate: f64) -> Result<()> {
    macos_helpers::set_device_sample_rate(id, rate)
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("setting device {id} to {rate} Hz"))
}

/// The device's current I/O buffer size in frames.
pub fn buffer_frame_size(id: DeviceId) -> Result<u32> {
    get(
        id,
        kAudioDevicePropertyBufferFrameSize,
        kAudioObjectPropertyScopeGlobal,
    )
}

/// The smallest and largest buffer size the device accepts.
pub fn buffer_frame_size_range(id: DeviceId) -> Result<(u32, u32)> {
    let range: AudioValueRange = get(
        id,
        kAudioDevicePropertyBufferFrameSizeRange,
        kAudioObjectPropertyScopeGlobal,
    )?;
    Ok((range.mMinimum as u32, range.mMaximum as u32))
}

/// Sets the device's I/O buffer size in frames and confirms what the device
/// actually chose, which may differ from the request.
pub fn set_buffer_frame_size(id: DeviceId, frames: u32) -> Result<u32> {
    let (min, max) = buffer_frame_size_range(id)?;
    if frames < min || frames > max {
        bail!("device {id} accepts buffers from {min} to {max} frames, not {frames}");
    }
    set(
        id,
        kAudioDevicePropertyBufferFrameSize,
        kAudioObjectPropertyScopeGlobal,
        &frames,
    )?;
    buffer_frame_size(id)
}

/// Latency the device reports for one direction, in frames, split into the
/// device's own latency and the HAL safety offset. Add the buffer size for
/// the total CoreAudio-reported latency in that direction.
pub fn reported_latency(id: DeviceId, input: bool) -> Result<(u32, u32)> {
    let scope = scope_for(input);
    let device: u32 = get(id, kAudioDevicePropertyLatency, scope)?;
    let safety: u32 = get(id, kAudioDevicePropertySafetyOffset, scope)?;
    Ok((device, safety))
}

fn scope_for(input: bool) -> AudioObjectPropertyScope {
    if input {
        kAudioObjectPropertyScopeInput
    } else {
        kAudioObjectPropertyScopeOutput
    }
}

fn address(selector: u32, scope: AudioObjectPropertyScope) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get<T: Copy>(id: DeviceId, selector: u32, scope: AudioObjectPropertyScope) -> Result<T> {
    let address = address(selector, scope);
    let mut value = MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr()).unwrap().cast(),
        )
    };
    check(status, "reading device property")?;
    if size as usize != size_of::<T>() {
        bail!(
            "device property {selector:#x} returned {size} bytes, expected {}",
            size_of::<T>()
        );
    }
    Ok(unsafe { value.assume_init() })
}

fn set<T>(id: DeviceId, selector: u32, scope: AudioObjectPropertyScope, value: &T) -> Result<()> {
    let address = address(selector, scope);
    let status = unsafe {
        AudioObjectSetPropertyData(
            id,
            NonNull::from(&address),
            0,
            null(),
            size_of::<T>() as u32,
            NonNull::from(value).cast(),
        )
    };
    check(status, "setting device property")
}

fn check(status: i32, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "{what}: CoreAudio status {status} ({})",
            fourcc(status)
        ))
    }
}

/// CoreAudio statuses are often four ASCII characters packed in an i32.
fn fourcc(status: i32) -> String {
    let bytes = status.to_be_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        "not a four-char code".to_string()
    }
}
