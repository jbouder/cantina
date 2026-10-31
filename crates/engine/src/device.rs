//! CoreAudio device enumeration and configuration.
//!
//! Nothing here runs on the audio thread. These calls may block briefly
//! while the HAL talks to a driver.

use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};
use std::ptr::{null, NonNull};
use std::sync::mpsc::Sender;

use anyhow::{anyhow, bail, Context, Result};
use coreaudio::audio_unit::macos_helpers;
use objc2_core_audio::{
    kAudioDevicePropertyAvailableNominalSampleRates, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyBufferFrameSizeRange, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyLatency,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertySafetyOffset,
    kAudioDevicePropertyStreamConfiguration, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyScope,
    AudioObjectRemovePropertyListener, AudioObjectSetPropertyData,
};
use objc2_core_audio_types::{AudioBufferList, AudioValueRange};
use objc2_core_foundation::{CFRetained, CFString};

/// A CoreAudio device identifier. Stable for the life of the device
/// connection, not across unplug and replug.
pub type DeviceId = AudioObjectID;

/// What the UI needs to know about a device to offer it in a picker.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    /// Stable across launches and replugs, unlike the id. What to persist.
    pub uid: String,
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
        uid: uid(id)?,
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

/// The device's unique identifier, stable across launches and replugs.
pub fn uid(id: DeviceId) -> Result<String> {
    let ptr: *const CFString = get(
        id,
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
    )?;
    let ptr = NonNull::new(ptr as *mut CFString).context("device UID was null")?;
    // The property hands back a +1 retained string that is ours to release.
    let uid = unsafe { CFRetained::from_raw(ptr) };
    Ok(uid.to_string())
}

/// Finds a device by the UID a previous launch saved. `None` when it is
/// not connected now.
pub fn find_by_uid(wanted: &str) -> Result<Option<DeviceId>> {
    for id in macos_helpers::get_audio_device_ids().context("listing audio devices")? {
        if uid(id).is_ok_and(|u| u == wanted) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// True while the device is connected and usable.
pub fn is_alive(id: DeviceId) -> bool {
    get::<u32>(
        id,
        kAudioDevicePropertyDeviceIsAlive,
        kAudioObjectPropertyScopeGlobal,
    )
    .is_ok_and(|alive| alive != 0)
}

/// Sample rates the user can pick for this device, ascending. Devices
/// report ranges; the common studio rates inside them are returned, plus
/// any exact rate the device names that is not in the common list.
pub fn available_sample_rates(id: DeviceId) -> Result<Vec<f64>> {
    const COMMON: [f64; 8] = [
        44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0, 352_800.0, 384_000.0,
    ];
    let address = address(
        kAudioDevicePropertyAvailableNominalSampleRates,
        kAudioObjectPropertyScopeGlobal,
    );
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
    check(status, "reading sample rate list size")?;
    let count = size as usize / size_of::<AudioValueRange>();
    let mut ranges = vec![
        AudioValueRange {
            mMinimum: 0.0,
            mMaximum: 0.0
        };
        count
    ];
    if count > 0 {
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                NonNull::from(&address),
                0,
                null(),
                NonNull::from(&mut size),
                NonNull::new(ranges.as_mut_ptr()).unwrap().cast(),
            )
        };
        check(status, "reading sample rate list")?;
    }
    let mut rates: Vec<f64> = Vec::new();
    for range in &ranges {
        for rate in COMMON {
            if rate >= range.mMinimum && rate <= range.mMaximum && !rates.contains(&rate) {
                rates.push(rate);
            }
        }
        if range.mMinimum == range.mMaximum && !rates.contains(&range.mMinimum) {
            rates.push(range.mMinimum);
        }
    }
    rates.sort_by(|a, b| a.total_cmp(b));
    Ok(rates)
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

/// Something changed in the audio hardware while a stream was running.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEvent {
    /// A device was plugged in or removed.
    DevicesChanged,
    DefaultInputChanged,
    DefaultOutputChanged,
    /// The device's nominal sample rate is now this.
    SampleRateChanged(DeviceId, f64),
    /// The device is no longer alive.
    DeviceGone(DeviceId),
}

/// Listens for hardware changes and sends them to a channel.
///
/// The HAL calls back on its own notification thread; the callback only
/// reads a property and pushes to the channel, so it is safe anywhere.
/// Dropping the watcher removes every listener.
pub struct Watcher {
    /// Boxed so the pointer handed to CoreAudio stays put.
    inner: Box<Inner>,
    registered: Vec<(AudioObjectID, AudioObjectPropertyAddress)>,
}

struct Inner {
    sender: Sender<DeviceEvent>,
}

impl Watcher {
    /// Watches the device list and the system defaults, plus the sample
    /// rate and liveness of each device in `devices`.
    pub fn new(sender: Sender<DeviceEvent>, devices: &[DeviceId]) -> Result<Watcher> {
        let mut watcher = Watcher {
            inner: Box::new(Inner { sender }),
            registered: Vec::new(),
        };
        let system = kAudioObjectSystemObject as AudioObjectID;
        for selector in [
            kAudioHardwarePropertyDevices,
            kAudioHardwarePropertyDefaultInputDevice,
            kAudioHardwarePropertyDefaultOutputDevice,
        ] {
            watcher.listen(system, selector)?;
        }
        let mut seen = Vec::new();
        for &device in devices {
            if seen.contains(&device) {
                continue;
            }
            seen.push(device);
            watcher.listen(device, kAudioDevicePropertyNominalSampleRate)?;
            watcher.listen(device, kAudioDevicePropertyDeviceIsAlive)?;
        }
        Ok(watcher)
    }

    fn listen(&mut self, object: AudioObjectID, selector: u32) -> Result<()> {
        let address = address(selector, kAudioObjectPropertyScopeGlobal);
        let client = &*self.inner as *const Inner as *mut c_void;
        let status = unsafe {
            AudioObjectAddPropertyListener(object, NonNull::from(&address), Some(on_change), client)
        };
        check(status, "adding a property listener")?;
        self.registered.push((object, address));
        Ok(())
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let client = &*self.inner as *const Inner as *mut c_void;
        for (object, address) in self.registered.drain(..) {
            unsafe {
                AudioObjectRemovePropertyListener(
                    object,
                    NonNull::from(&address),
                    Some(on_change),
                    client,
                );
            }
        }
    }
}

unsafe extern "C-unwind" fn on_change(
    object: AudioObjectID,
    count: u32,
    addresses: NonNull<AudioObjectPropertyAddress>,
    client: *mut c_void,
) -> i32 {
    let inner = &*(client as *const Inner);
    let addresses = std::slice::from_raw_parts(addresses.as_ptr(), count as usize);
    for address in addresses {
        let event = match address.mSelector {
            s if s == kAudioHardwarePropertyDevices => DeviceEvent::DevicesChanged,
            s if s == kAudioHardwarePropertyDefaultInputDevice => DeviceEvent::DefaultInputChanged,
            s if s == kAudioHardwarePropertyDefaultOutputDevice => {
                DeviceEvent::DefaultOutputChanged
            }
            s if s == kAudioDevicePropertyNominalSampleRate => match nominal_sample_rate(object) {
                Ok(rate) => DeviceEvent::SampleRateChanged(object, rate),
                Err(_) => continue,
            },
            s if s == kAudioDevicePropertyDeviceIsAlive => {
                if is_alive(object) {
                    continue;
                }
                DeviceEvent::DeviceGone(object)
            }
            _ => continue,
        };
        // A closed receiver means nobody cares any more.
        let _ = inner.sender.send(event);
    }
    0
}
