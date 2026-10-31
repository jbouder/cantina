//! Builds processors from the short specs the examples and, later, the
//! project file use.

use anyhow::{Context, Result};
use cantina_dsp::{stock, Processor, STOCK_NAMES};
use cantina_hosting::au::{self, AudioUnitProcessor};

/// Builds a processor from a spec: a stock name such as `overdrive`, or
/// `au:<identifier or name>` for an Audio Unit, configured for `channels`.
pub fn create(spec: &str, channels: usize) -> Result<Box<dyn Processor>> {
    if let Some(query) = spec.strip_prefix("au:") {
        let info = au::find(query)?;
        let processor = AudioUnitProcessor::new(info, channels)?;
        return Ok(Box::new(processor));
    }
    stock(spec).with_context(|| {
        format!(
            "no processor {spec:?}; stock processors are {} and Audio Units are au:<name>",
            STOCK_NAMES.join(", ")
        )
    })
}
