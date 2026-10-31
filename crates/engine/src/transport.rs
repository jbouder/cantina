//! The transport: play, stop, locate and loop, plus the tempo map the
//! audio thread reads.
//!
//! [`Transport`] lives on the audio thread inside the runtime.
//! [`TransportHandle`] lives on the UI thread and sends it commands over a
//! lock-free queue; commands land at the start of the next block. State the
//! UI wants to show (playing, position, loop range) is published through
//! atomics after every block, so reading it never touches the audio
//! thread.
//!
//! A block is cut into segments at the loop end so a loop is
//! sample-accurate: the frames before the wrap and the frames after it are
//! processed as two blocks with their own positions.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::tempo::{MusicalPosition, TempoMap};

/// Commands that can wait between two blocks.
const COMMAND_QUEUE: usize = 64;

/// A loop between two timeline samples, `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopRange {
    pub start: u64,
    pub end: u64,
}

impl LoopRange {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn contains(&self, sample: u64) -> bool {
        (self.start..self.end).contains(&sample)
    }
}

/// What the UI asks of the transport.
pub enum TransportCommand {
    Play,
    Stop,
    Locate(u64),
    SetLoop(Option<LoopRange>),
    SetTempoMap(Box<TempoMap>),
}

/// Transport state published by the audio thread.
#[derive(Debug, Default)]
struct Shared {
    playing: AtomicBool,
    position: AtomicU64,
    loop_enabled: AtomicBool,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
}

/// A contiguous run of frames within one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub frames: usize,
    /// Timeline sample of the segment's first frame.
    pub position: u64,
    pub playing: bool,
}

/// The audio-thread half.
pub struct Transport {
    shared: Arc<Shared>,
    commands: Consumer<TransportCommand>,
    tempo: Box<TempoMap>,
    playing: bool,
    position: u64,
    loop_range: Option<LoopRange>,
}

/// The UI-thread half.
pub struct TransportHandle {
    shared: Arc<Shared>,
    commands: Producer<TransportCommand>,
    tempo: TempoMap,
}

impl Transport {
    pub fn new(tempo: TempoMap) -> (Transport, TransportHandle) {
        let shared = Arc::new(Shared::default());
        let (producer, consumer) = RingBuffer::new(COMMAND_QUEUE);
        let transport = Transport {
            shared: Arc::clone(&shared),
            commands: consumer,
            tempo: Box::new(tempo.clone()),
            playing: false,
            position: 0,
            loop_range: None,
        };
        let handle = TransportHandle {
            shared,
            commands: producer,
            tempo,
        };
        (transport, handle)
    }

    /// Applies every queued command. Retired tempo maps go to `retire` so
    /// they can be freed off the audio thread.
    pub fn apply_commands(&mut self, mut retire: impl FnMut(Box<TempoMap>)) {
        while let Ok(command) = self.commands.pop() {
            match command {
                TransportCommand::Play => self.playing = true,
                TransportCommand::Stop => self.playing = false,
                TransportCommand::Locate(sample) => self.position = sample,
                TransportCommand::SetLoop(range) => {
                    self.loop_range = range.filter(|r| !r.is_empty());
                }
                TransportCommand::SetTempoMap(map) => {
                    retire(std::mem::replace(&mut self.tempo, map));
                }
            }
        }
    }

    /// The next segment of a block with `remaining` frames left, and
    /// advances the position past it. While stopped the whole remainder is
    /// one segment and the position stays put.
    pub fn next_segment(&mut self, remaining: usize) -> Segment {
        let segment = Segment {
            frames: self.frames_before_wrap(remaining),
            position: self.position,
            playing: self.playing,
        };
        if self.playing {
            self.position += segment.frames as u64;
            if let Some(range) = self.loop_range {
                if self.position == range.end {
                    self.position = range.start;
                }
            }
        }
        segment
    }

    fn frames_before_wrap(&self, remaining: usize) -> usize {
        match self.loop_range {
            Some(range) if self.playing && range.contains(self.position) => {
                let to_end = range.end - self.position;
                remaining.min(to_end.min(usize::MAX as u64) as usize)
            }
            _ => remaining,
        }
    }

    /// Publishes the state for the UI. Call once per block.
    pub fn publish(&self) {
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        self.shared.position.store(self.position, Ordering::Relaxed);
        let (enabled, start, end) = match self.loop_range {
            Some(r) => (true, r.start, r.end),
            None => (false, 0, 0),
        };
        self.shared.loop_start.store(start, Ordering::Relaxed);
        self.shared.loop_end.store(end, Ordering::Relaxed);
        self.shared.loop_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn tempo_map(&self) -> &TempoMap {
        &self.tempo
    }

    pub fn loop_range(&self) -> Option<LoopRange> {
        self.loop_range
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn position(&self) -> u64 {
        self.position
    }
}

/// A read-only view of the transport for threads that only watch it,
/// such as a meter pump. Clone it from [`TransportHandle::clock`].
#[derive(Clone)]
pub struct TransportClock {
    shared: Arc<Shared>,
}

impl TransportClock {
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Timeline sample at the end of the last block.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }
}

impl TransportHandle {
    /// Sends a command. Returns false when the queue is full, which means
    /// the audio thread has not run for a while.
    fn send(&mut self, command: TransportCommand) -> bool {
        self.commands.push(command).is_ok()
    }

    /// A view of the playing flag and position that other threads can hold.
    pub fn clock(&self) -> TransportClock {
        TransportClock {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn play(&mut self) -> bool {
        self.send(TransportCommand::Play)
    }

    pub fn stop(&mut self) -> bool {
        self.send(TransportCommand::Stop)
    }

    pub fn locate(&mut self, sample: u64) -> bool {
        self.send(TransportCommand::Locate(sample))
    }

    /// Sets or clears the loop. An empty range clears it.
    pub fn set_loop(&mut self, range: Option<LoopRange>) -> bool {
        self.send(TransportCommand::SetLoop(range))
    }

    /// Replaces the tempo map on both threads.
    pub fn set_tempo_map(&mut self, map: TempoMap) -> bool {
        let sent = self.send(TransportCommand::SetTempoMap(Box::new(map.clone())));
        if sent {
            self.tempo = map;
        }
        sent
    }

    /// The tempo map as last set from this side.
    pub fn tempo_map(&self) -> &TempoMap {
        &self.tempo
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Timeline sample at the end of the last block.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }

    /// The position as bars, beats and ticks at `sample_rate`.
    pub fn musical_position(&self, sample_rate: f64) -> MusicalPosition {
        self.tempo.samples_to_position(self.position(), sample_rate)
    }

    pub fn loop_range(&self) -> Option<LoopRange> {
        if !self.shared.loop_enabled.load(Ordering::Relaxed) {
            return None;
        }
        Some(LoopRange {
            start: self.shared.loop_start.load(Ordering::Relaxed),
            end: self.shared.loop_end.load(Ordering::Relaxed),
        })
    }

    /// True when the audio thread has gone away.
    pub fn is_abandoned(&self) -> bool {
        self.commands.is_abandoned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(transport: &mut Transport, frames: usize) -> Vec<Segment> {
        let mut out = Vec::new();
        let mut done = 0;
        while done < frames {
            let segment = transport.next_segment(frames - done);
            out.push(segment);
            done += segment.frames;
        }
        transport.publish();
        out
    }

    #[test]
    fn stopped_transport_holds_position() {
        let (mut transport, handle) = Transport::new(TempoMap::default());
        let segs = segments(&mut transport, 64);
        assert_eq!(
            segs,
            vec![Segment {
                frames: 64,
                position: 0,
                playing: false
            }]
        );
        assert!(!handle.is_playing());
        assert_eq!(handle.position(), 0);
    }

    #[test]
    fn play_stop_locate() {
        let (mut transport, mut handle) = Transport::new(TempoMap::default());
        assert!(handle.play());
        transport.apply_commands(|_| {});
        segments(&mut transport, 64);
        assert_eq!(handle.position(), 64);
        assert!(handle.is_playing());
        handle.locate(1000);
        transport.apply_commands(|_| {});
        assert_eq!(
            segments(&mut transport, 10)[0],
            Segment {
                frames: 10,
                position: 1000,
                playing: true
            }
        );
        assert_eq!(handle.position(), 1010);
        handle.stop();
        transport.apply_commands(|_| {});
        segments(&mut transport, 64);
        assert_eq!(handle.position(), 1010);
        assert!(!handle.is_playing());
        assert_eq!(
            handle.musical_position(48_000.0),
            MusicalPosition::new(0, 0, 40)
        );
    }

    #[test]
    fn loop_splits_blocks_at_the_wrap() {
        let (mut transport, mut handle) = Transport::new(TempoMap::default());
        handle.set_loop(Some(LoopRange {
            start: 100,
            end: 150,
        }));
        handle.locate(120);
        handle.play();
        transport.apply_commands(|_| {});
        let segs = segments(&mut transport, 64);
        assert_eq!(
            segs,
            vec![
                Segment {
                    frames: 30,
                    position: 120,
                    playing: true
                },
                Segment {
                    frames: 34,
                    position: 100,
                    playing: true
                },
            ]
        );
        assert_eq!(handle.position(), 134);
        assert_eq!(
            handle.loop_range(),
            Some(LoopRange {
                start: 100,
                end: 150
            })
        );
        // Exactly one segment of the loop's length wraps to the start.
        handle.locate(100);
        transport.apply_commands(|_| {});
        let segs = segments(&mut transport, 50);
        assert_eq!(segs.len(), 1);
        assert_eq!(handle.position(), 100);
        // Outside the loop the transport runs free.
        handle.locate(500);
        transport.apply_commands(|_| {});
        assert_eq!(segments(&mut transport, 64).len(), 1);
        assert_eq!(handle.position(), 564);
        // Empty ranges clear the loop.
        handle.set_loop(Some(LoopRange { start: 5, end: 5 }));
        transport.apply_commands(|_| {});
        transport.publish();
        assert_eq!(handle.loop_range(), None);
    }

    #[test]
    fn tempo_map_swap_retires_the_old_one() {
        let (mut transport, mut handle) = Transport::new(TempoMap::default());
        let mut map = TempoMap::default();
        map.set_tempo(0, 90.0);
        assert!(handle.set_tempo_map(map.clone()));
        assert_eq!(handle.tempo_map(), &map);
        let mut retired = Vec::new();
        transport.apply_commands(|old| retired.push(old));
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].tempo_at(0), 120.0);
        assert_eq!(transport.tempo_map().tempo_at(0), 90.0);
    }

    #[test]
    fn queue_full_and_abandoned_are_reported() {
        let (transport, mut handle) = Transport::new(TempoMap::default());
        for _ in 0..COMMAND_QUEUE {
            assert!(handle.play());
        }
        assert!(!handle.play());
        assert!(!handle.is_abandoned());
        drop(transport);
        assert!(handle.is_abandoned());
    }
}
