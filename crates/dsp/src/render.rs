//! Offline rendering: runs processing over whole signals in blocks, as
//! fast as the machine allows, with no device and no clock.
//!
//! This is what the golden-file tests use, and what mixdown export will
//! build on: the same block loop a stream would drive, fed from memory.

use crate::processor::{Buffer, Chain, Processor, MAX_CHANNELS};

/// Block size and sample rate for a render. Blocks are the size a stream
/// would deliver, so block-rate parameter updates behave as they do live.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Renderer {
    pub sample_rate: f64,
    pub block_frames: usize,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_frames: 64,
        }
    }
}

impl Renderer {
    pub fn new(sample_rate: f64, block_frames: usize) -> Self {
        Self {
            sample_rate,
            block_frames: block_frames.max(1),
        }
    }

    /// Runs `process` over `channels` in place, one block at a time. Every
    /// channel must be the same length; the last block may be short. At
    /// most [`MAX_CHANNELS`] channels are processed.
    pub fn run(&self, channels: &mut [Vec<f32>], mut process: impl FnMut(&mut Buffer)) {
        let frames = channels.first().map_or(0, |c| c.len());
        assert!(
            channels.iter().all(|c| c.len() == frames),
            "every channel must be the same length"
        );
        let count = channels.len().min(MAX_CHANNELS);
        let mut start = 0;
        while start < frames {
            let end = (start + self.block_frames).min(frames);
            let mut lanes: [&mut [f32]; MAX_CHANNELS] = std::array::from_fn(|_| &mut [][..]);
            for (lane, channel) in lanes.iter_mut().zip(channels.iter_mut()) {
                *lane = &mut channel[start..end];
            }
            let mut buffer = Buffer::new(&mut lanes[..count]);
            process(&mut buffer);
            start = end;
        }
    }

    /// Prepares `processor` for this renderer and runs it over `channels`
    /// in place.
    pub fn processor(&self, processor: &mut dyn Processor, channels: &mut [Vec<f32>]) {
        processor.prepare(self.sample_rate, self.block_frames);
        self.run(channels, |buffer| processor.process(buffer));
    }

    /// Prepares `chain` for this renderer and runs it over `channels` in
    /// place.
    pub fn chain(&self, chain: &mut Chain, channels: &mut [Vec<f32>]) {
        chain.prepare(self.sample_rate, self.block_frames);
        self.run(channels, |buffer| chain.process(buffer));
    }

    /// As [`Renderer::processor`], returning a processed copy.
    pub fn render(&self, processor: &mut dyn Processor, input: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let mut channels = input.to_vec();
        self.processor(processor, &mut channels);
        channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{stock, Gain};

    #[test]
    fn blocks_cover_the_signal_including_a_short_tail() {
        let renderer = Renderer::new(48_000.0, 7);
        let mut channels = vec![vec![1.0f32; 100], vec![0.5f32; 100]];
        let mut blocks = Vec::new();
        renderer.run(&mut channels, |buffer| {
            blocks.push(buffer.frames());
            for lane in buffer.channels_mut() {
                for s in lane.iter_mut() {
                    *s *= 2.0;
                }
            }
        });
        assert_eq!(blocks.iter().sum::<usize>(), 100);
        assert_eq!(blocks.last(), Some(&2));
        assert!(blocks[..blocks.len() - 1].iter().all(|b| *b == 7));
        assert!(channels[0].iter().all(|s| *s == 2.0));
        assert!(channels[1].iter().all(|s| *s == 1.0));
    }

    #[test]
    fn render_matches_block_size_independent_processing_for_memoryless_dsp() {
        let input = vec![(0..1000)
            .map(|i| (i as f32 * 0.01).sin())
            .collect::<Vec<f32>>()];
        let a = Renderer::new(48_000.0, 64).render(&mut Gain::new(-6.0), &input);
        let b = Renderer::new(48_000.0, 13).render(&mut Gain::new(-6.0), &input);
        assert_eq!(a, b);
        assert_ne!(a, input);
    }

    #[test]
    fn chain_and_processor_agree() {
        let input = vec![(0..500)
            .map(|i| ((i % 50) as f32 / 25.0) - 1.0)
            .collect::<Vec<f32>>()];
        let renderer = Renderer::default();
        let direct = renderer.render(stock("overdrive").unwrap().as_mut(), &input);
        let mut chain = Chain::new();
        chain.push(stock("overdrive").unwrap());
        let mut via_chain = input.clone();
        renderer.chain(&mut chain, &mut via_chain);
        assert_eq!(direct, via_chain);
    }
}
