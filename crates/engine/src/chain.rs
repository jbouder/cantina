//! Runs a [`Chain`] of processors inside a stream, with parameter changes
//! arriving over a lock-free queue.

use cantina_dsp::{Buffer, Chain, ParamId};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::stream::BlockProcessor;

/// A parameter change addressed to one processor in the chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamChange {
    pub processor: usize,
    pub param: ParamId,
    pub value: f32,
}

/// How many parameter changes can wait between two audio callbacks. UI
/// gestures produce far fewer; a full queue drops the newest change.
const QUEUE_CAPACITY: usize = 1024;

/// Adapts a mono [`Chain`] to the stream's [`BlockProcessor`].
pub struct ChainProcessor {
    chain: Chain,
    changes: Consumer<ParamChange>,
}

impl ChainProcessor {
    /// Prepares the chain for the stream and returns the processor together
    /// with the producer the UI thread uses to change parameters.
    pub fn new(
        mut chain: Chain,
        sample_rate: f64,
        max_block_frames: usize,
    ) -> (Self, Producer<ParamChange>) {
        chain.prepare(sample_rate, max_block_frames);
        let (producer, changes) = RingBuffer::new(QUEUE_CAPACITY);
        (Self { chain, changes }, producer)
    }

    pub fn chain(&self) -> &Chain {
        &self.chain
    }
}

impl BlockProcessor for ChainProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        while let Ok(change) = self.changes.pop() {
            if let Some(processor) = self.chain.get_mut(change.processor) {
                processor.set_param(change.param, change.value);
            }
        }
        output.copy_from_slice(input);
        let mut channels = [output];
        let mut buffer = Buffer::new(&mut channels);
        self.chain.process(&mut buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cantina_dsp::stock;

    #[test]
    fn applies_queued_changes_before_processing() {
        let mut chain = Chain::new();
        chain.push(stock("gain").unwrap());
        let (mut processor, mut changes) = ChainProcessor::new(chain, 48_000.0, 64);
        changes
            .push(ParamChange {
                processor: 0,
                param: 0,
                value: -20.0,
            })
            .unwrap();
        // Unknown processor index is ignored, not a crash.
        changes
            .push(ParamChange {
                processor: 7,
                param: 0,
                value: 0.0,
            })
            .unwrap();
        let input = vec![1.0f32; 4800];
        let mut output = vec![0.0f32; 4800];
        processor.process(&input, &mut output);
        assert!((output[4799] - 0.1).abs() < 1e-3, "{}", output[4799]);
        assert_eq!(processor.chain().get(0).unwrap().param(0), -20.0);
    }
}
