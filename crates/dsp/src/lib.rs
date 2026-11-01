//! Pure audio processing for Cantina.
//!
//! This crate knows nothing about devices, files, threads or the UI. It
//! holds the building blocks that run inside the audio callback, so every
//! type here obeys the real-time rules:
//!
//! - `process` never allocates, locks, blocks or performs I/O.
//! - All memory a processor needs is reserved in `prepare`, which runs off
//!   the audio thread.
//! - Parameter changes are smoothed so a step change never clicks.
//!
//! The [`Processor`] trait is the one abstraction every pedal shares: stock
//! pedals here, user-designed pedals later, and third-party plugins in the
//! `hosting` crate. The chain, the mixer, automation, latency compensation
//! and the project file only ever see a `Processor`.
//!
//! Drums are here too: the [`kit`] module defines the kit format and the
//! [`Sampler`] plays one. The sequencer that decides when to hit a pad
//! lives in the engine crate, next to the tempo map.

mod biquad;
mod compressor;
mod delay_line;
mod drive;
mod envelope;
mod eq;
mod gain;
mod gate;
pub mod kit;
pub mod lfo;
mod modulation;
pub mod oversample;
pub mod pitch;
mod processor;
mod readout;
mod render;
mod sampler;
mod shaper;
mod smooth;
mod tuner;

pub use biquad::Biquad;
pub use compressor::Compressor;
pub use delay_line::DelayLine;
pub use drive::{Distortion, Drive, Fuzz, Overdrive, Voicing};
pub use envelope::Envelope;
pub use eq::Equalizer;
pub use gain::Gain;
pub use gate::NoiseGate;
pub use kit::{Kit, KitError, LoadedKit, Sample, KIT_VERSION};
pub use lfo::{Lfo, Shape};
pub use modulation::{Phaser, SweptDelay, Tremolo};
pub use oversample::Oversampler;
pub use processor::{
    Buffer, Chain, Handle, ParamId, ParamInfo, Processor, StateError, Transport, Unit, MAX_CHANNELS,
};
pub use readout::Readout;
pub use render::Renderer;
pub use sampler::{Sampler, Trigger, MAX_TRIGGERS, MAX_VOICES};
pub use smooth::Smoother;
pub use tuner::Tuner;

/// Converts decibels to a linear amplitude multiplier.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Converts a linear amplitude multiplier to decibels, clamping silence to
/// a finite floor instead of returning negative infinity.
#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-9).log10()
}

/// Names of the stock processors [`stock`] can build.
pub const STOCK_NAMES: &[&str] = &[
    "tuner",
    "gate",
    "compressor",
    "overdrive",
    "distortion",
    "fuzz",
    "eq",
    "chorus",
    "flanger",
    "phaser",
    "tremolo",
    "gain",
];

/// Builds a stock processor by name, unprepared and at default settings.
pub fn stock(name: &str) -> Option<Box<dyn Processor>> {
    match name.to_ascii_lowercase().as_str() {
        "tuner" => Some(Box::new(Tuner::default())),
        "gate" => Some(Box::new(NoiseGate::default())),
        "compressor" => Some(Box::new(Compressor::default())),
        "overdrive" => Some(Box::new(Overdrive::default())),
        "distortion" => Some(Box::new(Distortion::default())),
        "fuzz" => Some(Box::new(Fuzz::default())),
        "eq" => Some(Box::new(Equalizer::default())),
        "chorus" => Some(Box::new(SweptDelay::chorus())),
        "flanger" => Some(Box::new(SweptDelay::flanger())),
        "phaser" => Some(Box::new(Phaser::default())),
        "tremolo" => Some(Box::new(Tremolo::default())),
        "gain" => Some(Box::new(Gain::default())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_conversions_round_trip() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 1e-4);
        assert!((linear_to_db(db_to_linear(-12.0)) + 12.0).abs() < 1e-4);
        assert!(linear_to_db(0.0).is_finite());
    }

    #[test]
    fn every_stock_name_builds() {
        for name in STOCK_NAMES {
            let p = stock(name).unwrap_or_else(|| panic!("{name} did not build"));
            assert!(!p.params().is_empty(), "{name} has no parameters");
        }
        assert!(stock("flux capacitor").is_none());
    }
}
