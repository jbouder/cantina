//! Retroactive capture: the last minutes of a signal, always.
//!
//! A [`Capture`] is a graph node that passes its input through and writes
//! every frame into a [`CaptureRing`], a ring of samples allocated whole
//! before the node goes into a graph. The audio thread only ever writes:
//! one relaxed atomic store per sample and a count of frames written per
//! block, so a capture never allocates, locks or waits. The samples are
//! atomics so the thread that reads them back, to keep what was played,
//! can do so while the ring goes on filling.
//!
//! [`KeepJob`] reads a span of one or more rings into WAV files. It runs
//! on a recorder's writer thread ([`crate::RecorderHandle::keep`]), never
//! on the audio thread, and checks as it goes that the ring has not come
//! round over what it is copying.
//!
//! The ring also remembers where the transport was: the frame it started
//! rolling on and the timeline sample of that frame, so what was kept can
//! be put back on the timeline where it was played.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use cantina_dsp::{Buffer, MAX_CHANNELS};
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::graph::{BlockContext, Node};
use crate::stream::MAX_BLOCK_FRAMES;

/// Frames copied at a time when keeping.
const COPY_FRAMES: usize = 8192;

/// Where the transport last started rolling, as the ring saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    /// The ring's frame count when it started.
    pub frame: u64,
    /// The timeline sample of that frame.
    pub position: u64,
    /// Still rolling, with no jump since.
    pub rolling: bool,
}

/// The last `frames` frames of a signal, `channels` samples each.
#[derive(Debug)]
pub struct CaptureRing {
    channels: usize,
    frames: usize,
    samples: Box<[AtomicU32]>,
    /// Frames written since the ring was made; the newest is at
    /// `written - 1`, the oldest still held at `written - frames`.
    written: AtomicU64,
    /// A seqlock over the anchor: odd while the audio thread writes it.
    anchor_seq: AtomicU64,
    anchor_frame: AtomicU64,
    anchor_position: AtomicU64,
    anchor_rolling: AtomicBool,
}

impl CaptureRing {
    /// A silent ring of `frames` frames of `channels` lanes. This is the
    /// allocation: make it before the node goes into a graph.
    pub fn new(channels: usize, frames: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let frames = frames.max(1);
        let samples = vec![0u32; channels * frames]
            .into_iter()
            .map(AtomicU32::new)
            .collect();
        Self {
            channels,
            frames,
            samples,
            written: AtomicU64::new(0),
            anchor_seq: AtomicU64::new(0),
            anchor_frame: AtomicU64::new(0),
            anchor_position: AtomicU64::new(0),
            anchor_rolling: AtomicBool::new(false),
        }
    }

    /// Lanes per frame.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Frames the ring holds when full.
    pub fn capacity(&self) -> usize {
        self.frames
    }

    /// Frames written since the ring was made.
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    /// The oldest frame the ring surely still holds. A block being
    /// written overwrites frames before it counts them, so this leaves a
    /// block's room.
    pub fn oldest(&self) -> u64 {
        (self.written() + MAX_BLOCK_FRAMES as u64).saturating_sub(self.frames as u64)
    }

    /// Where the transport last started rolling; none if it never has
    /// while this ring was filling.
    pub fn anchor(&self) -> Option<Anchor> {
        loop {
            let before = self.anchor_seq.load(Ordering::Acquire);
            if before == 0 {
                return None;
            }
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let anchor = Anchor {
                frame: self.anchor_frame.load(Ordering::Relaxed),
                position: self.anchor_position.load(Ordering::Relaxed),
                rolling: self.anchor_rolling.load(Ordering::Relaxed),
            };
            std::sync::atomic::fence(Ordering::Acquire);
            if self.anchor_seq.load(Ordering::Relaxed) == before {
                return Some(anchor);
            }
        }
    }

    fn set_anchor(&self, frame: u64, position: u64, rolling: bool) {
        self.anchor_seq.fetch_add(1, Ordering::AcqRel);
        self.anchor_frame.store(frame, Ordering::Relaxed);
        self.anchor_position.store(position, Ordering::Relaxed);
        self.anchor_rolling.store(rolling, Ordering::Relaxed);
        self.anchor_seq.fetch_add(1, Ordering::Release);
    }

    /// Copies frames `from..from + frames` into `out`, interleaved. False
    /// when any of them is no longer held, or not written yet.
    pub fn read(&self, from: u64, frames: usize, out: &mut Vec<f32>) -> bool {
        out.clear();
        let end = from + frames as u64;
        if from < self.oldest() || end > self.written() {
            return false;
        }
        for frame in from..end {
            let at = (frame % self.frames as u64) as usize * self.channels;
            for lane in 0..self.channels {
                out.push(f32::from_bits(
                    self.samples[at + lane].load(Ordering::Relaxed),
                ));
            }
        }
        // Copied while the ring went on: still good only if the oldest
        // frame held has not passed what was copied.
        self.oldest() <= from
    }

    /// Writes one block, the audio thread's only part.
    fn write(&self, buffer: &Buffer) {
        let start = self.written.load(Ordering::Relaxed);
        let lanes = buffer.channels();
        for frame in 0..buffer.frames() {
            let at = ((start + frame as u64) % self.frames as u64) as usize * self.channels;
            for lane in 0..self.channels {
                let v = if lane < lanes {
                    buffer.channel(lane)[frame]
                } else {
                    0.0
                };
                self.samples[at + lane].store(v.to_bits(), Ordering::Relaxed);
            }
        }
        self.written
            .store(start + buffer.frames() as u64, Ordering::Release);
    }
}

/// The graph node: passes its input through and keeps it in the ring.
pub struct Capture {
    ring: Arc<CaptureRing>,
    /// The timeline sample the next block starts on if the transport
    /// rolls on, to tell a jump from a run.
    next: Option<u64>,
}

impl Capture {
    /// A node writing into `ring`.
    pub fn new(ring: Arc<CaptureRing>) -> Self {
        Self { ring, next: None }
    }
}

impl Node for Capture {
    fn name(&self) -> &str {
        "Capture"
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block_frames: usize) {}

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        let frame = self.ring.written.load(Ordering::Relaxed);
        if ctx.playing {
            if self.next != Some(ctx.position) {
                self.ring.set_anchor(frame, ctx.position, true);
            }
            self.next = Some(ctx.position + ctx.span);
        } else if self.next.take().is_some() {
            if let Some(a) = self.ring.anchor() {
                self.ring.set_anchor(a.frame, a.position, false);
            }
        }
        self.ring.write(buffer);
    }
}

/// What a keep wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    /// The files, one per ring, in the job's order.
    pub files: Vec<PathBuf>,
    /// Frames in each.
    pub frames: u64,
}

/// Frames `from..to` of each ring, into its own 32-bit float WAV file.
pub struct KeepJob {
    /// Each ring and where its frames go.
    pub rings: Vec<(Arc<CaptureRing>, PathBuf)>,
    /// The first frame to keep.
    pub from: u64,
    /// The frame after the last.
    pub to: u64,
    /// The files' sample rate.
    pub sample_rate: f64,
    /// Where the result goes when the files are written.
    pub done: std::sync::mpsc::Sender<Result<Kept, String>>,
}

impl KeepJob {
    /// Writes the files. Runs on a writer thread.
    pub fn run(self) {
        let result = self.write().map_err(|e| format!("{e:#}"));
        let _ = self.done.send(result);
    }

    fn write(&self) -> Result<Kept> {
        if self.to <= self.from {
            bail!("there is nothing to keep");
        }
        let mut chunk = Vec::with_capacity(COPY_FRAMES * MAX_CHANNELS);
        for (ring, path) in &self.rings {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            let spec = WavSpec {
                channels: ring.channels() as u16,
                sample_rate: self.sample_rate.round() as u32,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            };
            let mut writer = WavWriter::create(path, spec)
                .with_context(|| format!("creating {}", path.display()))?;
            let mut at = self.from;
            while at < self.to {
                let n = ((self.to - at) as usize).min(COPY_FRAMES);
                if !ring.read(at, n, &mut chunk) {
                    drop(writer);
                    let _ = std::fs::remove_file(path);
                    bail!("the capture came round over what was being kept");
                }
                for s in &chunk {
                    writer.write_sample(*s)?;
                }
                at += n as u64;
            }
            writer
                .finalize()
                .with_context(|| format!("finishing {}", path.display()))?;
        }
        Ok(Kept {
            files: self.rings.iter().map(|(_, p)| p.clone()).collect(),
            frames: self.to - self.from,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::runtime::Runtime;
    use crate::stream::BlockProcessor;
    use crate::wav::tests::tempdir;

    #[test]
    fn the_ring_keeps_the_last_frames_and_knows_what_it_lost() {
        let cap = MAX_BLOCK_FRAMES + 8;
        let ring = CaptureRing::new(2, cap);
        let mut a = [0.0f32; 5];
        let mut b = [0.0f32; 5];
        let rounds = cap / 5 + 3;
        for round in 0..rounds {
            for (i, (x, y)) in a.iter_mut().zip(b.iter_mut()).enumerate() {
                *x = (round * 5 + i) as f32;
                *y = -*x;
            }
            let mut lanes = [&mut a[..], &mut b[..]];
            ring.write(&Buffer::new(&mut lanes));
        }
        let written = (rounds * 5) as u64;
        // A block's room is left below the oldest frame overwritten.
        assert_eq!((ring.written(), ring.oldest()), (written, written - 8));
        let mut out = Vec::new();
        let from = written - 8;
        assert!(ring.read(from, 8, &mut out));
        assert_eq!(out[..2], [from as f32, -(from as f32)]);
        assert_eq!(out[14..], [(written - 1) as f32, -((written - 1) as f32)]);
        // Gone, or not there yet.
        assert!(!ring.read(from - 1, 2, &mut out));
        assert!(!ring.read(written - 1, 2, &mut out));
    }

    #[test]
    fn the_node_passes_through_and_anchors_where_the_transport_rolled() {
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: 48_000.0,
            max_block_frames: 64,
            channels: 1,
        })
        .unwrap();
        let ring = Arc::new(CaptureRing::new(1, 1000));
        let id = controller
            .graph_mut()
            .add(Box::new(Capture::new(Arc::clone(&ring))));
        controller.graph_mut().connect(Endpoint::Input, id).unwrap();
        controller
            .graph_mut()
            .connect(id, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();
        let input: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let mut out = vec![0.0; 64];
        // Stopped: captured, but never anchored.
        runtime.process(&input, &mut out);
        runtime.process(&input, &mut out);
        assert_eq!(ring.anchor(), None);
        controller.transport_mut().locate(10_000);
        controller.transport_mut().play();
        runtime.process(&input, &mut out);
        runtime.process(&input, &mut out);
        assert_eq!(out[5], 5.0);
        let anchor = ring.anchor().unwrap();
        assert!(anchor.rolling);
        // The third block, from frame 128, is the first that rolled.
        assert_eq!((anchor.frame, anchor.position), (128, 10_000));
        controller.transport_mut().stop();
        runtime.process(&input, &mut out);
        let stopped = ring.anchor().unwrap();
        assert!(!stopped.rolling);
        assert_eq!(stopped.frame, 128);
        assert_eq!(ring.written(), 64 * 5);
    }

    #[test]
    fn a_keep_writes_each_ring_to_its_file() {
        let dir = tempdir();
        let mono = Arc::new(CaptureRing::new(1, MAX_BLOCK_FRAMES + 100));
        let stereo = Arc::new(CaptureRing::new(2, MAX_BLOCK_FRAMES + 100));
        let mut a: Vec<f32> = (0..60).map(|i| i as f32 / 100.0).collect();
        let mut b: Vec<f32> = a.iter().map(|v| -v).collect();
        mono.write(&Buffer::new(&mut [&mut a[..]]));
        stereo.write(&Buffer::new(&mut [&mut a[..], &mut b[..]]));
        let (done, result) = std::sync::mpsc::channel();
        KeepJob {
            rings: vec![
                (Arc::clone(&mono), dir.join("dry.wav")),
                (Arc::clone(&stereo), dir.join("wet.wav")),
            ],
            from: 10,
            to: 60,
            sample_rate: 48_000.0,
            done,
        }
        .run();
        let kept = result.recv().unwrap().unwrap();
        assert_eq!(kept.frames, 50);
        let (info, channels) = crate::wav::read_all(&dir.join("wet.wav")).unwrap();
        assert_eq!((info.channels, info.frames), (2, 50));
        assert_eq!((channels[0][0], channels[1][49]), (0.1, -0.59));
        let (info, channels) = crate::wav::read_all(&dir.join("dry.wav")).unwrap();
        assert_eq!((info.channels, channels[0][0]), (1, 0.1));
        // Asking for frames the ring no longer holds fails, and leaves no file.
        for _ in 0..MAX_BLOCK_FRAMES / 60 + 2 {
            mono.write(&Buffer::new(&mut [&mut a[..]]));
        }
        let (done, result) = std::sync::mpsc::channel();
        KeepJob {
            rings: vec![(Arc::clone(&mono), dir.join("old.wav"))],
            from: 0,
            to: mono.written(),
            sample_rate: 48_000.0,
            done,
        }
        .run();
        assert!(result.recv().unwrap().unwrap_err().contains("came round"));
        assert!(!dir.join("old.wav").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
