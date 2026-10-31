//! The Cantina audio engine: CoreAudio streams, the real-time graph
//! runtime, the transport and tempo map, and later disk streaming.
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
pub mod processors;
pub mod runtime;
pub mod stream;
pub mod tempo;
pub mod transport;

pub use click::Click;
pub use device::{DeviceId, DeviceInfo};
pub use graph::{
    BlockContext, Endpoint, Graph, GraphConfig, GraphError, Node, NodeId, NodeInfo, ProcessorNode,
};
pub use runtime::{CommitError, Controller, ParamChange, Runtime, RuntimeStats};
pub use stream::{BlockProcessor, Stats, Stream, StreamConfig, StreamInfo, MAX_BLOCK_FRAMES};
pub use tempo::{Beat, MusicalPosition, TempoMap, TimeSignature, PPQ};
pub use transport::{LoopRange, TransportHandle};
