//! Third-party plugin hosting for Cantina.
//!
//! Every plugin format ends up as a [`cantina_dsp::Processor`], so the
//! chain, the mixer and the project file never know they are holding a
//! plugin. Audio Units come first because they cover nearly every
//! commercial Mac plugin; CLAP follows.
//!
//! Plugins run in-process on the audio thread, like every mainstream DAW
//! except Bitwig. Scanning runs in a child process ([`scan`]), so a plugin
//! that crashes when it loads is recorded as failed instead of taking the
//! app down.

pub mod au;
pub mod clap;
pub mod scan;

pub use au::{AudioUnitHandle, AudioUnitProcessor, ComponentId, ComponentInfo};
pub use clap::ClapProcessor;
pub use scan::{Format, PluginEntry, PluginList, Scanner, Source};
