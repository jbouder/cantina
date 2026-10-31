//! Third-party plugin hosting for Cantina.
//!
//! Every plugin format ends up as a [`cantina_dsp::Processor`], so the
//! chain, the mixer and the project file never know they are holding a
//! plugin. Audio Units come first because they cover nearly every
//! commercial Mac plugin; CLAP follows.
//!
//! Plugins run in-process on the audio thread, like every mainstream DAW
//! except Bitwig. Scanning in a child process, so a crashing plugin cannot
//! take the app down, is M4.

pub mod au;

pub use au::{AudioUnitProcessor, ComponentId, ComponentInfo};
