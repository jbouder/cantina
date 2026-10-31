//! Records a graph tap to a WAV file on a writer thread.
//!
//! A [`Recorder`] is a graph node that passes its input through untouched
//! and, while armed, copies it into a lock-free ring. A writer thread
//! owned by the pair drains the ring into a 32-bit float WAV file. The
//! [`RecorderHandle`] on the UI thread starts and stops recordings.
//!
//! The recording starts at an exact timeline sample: the node begins
//! copying at that frame of the block that contains it, so the file's
//! first sample is that transport position. A full ring never blocks the
//! audio thread: the frames are counted as an overrun, and the writer
//! pads the file with that many zeros so what follows stays aligned.
//!
//! A recording ends with an end marker the node pushes on stop, or when
//! the node goes away, which is what a device change does to the graph
//! that holds it. Either way the writer finalizes the file, and `hound`
//! finalizes on drop as well, so the header is right even then.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cantina_dsp::{Buffer, MAX_CHANNELS};
use hound::{SampleFormat, WavSpec, WavWriter};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::graph::{BlockContext, Node};

/// Seconds of audio the ring can hold while the writer thread is busy.
const RING_SECONDS: f64 = 2.0;
/// Blocks' worth of headers the ring can hold.
const HEADER_CAPACITY: usize = 4096;
const COMMAND_QUEUE: usize = 16;
const IDLE: Duration = Duration::from_millis(2);

/// Counters bumped by both threads. Read from anywhere.
#[derive(Debug, Default)]
pub struct RecorderStats {
    /// True from the first recorded frame until stop.
    pub recording: AtomicBool,
    /// Timeline sample of the first recorded frame of the current or last
    /// recording.
    pub started_at: AtomicU64,
    /// Frames the node copied into the ring.
    pub captured_frames: AtomicU64,
    /// Frames lost because the ring was full; the file holds zeros for them.
    pub overrun_frames: AtomicU64,
    /// Frames the writer has put in the file, padding included.
    pub written_frames: AtomicU64,
    /// Files the writer has finalized.
    pub files_closed: AtomicU64,
    pub write_errors: AtomicU64,
}

/// UI thread to audio thread.
#[derive(Debug, Clone, Copy)]
enum Command {
    Arm { generation: u64, from: u64 },
    Disarm,
}

/// UI thread to writer thread.
enum WriterCommand {
    Open { generation: u64, path: PathBuf },
}

/// Describes the next `frames * channels` samples in the data ring.
#[derive(Debug, Clone, Copy)]
struct Header {
    generation: u64,
    /// Frames lost before this chunk, to be written as silence.
    dropped_before: u64,
    frames: usize,
    /// The recording ends here; `frames` is zero.
    end: bool,
}

/// The graph node.
pub struct Recorder {
    channels: usize,
    commands: Consumer<Command>,
    headers: Producer<Header>,
    data: Producer<f32>,
    stats: Arc<RecorderStats>,
    armed: Option<Armed>,
}

#[derive(Debug, Clone, Copy)]
struct Armed {
    generation: u64,
    from: u64,
    started: bool,
    /// Frames dropped since the last header that carried them.
    dropped: u64,
}

/// The UI-thread half.
pub struct RecorderHandle {
    commands: Producer<Command>,
    writer: mpsc::Sender<WriterCommand>,
    stats: Arc<RecorderStats>,
    generation: u64,
    armed: bool,
}

impl Recorder {
    /// Creates a recorder for a graph at `sample_rate` with `channels`
    /// lanes and starts its writer thread.
    pub fn new(sample_rate: f64, channels: usize) -> (Recorder, RecorderHandle) {
        Self::with_ring(sample_rate, channels, (sample_rate * RING_SECONDS) as usize)
    }

    /// As [`Recorder::new`], with a ring of `ring_frames` frames.
    pub fn with_ring(
        sample_rate: f64,
        channels: usize,
        ring_frames: usize,
    ) -> (Recorder, RecorderHandle) {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let (command_tx, command_rx) = RingBuffer::new(COMMAND_QUEUE);
        let (header_tx, header_rx) = RingBuffer::new(HEADER_CAPACITY);
        let (data_tx, data_rx) = RingBuffer::new(ring_frames.max(1) * channels);
        let (writer_tx, writer_rx) = mpsc::channel();
        let stats = Arc::new(RecorderStats::default());

        let writer = WriterThread {
            spec: WavSpec {
                channels: channels as u16,
                sample_rate: sample_rate as u32,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
            channels,
            commands: writer_rx,
            headers: header_rx,
            data: data_rx,
            stats: Arc::clone(&stats),
            open: None,
        };
        thread::Builder::new()
            .name("cantina-recorder-writer".into())
            .spawn(move || writer.run())
            .expect("spawning the recorder's writer thread");

        let recorder = Recorder {
            channels,
            commands: command_rx,
            headers: header_tx,
            data: data_tx,
            stats: Arc::clone(&stats),
            armed: None,
        };
        let handle = RecorderHandle {
            commands: command_tx,
            writer: writer_tx,
            stats,
            generation: 0,
            armed: false,
        };
        (recorder, handle)
    }

    pub fn stats(&self) -> &Arc<RecorderStats> {
        &self.stats
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Arm { generation, from } => {
                    self.finish();
                    self.armed = Some(Armed {
                        generation,
                        from,
                        started: false,
                        dropped: 0,
                    });
                }
                Command::Disarm => self.finish(),
            }
        }
    }

    /// Ends the current recording with an end marker.
    fn finish(&mut self) {
        if let Some(armed) = self.armed.take() {
            let end = Header {
                generation: armed.generation,
                dropped_before: armed.dropped,
                frames: 0,
                end: true,
            };
            // A full header ring loses the marker; the writer then closes
            // the file when the next recording opens or the node goes away.
            let _ = self.headers.push(end);
            self.stats.recording.store(false, Ordering::Relaxed);
        }
    }

    /// Copies frames `first..` of the buffer into the ring.
    fn capture(&mut self, buffer: &Buffer, first: usize) {
        let Some(armed) = self.armed.as_mut() else {
            return;
        };
        let frames = buffer.frames() - first;
        if frames == 0 {
            return;
        }
        let samples = frames * self.channels;
        if self.headers.is_full() || self.data.slots() < samples {
            armed.dropped += frames as u64;
            self.stats
                .overrun_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
            return;
        }
        if let Ok(chunk) = self.data.write_chunk_uninit(samples) {
            let channels = self.channels;
            let lanes = buffer.channels();
            chunk.fill_from_iter((first..buffer.frames()).flat_map(|frame| {
                (0..channels).map(move |lane| {
                    if lane < lanes {
                        buffer.channel(lane)[frame]
                    } else {
                        0.0
                    }
                })
            }));
        }
        let header = Header {
            generation: armed.generation,
            dropped_before: std::mem::take(&mut armed.dropped),
            frames,
            end: false,
        };
        let _ = self.headers.push(header);
        self.stats
            .captured_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
    }
}

impl Node for Recorder {
    fn name(&self) -> &str {
        "Recorder"
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block_frames: usize) {}

    fn process(&mut self, ctx: &BlockContext, buffer: &mut Buffer) {
        self.apply_commands();
        let Some(armed) = self.armed else {
            return;
        };
        if !ctx.playing {
            return;
        }
        let end = ctx.position + buffer.frames() as u64;
        let first = if armed.started {
            0
        } else if armed.from < end {
            // The recording starts in this block, or should have already.
            let start = armed.from.max(ctx.position);
            self.stats.started_at.store(start, Ordering::Relaxed);
            self.stats.recording.store(true, Ordering::Relaxed);
            if let Some(a) = self.armed.as_mut() {
                a.started = true;
            }
            (start - ctx.position) as usize
        } else {
            return;
        };
        self.capture(buffer, first);
    }
}

impl RecorderHandle {
    pub fn stats(&self) -> &Arc<RecorderStats> {
        &self.stats
    }

    /// True between `start` and `stop`, whether or not the transport has
    /// reached the start position yet.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Starts a recording into `path` from timeline sample `from`. If the
    /// transport is already past `from` the recording starts at once. Any
    /// recording in progress is stopped first.
    pub fn start(&mut self, path: PathBuf, from: u64) -> Result<()> {
        self.generation += 1;
        let generation = self.generation;
        // The writer learns of the file before the node can send a frame
        // for it, because this send comes first.
        self.writer
            .send(WriterCommand::Open { generation, path })
            .context("the recorder's writer thread has stopped")?;
        self.commands
            .push(Command::Arm { generation, from })
            .map_err(|_| anyhow::anyhow!("the audio thread is not taking commands"))?;
        self.armed = true;
        Ok(())
    }

    /// Stops the recording. The node ends it at its next block, after its
    /// last frames, and the writer thread finalizes the file once they have
    /// reached it. A node that never runs another block, because its
    /// stream closed, ends it by going away instead.
    pub fn stop(&mut self) -> Result<()> {
        self.armed = false;
        self.commands
            .push(Command::Disarm)
            .map_err(|_| anyhow::anyhow!("the audio thread is not taking commands"))
    }
}

struct WriterThread {
    spec: WavSpec,
    channels: usize,
    commands: mpsc::Receiver<WriterCommand>,
    headers: Consumer<Header>,
    data: Consumer<f32>,
    stats: Arc<RecorderStats>,
    open: Option<(u64, WavWriter<BufWriter<File>>)>,
}

impl WriterThread {
    fn run(mut self) {
        let mut connected = true;
        loop {
            // Every command first, so a chunk never arrives ahead of the
            // file it belongs to.
            while connected {
                match self.commands.try_recv() {
                    Ok(WriterCommand::Open { generation, path }) => self.open(generation, path),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => connected = false,
                }
            }
            let mut busy = false;
            while let Ok(header) = self.headers.pop() {
                busy = true;
                self.write(header);
            }
            if self.headers.is_abandoned() && self.headers.is_empty() {
                self.close();
                return;
            }
            if !busy {
                thread::sleep(IDLE);
            }
        }
    }

    fn open(&mut self, generation: u64, path: PathBuf) {
        self.close();
        match WavWriter::create(&path, self.spec) {
            Ok(writer) => self.open = Some((generation, writer)),
            Err(_) => {
                self.stats.write_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn close(&mut self) {
        if let Some((_, writer)) = self.open.take() {
            match writer.finalize() {
                Ok(()) => {
                    self.stats.files_closed.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    self.stats.write_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn write(&mut self, header: Header) {
        let samples = header.frames * self.channels;
        let matches = self
            .open
            .as_ref()
            .is_some_and(|(g, _)| *g == header.generation);
        if !matches {
            // A chunk for a file that failed to open or was already closed.
            if let Ok(chunk) = self.data.read_chunk(samples.min(self.data.slots())) {
                chunk.commit_all();
            }
            return;
        }
        let (_, writer) = self.open.as_mut().expect("checked above");
        let mut failed = false;
        for _ in 0..header.dropped_before * self.channels as u64 {
            failed |= writer.write_sample(0.0f32).is_err();
        }
        if let Ok(chunk) = self.data.read_chunk(samples) {
            let (first, second) = chunk.as_slices();
            for sample in first.iter().chain(second) {
                failed |= writer.write_sample(*sample).is_err();
            }
            chunk.commit_all();
        } else {
            failed = true;
        }
        if failed {
            self.stats.write_errors.fetch_add(1, Ordering::Relaxed);
        }
        self.stats.written_frames.fetch_add(
            header.dropped_before + header.frames as u64,
            Ordering::Relaxed,
        );
        if header.end {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::runtime::{Controller, Runtime};
    use crate::stream::BlockProcessor;
    use crate::wav::tests::{ramp, tempdir};
    use std::path::Path;
    use std::time::Instant;

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;

    fn runtime_with_recorder(ring: usize) -> (Runtime, Controller, RecorderHandle) {
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: BLOCK,
            channels: 1,
        })
        .unwrap();
        let (recorder, handle) = Recorder::with_ring(SR, 1, ring);
        let id = controller.graph_mut().add(Box::new(recorder));
        controller.graph_mut().set_chain(&[id]).unwrap();
        controller.commit().unwrap();
        runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
        (runtime, controller, handle)
    }

    /// Plays `frames` frames of the ramp from the transport's position,
    /// returning the graph output.
    fn play(runtime: &mut Runtime, controller: &Controller, frames: usize) -> Vec<f32> {
        let mut position = controller.transport().position();
        let mut out = vec![0.0; frames];
        for chunk in out.chunks_mut(BLOCK) {
            let input: Vec<f32> = (0..chunk.len())
                .map(|i| ramp(position + i as u64, 0))
                .collect();
            runtime.process(&input, chunk);
            position += chunk.len() as u64;
        }
        out
    }

    /// One block for the node to take up a command.
    fn settle(runtime: &mut Runtime) {
        runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
    }

    fn wait_for_close(handle: &RecorderHandle, count: u64) {
        let start = Instant::now();
        while handle.stats().files_closed.load(Ordering::Relaxed) < count {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "file never closed: {:?}",
                handle.stats()
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn read_all(path: &Path) -> (hound::WavSpec, Vec<f32>) {
        let mut reader = hound::WavReader::open(path).unwrap();
        let samples = reader.samples::<f32>().map(|s| s.unwrap()).collect();
        (reader.spec(), samples)
    }

    #[test]
    fn records_from_an_exact_position_and_passes_audio_through() {
        let dir = tempdir();
        let path = dir.join("take.wav");
        let (mut runtime, mut controller, mut handle) = runtime_with_recorder(96_000);
        controller.transport_mut().play();
        // Roll for a while before arming, then start 10 frames into a block.
        play(&mut runtime, &controller, BLOCK * 3);
        handle.start(path.clone(), (BLOCK * 5 + 10) as u64).unwrap();
        assert!(handle.is_armed());
        let out = play(&mut runtime, &controller, BLOCK * 10);
        for (i, o) in out.iter().enumerate() {
            assert_eq!(*o, ramp((BLOCK * 3 + i) as u64, 0), "pass-through at {i}");
        }
        handle.stop().unwrap();
        assert!(!handle.is_armed());
        settle(&mut runtime);
        wait_for_close(&handle, 1);
        let (spec, samples) = read_all(&path);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 32);
        assert_eq!(spec.sample_format, SampleFormat::Float);
        let from = BLOCK * 5 + 10;
        assert_eq!(samples.len(), BLOCK * 13 - from);
        for (i, s) in samples.iter().enumerate() {
            assert_eq!(*s, ramp((from + i) as u64, 0), "sample {i}");
        }
        let stats = handle.stats();
        assert_eq!(stats.started_at.load(Ordering::Relaxed), from as u64);
        assert_eq!(stats.overrun_frames.load(Ordering::Relaxed), 0);
        assert_eq!(
            stats.written_frames.load(Ordering::Relaxed),
            samples.len() as u64
        );
        assert!(!stats.recording.load(Ordering::Relaxed));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn overruns_are_counted_and_padded() {
        let dir = tempdir();
        let path = dir.join("overrun.wav");
        // A ring of 200 frames and a burst of blocks the writer cannot keep
        // up with, since it sleeps between looks.
        let (mut runtime, mut controller, mut handle) = runtime_with_recorder(200);
        handle.start(path.clone(), 0).unwrap();
        controller.transport_mut().play();
        let total = BLOCK * 400;
        play(&mut runtime, &controller, total);
        handle.stop().unwrap();
        settle(&mut runtime);
        wait_for_close(&handle, 1);
        let stats = handle.stats();
        let overrun = stats.overrun_frames.load(Ordering::Relaxed);
        assert!(overrun > 0, "the burst should have overrun");
        let (_, samples) = read_all(&path);
        assert_eq!(samples.len(), total, "padding keeps the length");
        // Every sample is either the right ramp value or a padding zero.
        let mut padded = 0u64;
        for (i, s) in samples.iter().enumerate() {
            if *s == 0.0 && i > 0 {
                padded += 1;
            } else {
                assert_eq!(*s, ramp(i as u64, 0), "sample {i}");
            }
        }
        assert_eq!(padded, overrun);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dropping_the_node_closes_the_file_cleanly() {
        let dir = tempdir();
        let path = dir.join("unplugged.wav");
        let (mut runtime, mut controller, mut handle) = runtime_with_recorder(96_000);
        handle.start(path.clone(), 0).unwrap();
        controller.transport_mut().play();
        play(&mut runtime, &controller, BLOCK * 20);
        // Wait until the writer has everything, then pull the graph out
        // from under the recording the way a device change would.
        let start = Instant::now();
        while handle.stats().written_frames.load(Ordering::Relaxed) < (BLOCK * 20) as u64 {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(1));
        }
        drop(runtime);
        drop(controller);
        wait_for_close(&handle, 1);
        let (_, samples) = read_all(&path);
        assert_eq!(samples.len(), BLOCK * 20);
        assert_eq!(samples[BLOCK * 20 - 1], ramp((BLOCK * 20 - 1) as u64, 0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restarting_closes_the_previous_file_and_stopped_transport_records_nothing() {
        let dir = tempdir();
        let first = dir.join("first.wav");
        let second = dir.join("second.wav");
        let (mut runtime, mut controller, mut handle) = runtime_with_recorder(96_000);
        handle.start(first.clone(), 0).unwrap();
        // The transport is stopped, so nothing is captured.
        for _ in 0..10 {
            runtime.process(&[1.0; BLOCK], &mut [0.0; BLOCK]);
        }
        handle.start(second.clone(), 0).unwrap();
        settle(&mut runtime);
        wait_for_close(&handle, 1);
        assert_eq!(read_all(&first).1.len(), 0, "a valid, empty file");
        controller.transport_mut().play();
        play(&mut runtime, &controller, BLOCK * 4);
        handle.stop().unwrap();
        settle(&mut runtime);
        wait_for_close(&handle, 2);
        assert_eq!(read_all(&second).1.len(), BLOCK * 4);
        assert_eq!(
            handle.stats().captured_frames.load(Ordering::Relaxed),
            (BLOCK * 4) as u64
        );
        // A file that cannot be opened is a write error, not a crash.
        handle.start(dir.join("no/such/dir/x.wav"), 0).unwrap();
        play(&mut runtime, &controller, BLOCK * 2);
        handle.stop().unwrap();
        settle(&mut runtime);
        let start = Instant::now();
        while handle.stats().write_errors.load(Ordering::Relaxed) == 0 {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(1));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stereo_lanes_interleave() {
        let dir = tempdir();
        let path = dir.join("stereo.wav");
        let (mut runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: BLOCK,
            channels: 2,
        })
        .unwrap();
        let (recorder, mut handle) = Recorder::new(SR, 2);
        let id = controller.graph_mut().add(Box::new(recorder));
        controller.graph_mut().connect(Endpoint::Input, id).unwrap();
        controller.commit().unwrap();
        handle.start(path.clone(), 0).unwrap();
        controller.transport_mut().play();
        let left: Vec<f32> = (0..BLOCK).map(|i| ramp(i as u64, 0)).collect();
        let right: Vec<f32> = (0..BLOCK).map(|i| ramp(i as u64, 1)).collect();
        let (mut out_l, mut out_r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        runtime.run(&[&left, &right], &mut [&mut out_l, &mut out_r]);
        handle.stop().unwrap();
        settle(&mut runtime);
        wait_for_close(&handle, 1);
        let (spec, samples) = read_all(&path);
        assert_eq!(spec.channels, 2);
        assert_eq!(samples.len(), BLOCK * 2);
        for i in 0..BLOCK {
            assert_eq!(samples[2 * i], ramp(i as u64, 0));
            assert_eq!(samples[2 * i + 1], ramp(i as u64, 1));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
