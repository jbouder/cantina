//! Plays regions of WAV files from the timeline, streamed from disk on
//! their own thread.
//!
//! A [`Player`] is a graph node. Its [`PlayerHandle`] on the UI thread
//! sets the [`Clip`]s to play. A disk thread owned by the pair opens the
//! files, renders the clips into chunks tagged with their timeline
//! position, and pushes them through a lock-free ring. The node consumes
//! whatever chunk matches the block it is asked for and adds it to its
//! buffer.
//!
//! The node never waits for the disk. When the ring holds nothing for the
//! current position it outputs silence and counts the underrun. When the
//! transport locates, or the loop changes, the node asks the disk thread
//! to start again from the new position with a new generation number, and
//! discards chunks of older generations as they come through. The disk
//! thread knows the loop range, so it prefetches across the wrap and the
//! node sees contiguous data where the transport jumps. A change to the
//! clips bumps a version the node watches, which starts a new generation
//! the same way, and the disk thread holds off rendering it until it has
//! that version of the clips.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use cantina_dsp::{Buffer, MAX_CHANNELS};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::{BlockContext, Node};
use crate::transport::LoopRange;
use crate::wav::{self, Reader};

/// A region of a file placed on the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub path: PathBuf,
    /// Timeline sample the clip starts at.
    pub start: u64,
    /// Frame into the file the clip starts from.
    pub offset: u64,
    /// Frames to play.
    pub length: u64,
}

impl Clip {
    fn end(&self) -> u64 {
        self.start + self.length
    }
}

/// Frames the disk thread renders per chunk.
const CHUNK_FRAMES: usize = 1024;
/// Seconds of audio kept ahead of the audio thread.
const PREFETCH_SECONDS: f64 = 2.0;
/// Chunks the disk thread renders before looking at its mail again, so a
/// seek is never starved by a ring the node keeps draining.
const CHUNKS_PER_FILL: usize = 8;
/// How long the disk thread sleeps when there is nothing to do.
const IDLE: Duration = Duration::from_millis(2);
const REQUEST_QUEUE: usize = 64;

/// Counters bumped by both threads. Read from anywhere.
#[derive(Debug, Default)]
pub struct PlayerStats {
    /// Frames of silence output because no chunk was ready.
    pub underrun_frames: AtomicU64,
    /// Chunks thrown away because they were from before a seek.
    pub stale_chunks: AtomicU64,
    /// Seeks the node asked the disk thread for.
    pub seeks: AtomicU64,
    /// Seek requests refused because the request queue was full.
    pub requests_dropped: AtomicU64,
    /// Chunks the disk thread has delivered.
    pub chunks: AtomicU64,
    /// Read failures on the disk thread; the chunk plays as silence.
    pub read_errors: AtomicU64,
    /// Generation the node last asked for, and the one the disk thread
    /// last rendered. Equal once the disk thread has caught up.
    pub requested_generation: AtomicU64,
    pub rendered_generation: AtomicU64,
    /// Frames of the current generation waiting in the ring, as of the
    /// node's last block.
    pub ready_frames: AtomicU64,
}

impl PlayerStats {
    /// True when the disk thread is rendering what the node asked for.
    pub fn is_caught_up(&self) -> bool {
        self.requested_generation.load(Ordering::Acquire)
            == self.rendered_generation.load(Ordering::Acquire)
    }
}

/// Audio thread to disk thread.
#[derive(Debug, Clone, Copy)]
struct Seek {
    position: u64,
    generation: u64,
    loop_range: Option<LoopRange>,
    /// Version of the clips this generation must be rendered from.
    clips_version: u64,
}

/// UI thread to disk thread.
enum Command {
    SetClips { version: u64, clips: Vec<Clip> },
}

/// Describes the next `frames * channels` samples in the data ring.
#[derive(Debug, Clone, Copy)]
struct Header {
    generation: u64,
    position: u64,
    frames: usize,
}

/// The graph node. Adds the clips under the block to its buffer.
pub struct Player {
    channels: usize,
    requests: Producer<Seek>,
    headers: Consumer<Header>,
    data: Consumer<f32>,
    stats: Arc<PlayerStats>,
    /// Timeline sample the next sample in the ring should be for.
    expected: u64,
    generation: u64,
    loop_range: Option<LoopRange>,
    /// Chunk being consumed and how much of it is left.
    current: Option<Header>,
    /// Whether a seek has been requested since the last block.
    started: bool,
    /// Bumped by the handle whenever the clips change.
    clips_version: Arc<AtomicU64>,
    seen_clips_version: u64,
}

/// The UI-thread half.
pub struct PlayerHandle {
    sample_rate: f64,
    channels: usize,
    commands: mpsc::Sender<Command>,
    stats: Arc<PlayerStats>,
    clips: Vec<Clip>,
    clips_version: Arc<AtomicU64>,
}

impl Player {
    /// Creates a player for a graph at `sample_rate` with `channels` lanes
    /// and starts its disk thread. Files play at their own rate; clips in
    /// files at another rate are refused by [`PlayerHandle::set_clips`].
    pub fn new(sample_rate: f64, channels: usize) -> (Player, PlayerHandle) {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let prefetch_frames = (sample_rate * PREFETCH_SECONDS) as usize;
        let (request_tx, request_rx) = RingBuffer::new(REQUEST_QUEUE);
        let (header_tx, header_rx) = RingBuffer::new(prefetch_frames / CHUNK_FRAMES + 2);
        let (data_tx, data_rx) = RingBuffer::new(prefetch_frames * channels);
        let (command_tx, command_rx) = mpsc::channel();
        let stats = Arc::new(PlayerStats::default());
        let clips_version = Arc::new(AtomicU64::new(0));

        let disk = DiskThread {
            channels,
            requests: request_rx,
            commands: command_rx,
            headers: header_tx,
            data: data_tx,
            stats: Arc::clone(&stats),
            readers: HashMap::new(),
            clips: Vec::new(),
            clips_version: 0,
            wanted_clips_version: 0,
            head: None,
            generation: 0,
            loop_range: None,
            scratch: Vec::with_capacity(CHUNK_FRAMES * channels),
            file_scratch: Vec::with_capacity(CHUNK_FRAMES * MAX_CHANNELS),
        };
        thread::Builder::new()
            .name("cantina-player-disk".into())
            .spawn(move || disk.run())
            .expect("spawning the player's disk thread");

        let player = Player {
            channels,
            requests: request_tx,
            headers: header_rx,
            data: data_rx,
            stats: Arc::clone(&stats),
            expected: 0,
            generation: 0,
            loop_range: None,
            current: None,
            started: false,
            clips_version: Arc::clone(&clips_version),
            seen_clips_version: 0,
        };
        let handle = PlayerHandle {
            sample_rate,
            channels,
            commands: command_tx,
            stats,
            clips: Vec::new(),
            clips_version,
        };
        (player, handle)
    }

    pub fn stats(&self) -> &Arc<PlayerStats> {
        &self.stats
    }

    /// Asks the disk thread to start again from `position`.
    fn seek(&mut self, position: u64, loop_range: Option<LoopRange>) {
        self.generation += 1;
        self.expected = position;
        self.loop_range = loop_range;
        if let Some(header) = self.current.take() {
            // Whatever was left of the chunk being consumed is stale too.
            self.skip(header.frames);
            self.stats.stale_chunks.fetch_add(1, Ordering::Relaxed);
        }
        self.started = true;
        self.stats.seeks.fetch_add(1, Ordering::Relaxed);
        self.stats
            .requested_generation
            .store(self.generation, Ordering::Release);
        let request = Seek {
            position,
            generation: self.generation,
            loop_range,
            clips_version: self.seen_clips_version,
        };
        if self.requests.push(request).is_err() {
            self.stats.requests_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Throws away chunks from before the last seek.
    fn discard_stale(&mut self) {
        loop {
            let header = match self.current {
                Some(h) => h,
                None => match self.headers.peek() {
                    Ok(h) => *h,
                    Err(_) => return,
                },
            };
            if header.generation == self.generation {
                return;
            }
            if self.current.is_none() {
                let _ = self.headers.pop();
            }
            self.current = None;
            self.skip(header.frames);
            self.stats.stale_chunks.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn skip(&mut self, frames: usize) {
        let samples = (frames * self.channels).min(self.data.slots());
        if let Ok(chunk) = self.data.read_chunk(samples) {
            chunk.commit_all();
        }
    }

    /// Mirrors the transport's wrap so the expected position stays in
    /// step with what the disk thread rendered.
    fn advance(&mut self, frames: usize) {
        self.expected += frames as u64;
        if let Some(range) = self.loop_range {
            if self.expected == range.end {
                self.expected = range.start;
            }
        }
    }
}

impl Node for Player {
    fn name(&self) -> &str {
        "Player"
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block_frames: usize) {}

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        let clips_version = self.clips_version.load(Ordering::Acquire);
        if clips_version != self.seen_clips_version {
            self.seen_clips_version = clips_version;
            self.started = false;
        }
        if !self.started || ctx.position != self.expected || ctx.loop_range != self.loop_range {
            self.seek(ctx.position, ctx.loop_range);
        }
        self.discard_stale();
        // Generations are in order, so once the head is fresh so is the rest.
        self.stats.ready_frames.store(
            (self.data.slots() / self.channels) as u64,
            Ordering::Relaxed,
        );
        if !ctx.playing {
            return;
        }

        let frames = buffer.frames();
        let channels = self.channels.min(buffer.channels());
        let mut done = 0;
        while done < frames {
            let Some(header) = self.current.or_else(|| self.headers.pop().ok()) else {
                break;
            };
            if header.generation != self.generation {
                self.current = None;
                self.skip(header.frames);
                self.stats.stale_chunks.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if header.position > self.expected {
                // The disk thread is ahead, which only happens when the
                // loop changed under it. Start again from here.
                self.current = None;
                self.skip(header.frames);
                self.stats.stale_chunks.fetch_add(1, Ordering::Relaxed);
                self.seek(self.expected, ctx.loop_range);
                break;
            }
            if header.position < self.expected {
                // The chunk starts before where we are, because blocks
                // went by as silence while it was on its way. Skip into it.
                let behind = (self.expected - header.position).min(header.frames as u64) as usize;
                self.skip(behind);
                self.current = (behind < header.frames).then_some(Header {
                    generation: header.generation,
                    position: header.position + behind as u64,
                    frames: header.frames - behind,
                });
                continue;
            }
            let take = header.frames.min(frames - done);
            let samples = take * self.channels;
            let Ok(chunk) = self.data.read_chunk(samples) else {
                // Data lags its header, which the disk thread never does.
                break;
            };
            let (first, second) = chunk.as_slices();
            for (i, sample) in first.iter().chain(second).enumerate() {
                let lane = i % self.channels;
                if lane < channels {
                    buffer.channel_mut(lane)[done + i / self.channels] += sample;
                }
            }
            chunk.commit_all();
            done += take;
            self.advance(take);
            self.current = if take < header.frames {
                Some(Header {
                    generation: header.generation,
                    position: self.expected,
                    frames: header.frames - take,
                })
            } else {
                None
            };
        }
        if done < frames {
            self.stats
                .underrun_frames
                .fetch_add((frames - done) as u64, Ordering::Relaxed);
            // Stay in step with the transport so the data lands at the
            // right place when it arrives.
            self.advance(frames - done);
        }
    }
}

impl PlayerHandle {
    pub fn stats(&self) -> &Arc<PlayerStats> {
        &self.stats
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Replaces the clips. Every file is checked here: it must exist, be
    /// at the engine's sample rate, have no more channels than the player,
    /// and hold the region the clip asks for.
    pub fn set_clips(&mut self, clips: Vec<Clip>) -> Result<()> {
        for clip in &clips {
            let info = wav::info(&clip.path)?;
            if info.sample_rate as f64 != self.sample_rate {
                bail!(
                    "{} is at {} Hz, the engine runs at {} Hz",
                    clip.path.display(),
                    info.sample_rate,
                    self.sample_rate
                );
            }
            if info.channels as usize > self.channels {
                bail!(
                    "{} has {} channels, the player has {}",
                    clip.path.display(),
                    info.channels,
                    self.channels
                );
            }
            if clip.offset + clip.length > info.frames {
                bail!(
                    "{} has {} frames, the clip wants {}..{}",
                    clip.path.display(),
                    info.frames,
                    clip.offset,
                    clip.offset + clip.length
                );
            }
        }
        let version = self.clips_version.load(Ordering::Acquire) + 1;
        self.commands
            .send(Command::SetClips {
                version,
                clips: clips.clone(),
            })
            .context("the player's disk thread has stopped")?;
        // Published after the command is queued, so a generation that
        // sees this version finds its clips waiting on the disk thread.
        self.clips_version.store(version, Ordering::Release);
        self.clips = clips;
        Ok(())
    }
}

/// State of the disk thread.
struct DiskThread {
    channels: usize,
    requests: Consumer<Seek>,
    commands: mpsc::Receiver<Command>,
    headers: Producer<Header>,
    data: Producer<f32>,
    stats: Arc<PlayerStats>,
    readers: HashMap<PathBuf, Reader>,
    clips: Vec<Clip>,
    /// Version of `clips`, and the version the current generation needs.
    clips_version: u64,
    wanted_clips_version: u64,
    /// Timeline sample the next chunk starts at, once a seek has arrived.
    head: Option<u64>,
    generation: u64,
    loop_range: Option<LoopRange>,
    /// One chunk, interleaved with the player's channel count.
    scratch: Vec<f32>,
    /// Frames read from one file, interleaved with its channel count.
    file_scratch: Vec<f32>,
}

impl DiskThread {
    fn run(mut self) {
        let mut connected = true;
        loop {
            if self.requests.is_abandoned() {
                return;
            }
            if connected {
                match self.commands.try_recv() {
                    Ok(command) => self.apply(command),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => connected = false,
                }
            }
            while let Ok(seek) = self.requests.pop() {
                self.head = Some(seek.position);
                self.generation = seek.generation;
                self.loop_range = seek.loop_range;
                self.wanted_clips_version = seek.clips_version;
            }
            if !self.fill() {
                thread::sleep(IDLE);
            }
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::SetClips { version, clips } => {
                self.clips_version = version;
                self.readers
                    .retain(|path, _| clips.iter().any(|c| &c.path == path));
                for clip in &clips {
                    if !self.readers.contains_key(&clip.path) {
                        match Reader::open(&clip.path) {
                            Ok(reader) => {
                                self.readers.insert(clip.path.clone(), reader);
                            }
                            Err(_) => {
                                self.stats.read_errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                self.clips = clips;
            }
        }
    }

    /// Renders as many chunks as the rings have room for. Returns whether
    /// anything was pushed.
    fn fill(&mut self) -> bool {
        let Some(mut head) = self.head else {
            return false;
        };
        if self.clips_version < self.wanted_clips_version {
            // The clips this generation needs are still in the mailbox.
            return false;
        }
        let mut pushed = false;
        for _ in 0..CHUNKS_PER_FILL {
            if self.headers.is_full() || self.data.slots() < CHUNK_FRAMES * self.channels {
                break;
            }
            let frames = match self.loop_range {
                Some(range) if range.contains(head) => {
                    CHUNK_FRAMES.min((range.end - head) as usize)
                }
                _ => CHUNK_FRAMES,
            };
            self.render(head, frames);
            if let Ok(chunk) = self.data.write_chunk_uninit(frames * self.channels) {
                chunk.fill_from_iter(self.scratch.iter().copied());
            }
            let header = Header {
                generation: self.generation,
                position: head,
                frames,
            };
            if self.headers.push(header).is_err() {
                break;
            }
            self.stats.chunks.fetch_add(1, Ordering::Relaxed);
            self.stats
                .rendered_generation
                .store(self.generation, Ordering::Release);
            pushed = true;
            head += frames as u64;
            if let Some(range) = self.loop_range {
                if head == range.end {
                    head = range.start;
                }
            }
        }
        self.head = Some(head);
        pushed
    }

    /// Mixes every clip under `position..position + frames` into the
    /// scratch chunk.
    fn render(&mut self, position: u64, frames: usize) {
        self.scratch.clear();
        self.scratch.resize(frames * self.channels, 0.0);
        let end = position + frames as u64;
        for clip in &self.clips {
            if clip.end() <= position || clip.start >= end {
                continue;
            }
            let Some(reader) = self.readers.get_mut(&clip.path) else {
                continue;
            };
            let from = position.max(clip.start);
            let to = end.min(clip.end());
            let file_frame = clip.offset + (from - clip.start);
            let wanted = (to - from) as usize;
            self.file_scratch.clear();
            let got = match reader.read(file_frame, wanted, &mut self.file_scratch) {
                Ok(got) => got,
                Err(_) => {
                    self.stats.read_errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            let file_channels = reader.info().channels as usize;
            let offset = (from - position) as usize;
            for frame in 0..got {
                for lane in 0..self.channels {
                    let source = lane.min(file_channels - 1);
                    self.scratch[(offset + frame) * self.channels + lane] +=
                        self.file_scratch[frame * file_channels + source];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::runtime::Runtime;
    use crate::stream::BlockProcessor;
    use crate::wav::tests::{ramp, ramp_file, tempdir};
    use std::time::Instant;

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;

    fn runtime_with_player(channels: usize) -> (Runtime, crate::runtime::Controller, PlayerHandle) {
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: BLOCK,
            channels,
        })
        .unwrap();
        let (player, handle) = Player::new(SR, channels);
        let id = controller.graph_mut().add(Box::new(player));
        controller
            .graph_mut()
            .connect(id, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();
        runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
        (runtime, controller, handle)
    }

    /// Runs stopped blocks until the disk thread has caught up with the
    /// node and a few chunks of the current generation are ready.
    fn wait_for_prefetch(runtime: &mut Runtime, handle: &PlayerHandle) {
        let stats = handle.stats();
        let start = Instant::now();
        loop {
            runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
            if stats.is_caught_up()
                && stats.ready_frames.load(Ordering::Relaxed) >= 4 * CHUNK_FRAMES as u64
            {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "disk thread never caught up: {stats:?}"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn render(runtime: &mut Runtime, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        for chunk in out.chunks_mut(BLOCK) {
            let input = vec![0.0; chunk.len()];
            runtime.process(&input, chunk);
        }
        out
    }

    fn assert_close(actual: &[f32], expected: impl Fn(usize) -> f32, what: &str) {
        for (i, a) in actual.iter().enumerate() {
            let e = expected(i);
            assert!(
                (a - e).abs() < 1e-4,
                "{what}: sample {i} was {a}, expected {e}"
            );
        }
    }

    #[test]
    fn plays_a_clip_region_at_its_timeline_position() {
        let dir = tempdir();
        let path = ramp_file(&dir, "ramp.wav", 1, 48_000, 32);
        let (mut runtime, mut controller, mut handle) = runtime_with_player(1);
        handle
            .set_clips(vec![Clip {
                path: path.clone(),
                start: 1000,
                offset: 500,
                length: 2000,
            }])
            .unwrap();
        wait_for_prefetch(&mut runtime, &handle);
        controller.transport_mut().play();
        let out = render(&mut runtime, 4000);
        assert_close(
            &out,
            |i| match i {
                1000..3000 => ramp(500 + i as u64 - 1000, 0),
                _ => 0.0,
            },
            "clip",
        );
        assert_eq!(handle.stats().underrun_frames.load(Ordering::Relaxed), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn seeks_and_loops_without_gaps() {
        let dir = tempdir();
        let path = ramp_file(&dir, "ramp.wav", 1, 48_000, 32);
        let (mut runtime, mut controller, mut handle) = runtime_with_player(1);
        handle
            .set_clips(vec![Clip {
                path,
                start: 0,
                offset: 0,
                length: 48_000,
            }])
            .unwrap();

        // Locate while stopped, then play: the region at the new position.
        controller.transport_mut().locate(10_000);
        wait_for_prefetch(&mut runtime, &handle);
        controller.transport_mut().play();
        let out = render(&mut runtime, 640);
        assert_close(&out, |i| ramp(10_000 + i as u64, 0), "after locate");

        // Loop while stopped: data crosses the wrap with no underrun.
        controller.transport_mut().stop();
        controller.transport_mut().set_loop(Some(LoopRange {
            start: 20_000,
            end: 20_100,
        }));
        controller.transport_mut().locate(20_050);
        wait_for_prefetch(&mut runtime, &handle);
        let underruns = handle.stats().underrun_frames.load(Ordering::Relaxed);
        controller.transport_mut().play();
        let out = render(&mut runtime, 1000);
        assert_close(&out, |i| ramp(20_000 + (50 + i as u64) % 100, 0), "looping");
        assert_eq!(
            handle.stats().underrun_frames.load(Ordering::Relaxed),
            underruns,
            "no underrun across the wrap"
        );
        assert_eq!(
            controller.transport().position(),
            20_000 + (50 + 1000) % 100
        );

        // Locate while playing: a gap at most, then the right data.
        controller.transport_mut().set_loop(None);
        controller.transport_mut().locate(30_000);
        let start = Instant::now();
        loop {
            let out = render(&mut runtime, BLOCK);
            let position = controller.transport().position() as usize - BLOCK;
            if out[0] != 0.0 {
                assert_close(
                    &out,
                    |i| ramp((position + i) as u64, 0),
                    "after live locate",
                );
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "never recovered from the seek"
            );
            thread::sleep(Duration::from_millis(1));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn underruns_are_counted_and_position_is_kept() {
        let dir = tempdir();
        let path = ramp_file(&dir, "ramp.wav", 1, 48_000, 32);
        let (mut runtime, mut controller, mut handle) = runtime_with_player(1);
        handle
            .set_clips(vec![Clip {
                path,
                start: 0,
                offset: 0,
                length: 48_000,
            }])
            .unwrap();
        // Play at once from a fresh position, before the disk can answer.
        controller.transport_mut().locate(5000);
        controller.transport_mut().play();
        let start = Instant::now();
        let mut played = 0usize;
        loop {
            let out = render(&mut runtime, BLOCK);
            played += BLOCK;
            if out.iter().any(|v| *v != 0.0) {
                let position = 5000 + played - BLOCK;
                assert_close(
                    &out,
                    |i| ramp((position + i) as u64, 0),
                    "once data arrived",
                );
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "never got data");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            handle.stats().underrun_frames.load(Ordering::Relaxed),
            (played - BLOCK) as u64
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn integer_and_stereo_files_map_onto_lanes() {
        let dir = tempdir();
        let stereo = ramp_file(&dir, "stereo.wav", 2, 4000, 16);
        let mono = ramp_file(&dir, "mono.wav", 1, 4000, 24);
        let (mut runtime, mut controller, mut handle) = runtime_with_player(2);
        handle
            .set_clips(vec![
                Clip {
                    path: stereo,
                    start: 0,
                    offset: 0,
                    length: 1000,
                },
                Clip {
                    path: mono,
                    start: 500,
                    offset: 100,
                    length: 1000,
                },
            ])
            .unwrap();
        wait_for_prefetch(&mut runtime, &handle);
        controller.transport_mut().play();
        let mut left = vec![0.0f32; 2000];
        let mut right = vec![0.0f32; 2000];
        for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
            let silence = vec![0.0; l.len()];
            runtime.run(&[&silence, &silence], &mut [l, r]);
        }
        let expected = |lane: u16, i: usize| {
            let mut v = 0.0;
            if i < 1000 {
                v += ramp(i as u64, lane);
            }
            if (500..1500).contains(&i) {
                v += ramp(100 + i as u64 - 500, 0);
            }
            v
        };
        assert_close(&left, |i| expected(0, i), "left");
        assert_close(&right, |i| expected(1, i), "right");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clips_are_validated() {
        let dir = tempdir();
        let path = ramp_file(&dir, "short.wav", 2, 100, 16);
        let (_player, mut handle) = Player::new(SR, 1);
        let clip = |offset, length| Clip {
            path: path.clone(),
            start: 0,
            offset,
            length,
        };
        let err = handle.set_clips(vec![clip(0, 10)]).unwrap_err();
        assert!(err.to_string().contains("channels"), "{err}");
        let (_player, mut handle) = Player::new(SR, 2);
        let err = handle.set_clips(vec![clip(50, 51)]).unwrap_err();
        assert!(err.to_string().contains("frames"), "{err}");
        let (_player, mut handle) = Player::new(44_100.0, 2);
        let err = handle.set_clips(vec![clip(0, 10)]).unwrap_err();
        assert!(err.to_string().contains("Hz"), "{err}");
        let err = handle
            .set_clips(vec![Clip {
                path: dir.join("missing.wav"),
                start: 0,
                offset: 0,
                length: 1,
            }])
            .unwrap_err();
        assert!(err.to_string().contains("opening"), "{err}");
        let (_player, mut handle) = Player::new(SR, 2);
        handle.set_clips(vec![clip(50, 50)]).unwrap();
        assert_eq!(handle.clips().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
