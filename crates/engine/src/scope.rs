//! A capture of recent audio for the analysis stream.
//!
//! A [`Scope`] sits in the graph as a sink, like a meter: it sums what
//! flows through it to mono and writes it into a ring of atomics, leaving
//! the audio alone. The pump reads the most recent samples through a
//! [`ScopeHandle`] at display rate and analyses them off the audio
//! thread. A read that races a write sees a few samples from the newer
//! block, which a picture of the sound does not mind.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use cantina_dsp::Buffer;

use crate::graph::{BlockContext, Node};

/// Samples the ring holds: enough for one analysis window and a block or
/// two of slack. A power of two, so the index wraps with a mask.
pub const SCOPE_CAPACITY: usize = 8192;

struct Ring {
    samples: Box<[AtomicU32]>,
    /// Samples written since the start. The newest is at `written - 1`.
    written: AtomicU64,
}

/// The audio-thread half.
pub struct Scope {
    ring: Arc<Ring>,
}

/// The reader's half. Clone it freely.
#[derive(Clone)]
pub struct ScopeHandle {
    ring: Arc<Ring>,
}

impl Scope {
    /// A scope for the audio thread and its handle for the UI thread.
    pub fn new() -> (Scope, ScopeHandle) {
        let ring = Arc::new(Ring {
            samples: (0..SCOPE_CAPACITY).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicU64::new(0),
        });
        (
            Scope {
                ring: Arc::clone(&ring),
            },
            ScopeHandle { ring },
        )
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new().0
    }
}

impl Node for Scope {
    fn name(&self) -> &str {
        "Scope"
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block_frames: usize) {}

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        let frames = buffer.frames();
        let channels = buffer.channels();
        if frames == 0 || channels == 0 {
            return;
        }
        let scale = 1.0 / channels as f32;
        let start = self.ring.written.load(Ordering::Relaxed);
        for i in 0..frames {
            let mut sum = 0.0;
            for c in 0..channels {
                sum += buffer.channel(c)[i];
            }
            let at = (start as usize + i) & (SCOPE_CAPACITY - 1);
            self.ring.samples[at].store((sum * scale).to_bits(), Ordering::Relaxed);
        }
        self.ring
            .written
            .store(start + frames as u64, Ordering::Release);
    }
}

impl ScopeHandle {
    /// Fills `out` with the most recent samples, oldest first. Before
    /// that many have been written the front is silence. `out` may be at
    /// most [`SCOPE_CAPACITY`] long.
    pub fn latest(&self, out: &mut [f32]) {
        assert!(out.len() <= SCOPE_CAPACITY, "scope read of {}", out.len());
        let written = self.ring.written.load(Ordering::Acquire);
        let len = out.len() as u64;
        for (i, v) in out.iter_mut().enumerate() {
            // The sample `len - i` back from the newest.
            let back = len - i as u64;
            *v = if back > written {
                0.0
            } else {
                let at = (written - back) as usize & (SCOPE_CAPACITY - 1);
                f32::from_bits(self.ring.samples[at].load(Ordering::Relaxed))
            };
        }
    }

    /// Samples written since the start.
    pub fn written(&self) -> u64 {
        self.ring.written.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoMap;

    fn run(scope: &mut Scope, left: &mut [f32], right: &mut [f32]) {
        let tempo = TempoMap::default();
        let ctx = BlockContext {
            sample_rate: 48_000.0,
            frames: left.len(),
            span: left.len() as u64,
            rate: 1.0,
            playing: false,
            position: 0,
            loop_range: None,
            count_in: None,
            tempo: &tempo,
        };
        let mut lanes: [&mut [f32]; 2] = [left, right];
        let mut buffer = Buffer::new(&mut lanes);
        scope.process(&ctx, &mut buffer);
    }

    #[test]
    fn keeps_the_latest_samples_in_mono_and_leaves_audio_alone() {
        let (mut scope, handle) = Scope::new();
        let mut out = [9.0f32; 4];
        handle.latest(&mut out);
        assert_eq!(out, [0.0; 4], "silence before anything is written");

        let mut left = [1.0, 2.0, 3.0];
        let mut right = [1.0, 0.0, -3.0];
        run(&mut scope, &mut left, &mut right);
        assert_eq!(left, [1.0, 2.0, 3.0]);
        handle.latest(&mut out);
        assert_eq!(out, [0.0, 1.0, 1.0, 0.0]);
        assert_eq!(handle.written(), 3);
    }

    #[test]
    fn wraps_around_the_ring() {
        let (mut scope, handle) = Scope::new();
        let block = 1000;
        let mut n = 0.0f32;
        for _ in 0..20 {
            let mut left: Vec<f32> = (0..block).map(|i| n + i as f32).collect();
            let mut right = left.clone();
            run(&mut scope, &mut left, &mut right);
            n += block as f32;
        }
        let mut out = vec![0.0f32; 2048];
        handle.latest(&mut out);
        let expected: Vec<f32> = (0..2048).map(|i| n - 2048.0 + i as f32).collect();
        assert_eq!(out, expected);
    }
}
