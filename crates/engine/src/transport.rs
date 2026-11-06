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
//! processed as two blocks with their own positions. A count-in ends the
//! same way, so the timeline starts rolling on an exact frame of a block.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::tempo::{MusicalPosition, TempoMap};

/// Commands that can wait between two blocks.
const COMMAND_QUEUE: usize = 64;

/// Beats counted before a recording starts.
pub const COUNT_IN_BEATS: u64 = 4;

/// Beats counted off while the timeline holds, before it rolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountIn {
    /// Frames left, from the first frame of the block that carries this.
    pub remaining: u64,
    /// Frames in one beat.
    pub beat: u64,
    /// Beats counted in all.
    pub beats: u64,
}

impl CountIn {
    /// A count of `beats` beats of `beat` frames each.
    pub fn new(beat: u64, beats: u64) -> Self {
        let beat = beat.max(1);
        Self {
            remaining: beat * beats,
            beat,
            beats,
        }
    }

    /// Which beat is being counted, from 1.
    pub fn count(&self) -> u64 {
        (self.beat * self.beats).saturating_sub(self.remaining) / self.beat + 1
    }

    /// Whether a beat falls `frame` frames into the block, and if so
    /// whether it is the first of the count.
    pub fn beat_at(&self, frame: u64) -> Option<bool> {
        let left = self.remaining.checked_sub(frame).filter(|l| *l > 0)?;
        (left % self.beat == 0).then_some(left == self.beat * self.beats)
    }
}

/// A loop between two timeline samples, `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopRange {
    /// First sample in the loop.
    pub start: u64,
    /// Sample just after the loop; playback wraps from here to `start`.
    pub end: u64,
}

impl LoopRange {
    /// Samples in one pass.
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    /// True for a loop with no length, which never wraps.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// True when `sample` is inside the loop.
    pub fn contains(&self, sample: u64) -> bool {
        (self.start..self.end).contains(&sample)
    }
}

/// What the UI asks of the transport.
pub enum TransportCommand {
    /// Start playing from the current position.
    Play,
    /// Stop where it is, ending any count-in.
    Stop,
    /// Count beats with the timeline held, then play.
    CountIn(CountIn),
    /// Move to a timeline sample.
    Locate(u64),
    /// Set or clear the loop.
    SetLoop(Option<LoopRange>),
    /// Replace the tempo map.
    SetTempoMap(Box<TempoMap>),
}

/// Transport state published by the audio thread.
#[derive(Debug, Default)]
struct Shared {
    playing: AtomicBool,
    position: AtomicU64,
    /// The beat being counted, from 1; zero with no count-in.
    count: AtomicU64,
    loop_enabled: AtomicBool,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
}

/// A contiguous run of frames within one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Frames in the segment.
    pub frames: usize,
    /// Timeline sample of the segment's first frame.
    pub position: u64,
    /// True when the timeline moves during it.
    pub playing: bool,
    /// The count-in, when the segment is part of one.
    pub count_in: Option<CountIn>,
}

/// The audio-thread half.
pub struct Transport {
    shared: Arc<Shared>,
    commands: Consumer<TransportCommand>,
    tempo: Box<TempoMap>,
    playing: bool,
    position: u64,
    loop_range: Option<LoopRange>,
    count_in: Option<CountIn>,
}

/// The UI-thread half.
pub struct TransportHandle {
    shared: Arc<Shared>,
    commands: Producer<TransportCommand>,
    tempo: TempoMap,
}

impl Transport {
    /// A stopped transport at zero and its handle for the UI thread.
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
            count_in: None,
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
                TransportCommand::Play => {
                    self.count_in = None;
                    self.playing = true;
                }
                TransportCommand::Stop => {
                    self.count_in = None;
                    self.playing = false;
                }
                TransportCommand::CountIn(count) => {
                    self.playing = false;
                    self.count_in = Some(count).filter(|c| c.remaining > 0);
                }
                // A take is waiting for this position; it holds until then.
                TransportCommand::Locate(_) if self.count_in.is_some() => {}
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
        if let Some(count) = self.count_in {
            let frames = remaining.min(count.remaining.min(usize::MAX as u64) as usize);
            let left = count.remaining - frames as u64;
            self.count_in = (left > 0).then_some(CountIn {
                remaining: left,
                ..count
            });
            self.playing = left == 0;
            return Segment {
                frames,
                position: self.position,
                playing: false,
                count_in: Some(count),
            };
        }
        let segment = Segment {
            frames: self.frames_before_wrap(remaining),
            position: self.position,
            playing: self.playing,
            count_in: None,
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
        let count = self.count_in.map_or(0, |c| c.count());
        self.shared.count.store(count, Ordering::Relaxed);
        let (enabled, start, end) = match self.loop_range {
            Some(r) => (true, r.start, r.end),
            None => (false, 0, 0),
        };
        self.shared.loop_start.store(start, Ordering::Relaxed);
        self.shared.loop_end.store(end, Ordering::Relaxed);
        self.shared.loop_enabled.store(enabled, Ordering::Relaxed);
    }

    /// The tempo map in use.
    pub fn tempo_map(&self) -> &TempoMap {
        &self.tempo
    }

    /// The active loop, if any.
    pub fn loop_range(&self) -> Option<LoopRange> {
        self.loop_range
    }

    /// True while playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Timeline sample of the next frame.
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
    /// True while playing.
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Timeline sample at the end of the last block.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }

    /// The beat being counted, from 1; zero with no count-in.
    pub fn count_in(&self) -> u64 {
        self.shared.count.load(Ordering::Relaxed)
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

    /// Asks the transport to play. False when the queue is full.
    pub fn play(&mut self) -> bool {
        self.send(TransportCommand::Play)
    }

    /// Asks the transport to count [`COUNT_IN_BEATS`] beats at the tempo
    /// where it stands, then play. False when the queue is full.
    pub fn count_in(&mut self, sample_rate: f64) -> bool {
        let tick = self
            .tempo
            .samples_to_ticks(self.position(), sample_rate)
            .max(0.0) as u64;
        let beat = (60.0 / self.tempo.tempo_at(tick) * sample_rate).round() as u64;
        self.send(TransportCommand::CountIn(CountIn::new(
            beat,
            COUNT_IN_BEATS,
        )))
    }

    /// Asks the transport to stop. False when the queue is full.
    pub fn stop(&mut self) -> bool {
        self.send(TransportCommand::Stop)
    }

    /// Asks the transport to move to `sample`. False when the queue is full.
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

    /// True while playing.
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

    /// The loop, if one is set and enabled.
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
                playing: false,
                count_in: None,
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
                playing: true,
                count_in: None,
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
                    playing: true,
                    count_in: None,
                },
                Segment {
                    frames: 34,
                    position: 100,
                    playing: true,
                    count_in: None,
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

    #[test]
    fn a_count_in_holds_the_timeline_then_rolls_on_an_exact_frame() {
        let (mut transport, mut handle) = Transport::new(TempoMap::default());
        handle.locate(1000);
        transport.apply_commands(|_| {});
        transport.publish();
        // 120 bpm at 48 kHz: 24 000 frames a beat, 96 000 in the count.
        assert!(handle.count_in(48_000.0));
        transport.apply_commands(|_| {});
        let mut frames = 0;
        while frames + 4096 < 96_000 {
            let segs = segments(&mut transport, 4096);
            assert_eq!(segs.len(), 1);
            assert!(!segs[0].playing);
            assert_eq!(segs[0].position, 1000);
            assert_eq!(handle.clock().count_in(), (frames + 4096) / 24_000 + 1);
            frames += 4096;
        }
        // The block that ends the count is cut where the timeline starts.
        let before = (96_000 - frames) as usize;
        let segs = segments(&mut transport, 4096);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].frames, before);
        assert!(!segs[0].playing);
        assert!(segs[0].count_in.is_some());
        assert_eq!(
            segs[1],
            Segment {
                frames: 4096 - before,
                position: 1000,
                playing: true,
                count_in: None,
            }
        );
        assert_eq!(handle.clock().count_in(), 0);
        assert!(handle.is_playing());
    }

    #[test]
    fn stop_ends_a_count_in_and_locate_waits_for_it() {
        let (mut transport, mut handle) = Transport::new(TempoMap::default());
        handle.count_in(48_000.0);
        handle.locate(500);
        transport.apply_commands(|_| {});
        let segs = segments(&mut transport, 64);
        assert_eq!(segs[0].position, 0, "the take is waiting for this position");
        assert_eq!(handle.clock().count_in(), 1);
        handle.stop();
        transport.apply_commands(|_| {});
        let segs = segments(&mut transport, 96_000);
        assert_eq!(segs[0].count_in, None);
        assert!(!segs[0].playing);
        assert!(!handle.is_playing());
        assert_eq!(handle.clock().count_in(), 0);
    }

    #[test]
    fn a_count_in_finds_its_beats() {
        let count = CountIn::new(100, 4);
        assert_eq!(count.count(), 1);
        assert_eq!(count.beat_at(0), Some(true), "the first beat is marked");
        assert_eq!(count.beat_at(1), None);
        assert_eq!(count.beat_at(100), Some(false));
        assert_eq!(count.beat_at(300), Some(false));
        assert_eq!(count.beat_at(400), None, "the count is over there");
        let later = CountIn {
            remaining: 150,
            ..count
        };
        assert_eq!(later.count(), 3);
        assert_eq!(later.beat_at(50), Some(false));
    }
}
