//! The Cantina audio engine: CoreAudio streams, the real-time graph
//! runtime, the transport and tempo map, and WAV streaming to and from
//! disk.
//!
//! Only this crate touches the audio device. Everything the audio callback
//! does lives behind the real-time rules in [`stream`] and [`runtime`];
//! everything else, device enumeration included, runs on ordinary threads.
//!
//! macOS only, by design. See `PLAN.md` for why.

pub mod click;
pub mod device;
pub mod graph;
pub mod latency;
pub mod meter;
pub mod mute;
pub mod player;
pub mod processors;
pub mod recorder;
pub mod runtime;
pub mod stream;
pub mod strip;
pub mod tempo;
pub mod transport;
pub mod wav;

pub use click::Click;
pub use device::{DeviceEvent, DeviceId, DeviceInfo, Watcher};
pub use graph::{
    BlockContext, Endpoint, Graph, GraphConfig, GraphError, Node, NodeId, NodeInfo, ProcessorNode,
};
pub use meter::{Level, Meter, MeterHandle};
pub use mute::Mute;
pub use player::{Clip, Player, PlayerHandle, PlayerStats};
pub use recorder::{Recorder, RecorderHandle, RecorderStats};
pub use runtime::{CommitError, Controller, ParamChange, Runtime, RuntimeStats};
pub use stream::{BlockProcessor, Stats, Stream, StreamConfig, StreamInfo, MAX_BLOCK_FRAMES};
pub use strip::Strip;
pub use tempo::{Beat, MusicalPosition, TempoMap, TimeSignature, PPQ};
pub use transport::{LoopRange, TransportClock, TransportHandle};
pub use wav::WavInfo;
