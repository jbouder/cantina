//! Latency compensation: a plain delay node and the arithmetic that
//! decides how much each track gets.
//!
//! Every processor reports the latency it adds. A track's chain adds the
//! sum of its pedals' latencies, so without correction a track with a
//! lookahead limiter plays late against one without. [`plan`] takes the
//! chain latency of every track and gives each one the delay that brings
//! it level with the slowest: the slowest gets none, the others wait for
//! it. The shell puts a [`Delay`] on each track's *playback* branch,
//! between the player and the bus, so clips line up at the master. Live
//! input joins the bus after the delay and is never compensated: the
//! player hears their own chain's latency and nothing more.
//!
//! The delay is an integer number of frames and changes only when a
//! chain changes, which already interrupts the audio, so it is not
//! smoothed.

use cantina_dsp::{Buffer, ParamId, ParamInfo, Unit, MAX_CHANNELS};

use crate::graph::{BlockContext, Node};

/// The delay every track needs, in frames, to line up with the slowest:
/// the largest chain latency, and for each track the difference to it.
pub fn plan(chain_latencies: &[usize]) -> (usize, Vec<usize>) {
    let max = chain_latencies.iter().copied().max().unwrap_or(0);
    (max, chain_latencies.iter().map(|l| max - l).collect())
}

/// Delays its input by a whole number of frames, up to a capacity fixed
/// at construction. Parameter 0 is `Delay`, in frames.
pub struct Delay {
    capacity: usize,
    delay: usize,
    params: [ParamInfo; 1],
    /// One ring per channel, `capacity + 1` long so a full delay still
    /// leaves the write position ahead of the read position.
    rings: Vec<Vec<f32>>,
    write: usize,
}

impl Delay {
    /// A delay of up to `capacity` frames, starting at zero.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            delay: 0,
            params: [ParamInfo {
                name: "Delay".into(),
                unit: Unit::Generic,
                min: 0.0,
                max: capacity as f32,
                default: 0.0,
            }],
            rings: Vec::new(),
            write: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The current delay in frames.
    pub fn delay(&self) -> usize {
        self.delay
    }
}

impl Node for Delay {
    fn name(&self) -> &str {
        "Delay"
    }

    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn param(&self, _id: ParamId) -> f32 {
        self.delay as f32
    }

    fn set_param(&mut self, id: ParamId, value: f32) {
        if id == 0 {
            self.delay = self.params[0].clamp(value.round()) as usize;
        }
    }

    fn latency(&self) -> usize {
        // The delay is the compensation itself, not latency to compensate.
        0
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block_frames: usize) {
        self.rings = vec![vec![0.0; self.capacity + 1]; MAX_CHANNELS];
        self.write = 0;
    }

    fn reset(&mut self) {
        for ring in &mut self.rings {
            ring.fill(0.0);
        }
        self.write = 0;
    }

    fn process(&mut self, _ctx: &BlockContext, buffer: &mut Buffer) {
        if self.delay == 0 || self.rings.is_empty() {
            return;
        }
        let len = self.capacity + 1;
        let frames = buffer.frames();
        let channels = buffer.channels().min(self.rings.len());
        for ch in 0..channels {
            let ring = &mut self.rings[ch];
            let mut write = self.write;
            for sample in buffer.channel_mut(ch)[..frames].iter_mut() {
                ring[write] = *sample;
                let read = (write + len - self.delay) % len;
                *sample = ring[read];
                write = (write + 1) % len;
            }
        }
        self.write = (self.write + frames) % len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Endpoint, GraphConfig};
    use crate::player::{Clip, Player, PlayerHandle};
    use crate::runtime::Runtime;
    use crate::stream::BlockProcessor;
    use crate::tempo::TempoMap;
    use crate::wav;
    use cantina_dsp::Processor;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    const SR: f64 = 48_000.0;
    const BLOCK: usize = 64;

    #[test]
    fn plan_levels_every_track_with_the_slowest() {
        assert_eq!(plan(&[]), (0, vec![]));
        assert_eq!(plan(&[0, 0]), (0, vec![0, 0]));
        assert_eq!(plan(&[10, 0, 4]), (10, vec![0, 10, 6]));
    }

    #[test]
    fn delays_by_whole_frames_across_blocks_and_clamps_to_capacity() {
        let mut delay = Delay::new(100);
        delay.prepare(SR, BLOCK);
        delay.set_param(0, 37.4);
        assert_eq!(delay.delay(), 37);
        let tempo = TempoMap::default();
        let ctx = BlockContext {
            sample_rate: SR,
            frames: BLOCK,
            playing: true,
            position: 0,
            loop_range: None,
            tempo: &tempo,
        };
        let mut signal: Vec<f32> = (0..256).map(|i| i as f32).collect();
        for block in signal.chunks_mut(BLOCK) {
            let mut channels = [block];
            delay.process(&ctx, &mut Buffer::new(&mut channels));
        }
        assert!(signal[..37].iter().all(|s| *s == 0.0));
        for (i, s) in signal.iter().enumerate().skip(37) {
            assert_eq!(*s, (i - 37) as f32, "frame {i}");
        }
        delay.set_param(0, 1000.0);
        assert_eq!(delay.delay(), 100, "clamped to the capacity");
        delay.set_param(0, 0.0);
        let mut pass = vec![1.0f32; 8];
        let mut channels = [&mut pass[..]];
        delay.process(&ctx, &mut Buffer::new(&mut channels));
        assert_eq!(pass, vec![1.0; 8], "zero is a pass-through");
    }

    /// A processor that reports latency and delays by it, like a
    /// lookahead plugin.
    struct Latent(Delay);

    impl Latent {
        fn new(frames: usize) -> Self {
            let mut delay = Delay::new(frames);
            delay.set_param(0, frames as f32);
            Self(delay)
        }
    }

    impl Processor for Latent {
        fn name(&self) -> &str {
            "latent"
        }
        fn params(&self) -> &[ParamInfo] {
            &[]
        }
        fn param(&self, _: ParamId) -> f32 {
            0.0
        }
        fn set_param(&mut self, _: ParamId, _: f32) {}
        fn latency(&self) -> usize {
            self.0.delay()
        }
        fn prepare(&mut self, sample_rate: f64, max_block_frames: usize) {
            self.0.prepare(sample_rate, max_block_frames);
        }
        fn process(&mut self, buffer: &mut Buffer) {
            let tempo = TempoMap::default();
            let ctx = BlockContext {
                sample_rate: SR,
                frames: buffer.frames(),
                playing: true,
                position: 0,
                loop_range: None,
                tempo: &tempo,
            };
            self.0.process(&ctx, buffer);
        }
    }

    fn wait_for_prefetch(runtime: &mut Runtime, handles: &[&PlayerHandle]) {
        let start = Instant::now();
        loop {
            runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
            if handles.iter().all(|h| {
                h.stats().is_caught_up() && h.stats().ready_frames.load(Ordering::Relaxed) >= 4096
            }) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "disk threads never caught up"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn impulse_position(signal: &[f32]) -> usize {
        signal
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .map(|(i, _)| i)
            .unwrap()
    }

    /// Two tracks, one with a 10-frame lookahead in its chain, each
    /// playing a clip with an impulse at frame 100. With the planned
    /// delays the impulses reach the output on the same frame; without
    /// them the slow track is 10 frames late.
    #[test]
    fn compensated_tracks_play_aligned_to_the_sample() {
        let dir = wav::tests::tempdir();
        let mut impulse = vec![0.0f32; 4096];
        impulse[100] = 1.0;
        let path = dir.join("impulse.wav");
        wav::write(&path, SR, std::slice::from_ref(&impulse)).unwrap();
        let clip = Clip {
            path,
            start: 0,
            offset: 0,
            length: 4096,
        };
        let latency = 10;

        let render = |compensate: bool| -> (Vec<f32>, Vec<f32>) {
            let (mut runtime, mut controller) = Runtime::new(GraphConfig {
                sample_rate: SR,
                max_block_frames: BLOCK,
                channels: 1,
            })
            .unwrap();
            let g = controller.graph_mut();
            // Track A: player -> delay -> latent -> tap A.
            let (player_a, mut handle_a) = Player::new(SR, 1);
            let a = g.add(Box::new(player_a));
            let delay_a = g.add(Box::new(Delay::new(1000)));
            let latent = g.add_processor(Box::new(Latent::new(latency)));
            let tap_a = g.add_processor(cantina_dsp::stock("gain").unwrap());
            // Track B: player -> delay -> tap B.
            let (player_b, mut handle_b) = Player::new(SR, 1);
            let b = g.add(Box::new(player_b));
            let delay_b = g.add(Box::new(Delay::new(1000)));
            let tap_b = g.add_processor(cantina_dsp::stock("gain").unwrap());
            for (from, to) in [
                (a, delay_a),
                (delay_a, latent),
                (latent, tap_a),
                (b, delay_b),
                (delay_b, tap_b),
            ] {
                g.connect(from, to).unwrap();
            }
            // Only track A reaches the output, so the two can be measured
            // separately: B's tap is read through a second pass below.
            g.connect(tap_a, Endpoint::Output).unwrap();
            let (max, delays) = plan(&[g.node(latent).unwrap().latency, 0]);
            assert_eq!((max, delays.clone()), (latency, vec![0, latency]));
            if compensate {
                g.set_param(delay_a, 0, delays[0] as f32);
                g.set_param(delay_b, 0, delays[1] as f32);
            }
            controller.commit().unwrap();
            handle_a.set_clips(vec![clip.clone()]).unwrap();
            handle_b.set_clips(vec![clip.clone()]).unwrap();
            runtime.process(&[0.0; BLOCK], &mut [0.0; BLOCK]);
            wait_for_prefetch(&mut runtime, &[&handle_a, &handle_b]);

            let play = |controller: &mut crate::runtime::Controller,
                        runtime: &mut Runtime,
                        handles: &[&PlayerHandle]| {
                controller.transport_mut().locate(0);
                wait_for_prefetch(runtime, handles);
                controller.transport_mut().play();
                let mut out = vec![0.0f32; 1024];
                for chunk in out.chunks_mut(BLOCK) {
                    runtime.process(&[0.0; BLOCK], chunk);
                }
                controller.transport_mut().stop();
                out
            };
            let out_a = play(&mut controller, &mut runtime, &[&handle_a, &handle_b]);
            // Rewire so track B is heard, and play again from the top.
            controller.graph_mut().disconnect(tap_a, Endpoint::Output);
            controller
                .graph_mut()
                .connect(tap_b, Endpoint::Output)
                .unwrap();
            controller.commit().unwrap();
            let out_b = play(&mut controller, &mut runtime, &[&handle_a, &handle_b]);
            (out_a, out_b)
        };

        let (a, b) = render(false);
        assert_eq!(
            impulse_position(&a),
            100 + latency,
            "A is late by its chain"
        );
        assert_eq!(impulse_position(&b), 100);

        let (a, b) = render(true);
        assert_eq!(impulse_position(&a), 100 + latency);
        assert_eq!(
            impulse_position(&b),
            100 + latency,
            "B waits for A: both land on the same frame"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
