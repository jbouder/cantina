//! Audio Unit (v2 API) hosting.
//!
//! The wrapper owns an `AudioComponentInstance`, feeds it input through a
//! render callback that copies from a scratch buffer, and pulls output with
//! `AudioUnitRender`. Everything the audio thread touches is allocated in
//! `prepare`.
//!
//! The instance is shared with an [`AudioUnitHandle`], the UI-thread
//! side: state as the unit's `ClassInfo` property list, the live latency,
//! and parameter changes the unit made on its own. AUv2 property and
//! parameter calls are safe from another thread while the unit renders,
//! which is how every host saves and shows an editor during playback.
//! The instance is disposed when both sides are gone.

use std::any::Any;
use std::borrow::Cow;
use std::ffi::{c_void, CStr};
use std::mem::{size_of, MaybeUninit};
use std::ptr::{null, null_mut, NonNull};
use std::slice;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{Buffer, Handle, ParamId, ParamInfo, Processor, StateError, Unit};
use objc2_audio_toolbox::{
    kAUParameterListener_AnyParameter, kAudioUnitManufacturer_Apple, kAudioUnitProperty_ClassInfo,
    kAudioUnitProperty_CocoaUI, kAudioUnitProperty_ElementCount, kAudioUnitProperty_Latency,
    kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_ParameterInfo,
    kAudioUnitProperty_ParameterList, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
    kAudioUnitScope_Output, kAudioUnitType_Effect, kAudioUnitType_MusicEffect,
    AUParameterListenerNotify, AURenderCallbackStruct, AudioComponent, AudioComponentCopyName,
    AudioComponentDescription, AudioComponentFindNext, AudioComponentGetDescription,
    AudioComponentGetVersion, AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioUnit,
    AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo, AudioUnitInitialize,
    AudioUnitParameter, AudioUnitParameterID, AudioUnitParameterInfo, AudioUnitParameterOptions,
    AudioUnitParameterUnit, AudioUnitRender, AudioUnitRenderActionFlags, AudioUnitScope,
    AudioUnitSetParameter, AudioUnitSetProperty, AudioUnitUninitialize,
};
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM, AudioBuffer, AudioBufferList, AudioStreamBasicDescription,
    AudioTimeStamp, AudioTimeStampFlags,
};
use objc2_core_foundation::{
    CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListCreateWithData,
    CFPropertyListFormat, CFRetained, CFString,
};

/// Most channels the wrapper will route. Plugins with wider layouts are
/// configured narrower.
pub const MAX_HOST_CHANNELS: usize = 8;

/// The three four-character codes that identify an Audio Unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId {
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
}

impl ComponentId {
    /// Parses `type:subtype:manufacturer`, for example `aufx:dely:appl`.
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<&str> = text.split(':').collect();
        if parts.len() != 3 {
            bail!("an Audio Unit identifier looks like aufx:dely:appl, not {text:?}");
        }
        Ok(Self {
            kind: fourcc(parts[0])?,
            subtype: fourcc(parts[1])?,
            manufacturer: fourcc(parts[2])?,
        })
    }

    fn description(self) -> AudioComponentDescription {
        AudioComponentDescription {
            componentType: self.kind,
            componentSubType: self.subtype,
            componentManufacturer: self.manufacturer,
            componentFlags: 0,
            componentFlagsMask: 0,
        }
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            fourcc_string(self.kind),
            fourcc_string(self.subtype),
            fourcc_string(self.manufacturer)
        )
    }
}

/// What the scanner knows about an installed Audio Unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInfo {
    pub id: ComponentId,
    /// As the component reports it, usually `Manufacturer: Name`.
    pub name: String,
    pub version: u32,
}

impl ComponentInfo {
    pub fn is_apple(&self) -> bool {
        self.id.manufacturer == kAudioUnitManufacturer_Apple
    }

    /// The manufacturer and the unit's own name, split out of the
    /// `Manufacturer: Name` form the component reports.
    pub fn vendor_and_name(&self) -> (String, String) {
        match self.name.split_once(": ") {
            Some((vendor, name)) => (vendor.to_string(), name.to_string()),
            None => (String::new(), self.name.clone()),
        }
    }

    /// The version as `major.minor.patch`, decoded from the packed
    /// number the component reports.
    pub fn version_string(&self) -> String {
        format!(
            "{}.{}.{}",
            self.version >> 16,
            (self.version >> 8) & 0xff,
            self.version & 0xff
        )
    }
}

/// What instantiating a unit reveals that the component list does not:
/// its default channel layout and whether it ships an editor view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceInfo {
    pub inputs: u32,
    pub outputs: u32,
    pub has_editor: bool,
}

/// Instantiates a unit, reads its [`InstanceInfo`] and disposes of it
/// again. This runs the plugin's code, which is why the scanner does it
/// in a child process.
pub fn describe_instance(id: ComponentId) -> Result<InstanceInfo> {
    let mut wanted = id.description();
    let component = unsafe { AudioComponentFindNext(null_mut(), NonNull::from(&mut wanted)) };
    if component.is_null() {
        bail!("Audio Unit {id} is not installed");
    }
    let mut unit: AudioUnit = null_mut();
    check(
        unsafe { AudioComponentInstanceNew(component, NonNull::from(&mut unit)) },
        "instantiating the Audio Unit",
    )?;
    let info = read_instance_info(unit);
    unsafe { AudioComponentInstanceDispose(unit) };
    info
}

fn read_instance_info(unit: AudioUnit) -> Result<InstanceInfo> {
    let channels = |scope: AudioUnitScope| -> u32 {
        let mut count = 0u32;
        let mut size = size_of::<u32>() as u32;
        let status = unsafe {
            AudioUnitGetProperty(
                unit,
                kAudioUnitProperty_ElementCount,
                scope,
                0,
                NonNull::from(&mut count).cast(),
                NonNull::from(&mut size),
            )
        };
        if status != 0 || count == 0 {
            return 0;
        }
        let mut format = MaybeUninit::<AudioStreamBasicDescription>::uninit();
        let mut size = size_of::<AudioStreamBasicDescription>() as u32;
        let status = unsafe {
            AudioUnitGetProperty(
                unit,
                kAudioUnitProperty_StreamFormat,
                scope,
                0,
                NonNull::new(format.as_mut_ptr()).unwrap().cast(),
                NonNull::from(&mut size),
            )
        };
        if status != 0 {
            return 0;
        }
        unsafe { format.assume_init() }.mChannelsPerFrame
    };
    let inputs = channels(kAudioUnitScope_Input);
    let outputs = channels(kAudioUnitScope_Output);
    let mut size = 0u32;
    let status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &mut size,
            null_mut(),
        )
    };
    Ok(InstanceInfo {
        inputs,
        outputs,
        has_editor: status == 0 && size > 0,
    })
}

/// Every installed effect Audio Unit, Apple's and third parties', in the
/// order the system lists them.
pub fn list_effects() -> Vec<ComponentInfo> {
    let mut found = Vec::new();
    for kind in [kAudioUnitType_Effect, kAudioUnitType_MusicEffect] {
        let mut wanted = AudioComponentDescription {
            componentType: kind,
            componentSubType: 0,
            componentManufacturer: 0,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let mut component: AudioComponent =
            unsafe { AudioComponentFindNext(null_mut(), NonNull::from(&mut wanted)) };
        while !component.is_null() {
            if let Some(info) = describe(component) {
                found.push(info);
            }
            component = unsafe { AudioComponentFindNext(component, NonNull::from(&mut wanted)) };
        }
    }
    found
}

/// Finds one effect by identifier (`aufx:dely:appl`) or by a
/// case-insensitive substring of its name that matches exactly one unit.
pub fn find(query: &str) -> Result<ComponentInfo> {
    if query.matches(':').count() == 2 {
        let id = ComponentId::parse(query)?;
        return list_effects()
            .into_iter()
            .find(|c| c.id == id)
            .with_context(|| format!("no Audio Unit {id} is installed"));
    }
    let needle = query.to_lowercase();
    let matches: Vec<ComponentInfo> = list_effects()
        .into_iter()
        .filter(|c| c.name.to_lowercase().contains(&needle))
        .collect();
    match matches.len() {
        0 => bail!("no Audio Unit effect has {query:?} in its name"),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => bail!(
            "{n} Audio Units match {query:?}: {}",
            matches
                .iter()
                .map(|c| format!("{} ({})", c.name, c.id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn describe(component: AudioComponent) -> Option<ComponentInfo> {
    let mut description = MaybeUninit::<AudioComponentDescription>::uninit();
    let status = unsafe {
        AudioComponentGetDescription(component, NonNull::new(description.as_mut_ptr()).unwrap())
    };
    if status != 0 {
        return None;
    }
    let description = unsafe { description.assume_init() };
    let mut name_ptr: *const CFString = std::ptr::null();
    let status = unsafe { AudioComponentCopyName(component, NonNull::from(&mut name_ptr)) };
    let name = if status == 0 && !name_ptr.is_null() {
        let name =
            unsafe { CFRetained::from_raw(NonNull::new(name_ptr as *mut CFString).unwrap()) };
        name.to_string()
    } else {
        String::from("(unnamed)")
    };
    let mut version = 0u32;
    unsafe { AudioComponentGetVersion(component, NonNull::from(&mut version)) };
    Some(ComponentInfo {
        id: ComponentId {
            kind: description.componentType,
            subtype: description.componentSubType,
            manufacturer: description.componentManufacturer,
        },
        name,
        version,
    })
}

/// Owns the instance; disposed when the processor and its handle are
/// both gone.
pub struct UnitOwner(AudioUnit);

// Property and parameter calls on an AUv2 instance are safe from any
// thread, and `AudioUnitRender` is only ever called by the thread that
// owns the processor.
unsafe impl Send for UnitOwner {}
unsafe impl Sync for UnitOwner {}

impl UnitOwner {
    pub fn raw(&self) -> AudioUnit {
        self.0
    }
}

impl Drop for UnitOwner {
    fn drop(&mut self) {
        unsafe { AudioComponentInstanceDispose(self.0) };
    }
}

/// The unit's state as its `ClassInfo` property list, serialized as a
/// binary plist. This is what every host saves: it holds the parameters
/// and whatever else the unit keeps.
fn class_info(unit: AudioUnit) -> Result<Vec<u8>> {
    let mut plist: *const CFPropertyList = null();
    let mut size = size_of::<*const CFPropertyList>() as u32;
    check(
        unsafe {
            AudioUnitGetProperty(
                unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut plist).cast(),
                NonNull::from(&mut size),
            )
        },
        "reading the state",
    )?;
    let Some(plist) = NonNull::new(plist as *mut CFPropertyList) else {
        bail!("the unit returned no state");
    };
    // The property hands out a reference we own.
    let plist = unsafe { CFRetained::from_raw(plist) };
    let data = unsafe {
        CFPropertyListCreateData(
            None,
            Some(&plist),
            CFPropertyListFormat::BinaryFormat_v1_0,
            0,
            null_mut(),
        )
    }
    .context("encoding the state as a property list")?;
    Ok(data.to_vec())
}

/// Restores state saved by [`class_info`] and tells any listener, such
/// as an editor view, that every parameter may have changed.
fn set_class_info(unit: AudioUnit, bytes: &[u8]) -> Result<()> {
    let data = CFData::from_bytes(bytes);
    let plist =
        unsafe { CFPropertyListCreateWithData(None, Some(&data), 0, null_mut(), null_mut()) }
            .context("the state is not a property list")?;
    let pointer: *const CFPropertyList = CFRetained::as_ptr(&plist).as_ptr();
    check(
        unsafe {
            AudioUnitSetProperty(
                unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                &pointer as *const *const CFPropertyList as *const c_void,
                size_of::<*const CFPropertyList>() as u32,
            )
        },
        "restoring the state",
    )?;
    let mut all = AudioUnitParameter {
        mAudioUnit: unit,
        mParameterID: kAUParameterListener_AnyParameter,
        mScope: kAudioUnitScope_Global,
        mElement: 0,
    };
    unsafe { AUParameterListenerNotify(null_mut(), null_mut(), NonNull::from(&mut all)) };
    Ok(())
}

fn param_value(unit: AudioUnit, id: AudioUnitParameterID) -> f32 {
    let mut value = 0.0f32;
    let status = unsafe {
        AudioUnitGetParameter(
            unit,
            id,
            kAudioUnitScope_Global,
            0,
            NonNull::from(&mut value),
        )
    };
    if status == 0 {
        value
    } else {
        0.0
    }
}

/// The unit's latency in frames at its current output rate, or zero
/// when it does not say.
fn latency_frames(unit: AudioUnit) -> usize {
    let mut seconds = 0.0f64;
    let mut size = size_of::<f64>() as u32;
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            NonNull::from(&mut seconds).cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return 0;
    }
    let mut format = MaybeUninit::<AudioStreamBasicDescription>::uninit();
    let mut size = size_of::<AudioStreamBasicDescription>() as u32;
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Output,
            0,
            NonNull::new(format.as_mut_ptr()).unwrap().cast(),
            NonNull::from(&mut size),
        )
    };
    let sample_rate = if status == 0 {
        unsafe { format.assume_init() }.mSampleRate
    } else {
        0.0
    };
    (seconds * sample_rate).round().max(0.0) as usize
}

/// The UI-thread side of an [`AudioUnitProcessor`]. See the module docs.
pub struct AudioUnitHandle {
    unit: Arc<UnitOwner>,
    info: ComponentInfo,
    display_name: String,
    params: Vec<ParamInfo>,
    param_ids: Vec<AudioUnitParameterID>,
    /// Values as of the last `take_changes`, so changes made by an
    /// editor or a preset show up as a difference.
    last: Vec<f32>,
}

impl AudioUnitHandle {
    pub fn info(&self) -> &ComponentInfo {
        &self.info
    }

    pub fn name(&self) -> &str {
        &self.display_name
    }

    /// The raw instance, for an editor view.
    pub fn unit(&self) -> AudioUnit {
        self.unit.raw()
    }

    /// A share in the instance's ownership, so an editor window can
    /// outlive the processor and the handle without the unit going away
    /// under its view.
    pub fn owner(&self) -> Arc<UnitOwner> {
        Arc::clone(&self.unit)
    }

    /// Current value of a parameter, as the unit reports it.
    pub fn param(&self, id: ParamId) -> f32 {
        self.param_ids
            .get(id)
            .map_or(0.0, |&au_id| param_value(self.unit.raw(), au_id))
    }

    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }
}

impl Handle for AudioUnitHandle {
    fn state(&mut self) -> Vec<u8> {
        class_info(self.unit.raw()).unwrap_or_default()
    }

    fn set_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        set_class_info(self.unit.raw(), state).map_err(|e| StateError::Invalid(format!("{e:#}")))
    }

    fn latency(&self) -> usize {
        latency_frames(self.unit.raw())
    }

    /// Every Audio Unit has an editor: its own view, or Apple's generic
    /// one.
    fn has_editor(&self) -> bool {
        true
    }

    fn take_changes(&mut self) -> Vec<(ParamId, f32)> {
        let mut changes = Vec::new();
        for (id, &au_id) in self.param_ids.iter().enumerate() {
            let value = param_value(self.unit.raw(), au_id);
            if value != self.last[id] {
                self.last[id] = value;
                changes.push((id, value));
            }
        }
        changes
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Where the render callback reads input from during one `process` call.
/// Lives in a `Box` so its address is stable for the callback's ref-con.
struct InputSource {
    channels: [*const f32; MAX_HOST_CHANNELS],
    channel_count: usize,
    frames: usize,
}

/// An Audio Unit effect as a [`Processor`].
pub struct AudioUnitProcessor {
    owner: Arc<UnitOwner>,
    unit: AudioUnit,
    info: ComponentInfo,
    display_name: String,
    channels: usize,
    sample_rate: f64,
    max_frames: usize,
    initialized: bool,
    params: Vec<ParamInfo>,
    param_ids: Vec<AudioUnitParameterID>,
    input: Box<InputSource>,
    scratch_in: Vec<Vec<f32>>,
    scratch_out: Vec<Vec<f32>>,
    /// Storage for an `AudioBufferList` with `channels` buffers, 8-byte
    /// aligned.
    buffer_list: Vec<u64>,
    sample_time: f64,
    latency_frames: usize,
    render_errors: u64,
    handle_taken: bool,
}

// The instance is only ever rendered by the thread that currently owns
// the processor; see `UnitOwner` for the rest.
unsafe impl Send for AudioUnitProcessor {}

impl AudioUnitProcessor {
    /// Instantiates the unit for a fixed channel count, unprepared. Reads the
    /// parameter list so the UI can show it before audio starts.
    pub fn new(info: ComponentInfo, channels: usize) -> Result<Self> {
        if channels == 0 || channels > MAX_HOST_CHANNELS {
            bail!("channel count must be 1 to {MAX_HOST_CHANNELS}, not {channels}");
        }
        let mut wanted = info.id.description();
        let component = unsafe { AudioComponentFindNext(null_mut(), NonNull::from(&mut wanted)) };
        if component.is_null() {
            bail!("Audio Unit {} is not installed", info.id);
        }
        let mut unit: AudioUnit = null_mut();
        check(
            unsafe { AudioComponentInstanceNew(component, NonNull::from(&mut unit)) },
            "instantiating the Audio Unit",
        )?;
        let display_name = info
            .name
            .split_once(": ")
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| info.name.clone());

        let mut processor = Self {
            owner: Arc::new(UnitOwner(unit)),
            unit,
            info,
            display_name,
            channels,
            sample_rate: 48_000.0,
            max_frames: 0,
            initialized: false,
            params: Vec::new(),
            param_ids: Vec::new(),
            input: Box::new(InputSource {
                channels: [std::ptr::null(); MAX_HOST_CHANNELS],
                channel_count: 0,
                frames: 0,
            }),
            scratch_in: Vec::new(),
            scratch_out: Vec::new(),
            buffer_list: Vec::new(),
            sample_time: 0.0,
            latency_frames: 0,
            render_errors: 0,
            handle_taken: false,
        };
        processor.read_params()?;
        Ok(processor)
    }

    pub fn info(&self) -> &ComponentInfo {
        &self.info
    }

    /// Channels the unit was configured for.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Render calls the unit has failed. Non-zero means audio is bypassing.
    pub fn render_errors(&self) -> u64 {
        self.render_errors
    }

    fn read_params(&mut self) -> Result<()> {
        let size =
            self.property_size(kAudioUnitProperty_ParameterList, kAudioUnitScope_Global, 0)?;
        let count = size as usize / size_of::<AudioUnitParameterID>();
        let mut ids = vec![0 as AudioUnitParameterID; count];
        if count > 0 {
            let mut size = size;
            check(
                unsafe {
                    AudioUnitGetProperty(
                        self.unit,
                        kAudioUnitProperty_ParameterList,
                        kAudioUnitScope_Global,
                        0,
                        NonNull::new(ids.as_mut_ptr()).unwrap().cast(),
                        NonNull::from(&mut size),
                    )
                },
                "reading the parameter list",
            )?;
        }
        let mut params = Vec::with_capacity(count);
        for &id in &ids {
            let info: AudioUnitParameterInfo =
                self.get_property(kAudioUnitProperty_ParameterInfo, kAudioUnitScope_Global, id)?;
            params.push(param_info(&info));
        }
        self.params = params;
        self.param_ids = ids;
        Ok(())
    }

    fn property_size(&self, id: u32, scope: AudioUnitScope, element: u32) -> Result<u32> {
        let mut size = 0u32;
        check(
            unsafe {
                AudioUnitGetPropertyInfo(self.unit, id, scope, element, &mut size, null_mut())
            },
            "reading a property size",
        )?;
        Ok(size)
    }

    fn get_property<T>(&self, id: u32, scope: AudioUnitScope, element: u32) -> Result<T> {
        let mut value = MaybeUninit::<T>::uninit();
        let mut size = size_of::<T>() as u32;
        check(
            unsafe {
                AudioUnitGetProperty(
                    self.unit,
                    id,
                    scope,
                    element,
                    NonNull::new(value.as_mut_ptr()).unwrap().cast(),
                    NonNull::from(&mut size),
                )
            },
            "reading a property",
        )?;
        if size as usize != size_of::<T>() {
            bail!(
                "property {id} returned {size} bytes, expected {}",
                size_of::<T>()
            );
        }
        Ok(unsafe { value.assume_init() })
    }

    fn set_property<T>(
        &self,
        id: u32,
        scope: AudioUnitScope,
        element: u32,
        value: &T,
    ) -> Result<()> {
        check(
            unsafe {
                AudioUnitSetProperty(
                    self.unit,
                    id,
                    scope,
                    element,
                    value as *const T as *const c_void,
                    size_of::<T>() as u32,
                )
            },
            "setting a property",
        )
    }

    fn configure(&mut self, sample_rate: f64, max_frames: usize) -> Result<()> {
        if self.initialized {
            check(
                unsafe { AudioUnitUninitialize(self.unit) },
                "uninitialising",
            )?;
            self.initialized = false;
        }
        let format = AudioStreamBasicDescription {
            mSampleRate: sample_rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat
                | kAudioFormatFlagIsPacked
                | kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: self.channels as u32,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        self.set_property(
            kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Input,
            0,
            &format,
        )
        .with_context(|| {
            format!(
                "{} rejected {} input channels",
                self.display_name, self.channels
            )
        })?;
        self.set_property(
            kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Output,
            0,
            &format,
        )
        .with_context(|| {
            format!(
                "{} rejected {} output channels",
                self.display_name, self.channels
            )
        })?;
        self.set_property(
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            &(max_frames as u32),
        )?;
        let callback = AURenderCallbackStruct {
            inputProc: Some(input_proc),
            inputProcRefCon: &*self.input as *const InputSource as *mut c_void,
        };
        self.set_property(
            kAudioUnitProperty_SetRenderCallback,
            kAudioUnitScope_Input,
            0,
            &callback,
        )?;
        check(unsafe { AudioUnitInitialize(self.unit) }, "initialising")?;
        self.initialized = true;

        self.latency_frames = latency_frames(self.unit);
        self.sample_rate = sample_rate;
        self.max_frames = max_frames;
        self.scratch_in = vec![vec![0.0; max_frames]; self.channels];
        self.scratch_out = vec![vec![0.0; max_frames]; self.channels];
        let bytes = size_of::<AudioBufferList>() + (self.channels - 1) * size_of::<AudioBuffer>();
        self.buffer_list = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        self.sample_time = 0.0;
        Ok(())
    }
}

impl Drop for AudioUnitProcessor {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { AudioUnitUninitialize(self.unit) };
        }
        // The owner disposes of the instance once the handle is gone too.
    }
}

impl Processor for AudioUnitProcessor {
    fn name(&self) -> &str {
        &self.display_name
    }

    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn param(&self, id: ParamId) -> f32 {
        self.param_ids
            .get(id)
            .map_or(0.0, |&au_id| param_value(self.unit, au_id))
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        let (Some(&au_id), Some(info)) = (self.param_ids.get(id), self.params.get(id)) else {
            return;
        };
        unsafe {
            AudioUnitSetParameter(
                self.unit,
                au_id,
                kAudioUnitScope_Global,
                0,
                info.clamp(value),
                0,
            );
        }
    }

    fn latency(&self) -> usize {
        self.latency_frames
    }

    fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
        if let Err(e) = self.configure(sample_rate, max_block_frames) {
            // A processor that failed to prepare bypasses audio rather than
            // bringing the stream down; the error is visible as a render
            // error count and in the log once the engine has one.
            self.initialized = false;
            self.render_errors += 1;
            eprintln!("{}: {e:#}", self.display_name);
        }
    }

    fn reset(&mut self) {
        if self.initialized {
            unsafe {
                // Uninitialise and initialise again: the documented way to
                // clear an Audio Unit's internal state.
                AudioUnitUninitialize(self.unit);
                if AudioUnitInitialize(self.unit) != 0 {
                    self.initialized = false;
                }
            }
        }
        self.sample_time = 0.0;
    }

    fn process(&mut self, buffer: &mut Buffer) {
        let frames = buffer.frames();
        if !self.initialized || frames == 0 || frames > self.max_frames {
            return;
        }
        let present = buffer.channels().min(self.channels);
        // Copy the input aside so the unit can render over the buffer. Unit
        // channels the buffer lacks hear channel 0.
        for ch in 0..self.channels {
            let source = buffer.channel(ch.min(present - 1));
            self.scratch_in[ch][..frames].copy_from_slice(&source[..frames]);
            self.input.channels[ch] = self.scratch_in[ch].as_ptr();
        }
        self.input.channel_count = self.channels;
        self.input.frames = frames;

        let list = self.buffer_list.as_mut_ptr() as *mut AudioBufferList;
        unsafe {
            (*list).mNumberBuffers = self.channels as u32;
            let buffers = slice::from_raw_parts_mut((*list).mBuffers.as_mut_ptr(), self.channels);
            for (ch, slot) in buffers.iter_mut().enumerate() {
                *slot = AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: (frames * size_of::<f32>()) as u32,
                    mData: self.scratch_out[ch].as_mut_ptr() as *mut c_void,
                };
            }
        }
        let mut timestamp: AudioTimeStamp = unsafe { std::mem::zeroed() };
        timestamp.mSampleTime = self.sample_time;
        timestamp.mFlags = AudioTimeStampFlags::SampleTimeValid;
        let mut flags = AudioUnitRenderActionFlags(0);
        let status = unsafe {
            AudioUnitRender(
                self.unit,
                &mut flags,
                NonNull::from(&timestamp),
                0,
                frames as u32,
                NonNull::new(list).unwrap(),
            )
        };
        self.sample_time += frames as f64;
        if status != 0 {
            self.render_errors += 1;
            return;
        }
        for ch in 0..present {
            buffer.channel_mut(ch)[..frames].copy_from_slice(&self.scratch_out[ch][..frames]);
        }
    }

    fn state(&mut self) -> Vec<u8> {
        class_info(self.unit).unwrap_or_default()
    }

    fn set_state(&mut self, state: &[u8]) -> Result<(), StateError> {
        set_class_info(self.unit, state).map_err(|e| StateError::Invalid(format!("{e:#}")))
    }

    fn handle(&mut self) -> Option<Box<dyn Handle>> {
        if self.handle_taken {
            return None;
        }
        self.handle_taken = true;
        let last = (0..self.params.len()).map(|id| self.param(id)).collect();
        Some(Box::new(AudioUnitHandle {
            unit: Arc::clone(&self.owner),
            info: self.info.clone(),
            display_name: self.display_name.clone(),
            params: self.params.clone(),
            param_ids: self.param_ids.clone(),
            last,
        }))
    }
}

/// The unit pulls its input through this. Runs on the audio thread: no
/// allocation, no locks.
unsafe extern "C-unwind" fn input_proc(
    ref_con: NonNull<c_void>,
    _flags: NonNull<AudioUnitRenderActionFlags>,
    _timestamp: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    if io_data.is_null() {
        return -50; // paramErr
    }
    let source = &*(ref_con.as_ptr() as *const InputSource);
    let list = &mut *io_data;
    let buffers =
        slice::from_raw_parts_mut(list.mBuffers.as_mut_ptr(), list.mNumberBuffers as usize);
    let frames = frames as usize;
    for (ch, buffer) in buffers.iter_mut().enumerate() {
        let available = source.frames.min(frames);
        if ch >= source.channel_count {
            if !buffer.mData.is_null() {
                slice::from_raw_parts_mut(buffer.mData as *mut f32, frames).fill(0.0);
            }
            continue;
        }
        if buffer.mData.is_null() {
            // The unit asked us to supply the buffer; point it at ours.
            buffer.mData = source.channels[ch] as *mut c_void;
            buffer.mDataByteSize = (available * size_of::<f32>()) as u32;
            continue;
        }
        let destination = slice::from_raw_parts_mut(buffer.mData as *mut f32, frames);
        destination[..available]
            .copy_from_slice(slice::from_raw_parts(source.channels[ch], available));
        destination[available..].fill(0.0);
    }
    0
}

fn param_info(info: &AudioUnitParameterInfo) -> ParamInfo {
    let name = if info
        .flags
        .contains(AudioUnitParameterOptions::Flag_HasCFNameString)
        && !info.cfNameString.is_null()
    {
        let cf = unsafe { &*info.cfNameString };
        let name = cf.to_string();
        if info
            .flags
            .contains(AudioUnitParameterOptions::Flag_CFNameRelease)
        {
            unsafe {
                drop(CFRetained::from_raw(
                    NonNull::new(info.cfNameString as *mut CFString).unwrap(),
                ));
            }
        }
        name
    } else {
        let bytes: Vec<u8> = info
            .name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let unit = match info.unit {
        AudioUnitParameterUnit::Decibels => Unit::Decibels,
        AudioUnitParameterUnit::Hertz => Unit::Hertz,
        AudioUnitParameterUnit::Seconds => Unit::Seconds,
        AudioUnitParameterUnit::Percent => Unit::Percent,
        _ => Unit::Generic,
    };
    ParamInfo {
        name: Cow::Owned(name),
        unit,
        min: info.minValue,
        max: info.maxValue,
        default: info.defaultValue,
    }
}

fn fourcc(text: &str) -> Result<u32> {
    let bytes = text.as_bytes();
    if bytes.len() != 4 {
        bail!("{text:?} is not a four-character code");
    }
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn fourcc_string(code: u32) -> String {
    let bytes = code.to_be_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("{code:#010x}")
    }
}

fn check(status: i32, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "{what}: status {status} ({})",
            fourcc_string(status as u32)
        ))
    }
}

/// Keeps `CStr` in scope for callers that want to read the fixed name field.
#[allow(dead_code)]
fn c_name(name: &[i8; 52]) -> String {
    unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_id_round_trips() {
        let id = ComponentId::parse("aufx:dely:appl").unwrap();
        assert_eq!(id.kind, kAudioUnitType_Effect);
        assert_eq!(id.manufacturer, kAudioUnitManufacturer_Apple);
        assert_eq!(id.to_string(), "aufx:dely:appl");
        assert!(ComponentId::parse("aufx:dely").is_err());
        assert!(ComponentId::parse("aufx:delay:appl").is_err());
    }
}
