//! Offline rendering of a whole graph: the runtime and nodes live
//! playback uses, run block by block as fast as the machine allows.
//!
//! A live stream never waits for the disk: a [`Player`] that has nothing
//! ready plays silence. Offline there is no deadline, so [`Offline`]
//! waits instead. Before each block it asks every player's disk thread
//! whether it has rendered past the end of that block, and only then
//! runs it. The waiting happens here, between blocks, on the rendering
//! thread; nothing inside a block ever waits, so the nodes run exactly
//! the code they run on the audio thread and the output is what playback
//! would have produced with a disk that kept up.
//!
//! [`Offline::set_paced`] turns the waiting off and runs blocks at the
//! pace a stream would instead, which is how a test holds an offline
//! render against what playback produces.
//!
//! [`Player`]: crate::Player

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use cantina_dsp::MAX_CHANNELS;

use crate::player::PlayerStats;
use crate::runtime::{Controller, Runtime};

/// How long a player's disk thread may make no progress before a render
/// gives up on it.
const STALL: Duration = Duration::from_secs(10);
/// How long to sleep between looks at a disk thread that is behind.
const POLL: Duration = Duration::from_micros(200);
/// Stopped time a render runs before playing, as a stream idles before
/// someone presses play. The same in both modes: a pedal that moves on
/// its own (an LFO) has moved by the same amount when play begins.
const PRIME_SECONDS: f64 = 0.1;

/// A graph rendering offline from a timeline position.
pub struct Offline {
    runtime: Runtime,
    controller: Controller,
    players: Vec<Arc<PlayerStats>>,
    block: usize,
    position: u64,
    started: bool,
    /// When pacing: when the clock started, and frames run since.
    paced: Option<(Instant, u64)>,
    silence: Vec<f32>,
    outputs: Vec<Vec<f32>>,
}

impl Offline {
    /// Renders `runtime`, whose graph `controller` has committed, in
    /// blocks of `block_frames` (clamped to the graph's maximum). Every
    /// player in the graph must be listed in `players`, or it may play
    /// silence where a disk thread was slow.
    pub fn new(
        runtime: Runtime,
        controller: Controller,
        players: Vec<Arc<PlayerStats>>,
        block_frames: usize,
    ) -> Self {
        let config = runtime.config();
        let block = block_frames.clamp(1, config.max_block_frames);
        let channels = config.channels.min(MAX_CHANNELS);
        Self {
            runtime,
            controller,
            players,
            block,
            position: 0,
            started: false,
            paced: None,
            silence: vec![0.0; block],
            outputs: vec![vec![0.0; block]; channels],
        }
    }

    /// The controller, for setting the tempo map or parameters before
    /// the render starts.
    pub fn controller_mut(&mut self) -> &mut Controller {
        &mut self.controller
    }

    /// The players' counters: after a render, underruns say whether
    /// a paced one kept up.
    pub fn players(&self) -> &[Arc<PlayerStats>] {
        &self.players
    }

    /// Frames per block.
    pub fn block_frames(&self) -> usize {
        self.block
    }

    /// Timeline sample the next block starts at.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Runs blocks at the pace a live stream would, never waiting for a
    /// player: what playback produces, underruns and all. Set before
    /// [`Offline::start`].
    pub fn set_paced(&mut self, paced: bool) {
        self.paced = paced.then(|| (Instant::now(), 0));
    }

    /// Puts the transport at `position` and starts it. A tenth of a
    /// second of stopped blocks runs first, as a stream's would before
    /// play, so every player has asked its disk thread for `position`
    /// before anything has to be heard.
    pub fn start(&mut self, position: u64) {
        let transport = self.controller.transport_mut();
        transport.stop();
        transport.locate(position);
        if let Some(clock) = self.paced.as_mut() {
            *clock = (Instant::now(), 0);
        }
        let prime = (PRIME_SECONDS * self.runtime.config().sample_rate / self.block as f64).ceil();
        for _ in 0..(prime as usize).max(1) {
            self.run_block(self.block);
        }
        self.controller.transport_mut().play();
        self.controller.collect();
        self.position = position;
        self.started = true;
    }

    /// Renders the next `frames` (at most one block) and returns them,
    /// one slice per graph channel. Waits first for every player to have
    /// what the block needs; fails if one stops making progress.
    pub fn next(&mut self, frames: usize) -> Result<Vec<&[f32]>> {
        if !self.started {
            bail!("the offline render was not started");
        }
        let frames = frames.clamp(1, self.block);
        let end = self.position + frames as u64;
        if self.paced.is_none() {
            self.wait_for_players(end)?;
        }
        self.run_block(frames);
        self.position = end;
        Ok(self.outputs.iter().map(|o| &o[..frames]).collect())
    }

    /// Renders `frames` from where the render is, into whole channels.
    pub fn render(&mut self, frames: usize) -> Result<Vec<Vec<f32>>> {
        let mut out: Vec<Vec<f32>> = (0..self.outputs.len())
            .map(|_| Vec::with_capacity(frames))
            .collect();
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.block);
            for (channel, block) in out.iter_mut().zip(self.next(n)?) {
                channel.extend_from_slice(block);
            }
            done += n;
        }
        Ok(out)
    }

    fn run_block(&mut self, frames: usize) {
        if let Some((since, run)) = self.paced.as_mut() {
            // The block is due when the frames before it have played.
            let due = Duration::from_secs_f64(*run as f64 / self.runtime.config().sample_rate);
            if let Some(wait) = due.checked_sub(since.elapsed()) {
                thread::sleep(wait);
            }
            *run += frames as u64;
        }
        let ins: Vec<&[f32]> = (0..self.outputs.len())
            .map(|_| &self.silence[..frames])
            .collect();
        let mut outs: Vec<&mut [f32]> = self.outputs.iter_mut().map(|o| &mut o[..frames]).collect();
        self.runtime.run(&ins, &mut outs);
    }

    fn wait_for_players(&self, end: u64) -> Result<()> {
        for stats in &self.players {
            let mut last = stats
                .rendered_through
                .load(std::sync::atomic::Ordering::Acquire);
            let mut since = Instant::now();
            while !stats.is_ready_through(end) {
                let now = stats
                    .rendered_through
                    .load(std::sync::atomic::Ordering::Acquire);
                if now != last {
                    last = now;
                    since = Instant::now();
                } else if since.elapsed() > STALL {
                    bail!("a player's disk thread stopped delivering audio at sample {now}");
                }
                thread::sleep(POLL);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::player::{Clip, Player};
    use crate::wav::tests::{ramp, ramp_file, tempdir};

    const SR: f64 = 48_000.0;

    #[test]
    fn plays_a_long_clip_without_a_single_underrun() {
        let dir = tempdir();
        // Longer than the player prefetches, so the render has to wait
        // for the disk thread more than once.
        let frames = (SR * 5.0) as usize;
        let path = ramp_file(&dir, "long.wav", 1, frames as u32, 32);
        let (runtime, mut controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: 512,
            channels: 1,
        })
        .unwrap();
        let (player, mut handle) = Player::new(SR, 1);
        let node = controller.graph_mut().add(Box::new(player));
        controller
            .graph_mut()
            .connect(node, Endpoint::Output)
            .unwrap();
        controller.commit().unwrap();
        handle
            .set_clips(vec![Clip {
                path,
                start: 100,
                offset: 0,
                length: frames as u64,
            }])
            .unwrap();
        let stats = Arc::clone(handle.stats());
        let mut offline = Offline::new(runtime, controller, vec![Arc::clone(&stats)], 512);
        offline.start(0);
        let out = offline.render(frames + 100).unwrap();
        assert_eq!(
            stats
                .underrun_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(out[0][..100].iter().all(|s| *s == 0.0));
        for (i, s) in out[0][100..].iter().enumerate() {
            assert!(
                (s - ramp(i as u64, 0)).abs() < 1e-6,
                "frame {i}: {s} != {}",
                ramp(i as u64, 0)
            );
        }
    }

    #[test]
    fn next_before_start_is_refused() {
        let (runtime, controller) = Runtime::new(GraphConfig {
            sample_rate: SR,
            max_block_frames: 64,
            channels: 1,
        })
        .unwrap();
        let mut offline = Offline::new(runtime, controller, Vec::new(), 64);
        assert!(offline.next(64).is_err());
    }
}
