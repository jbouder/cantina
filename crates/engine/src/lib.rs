//! The Cantina audio engine: CoreAudio streams, and later the real-time
//! graph runtime, transport and lock-free queues.
//!
//! Only this crate touches the audio device. Everything the audio callback
//! does lives behind the real-time rules in [`stream`]; everything else,
//! device enumeration included, runs on ordinary threads.
//!
//! macOS only, by design. See `PLAN.md` for why.

pub mod device;
pub mod latency;
pub mod stream;

pub use device::{DeviceId, DeviceInfo};
pub use stream::{BlockProcessor, Stats, Stream, StreamConfig, StreamInfo, MAX_BLOCK_FRAMES};
