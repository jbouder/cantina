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
//!
//! User-designed pedals are the [`pedal`] module: a JSON graph of the
//! primitives above, compiled into a [`PedalProcessor`] that is
//! real-time safe by construction.

mod allpass;
mod biquad;
mod cabinet;
mod compressor;
pub mod convolution;
mod delay_line;
mod echo;
mod envelope;
pub mod fft;
mod gain;
mod gate;
pub mod kit;
pub mod lfo;
pub mod oversample;
pub mod pedal;
pub mod pitch;
mod processor;
mod readout;
mod render;
mod reverb;
mod sampler;
mod shaper;
mod smooth;
mod tuner;

pub use allpass::Allpass;
pub use biquad::Biquad;
pub use cabinet::Cabinet;
pub use compressor::Compressor;
pub use convolution::Convolver;
pub use delay_line::DelayLine;
pub use echo::Echo;
pub use envelope::Envelope;
pub use gain::Gain;
pub use gate::NoiseGate;
pub use kit::{Kit, KitError, LoadedKit, Sample, KIT_VERSION};
pub use lfo::{Lfo, Shape};
pub use oversample::Oversampler;
pub use pedal::{compile, PedalDef, PedalError, PedalProcessor, Problem, PEDAL_VERSION};
pub use processor::{
    Buffer, Chain, Handle, ParamId, ParamInfo, Processor, StateError, Transport, Unit, MAX_CHANNELS,
};
pub use readout::Readout;
pub use render::Renderer;
pub use reverb::Reverb;
pub use sampler::{Sampler, Trigger, MAX_TRIGGERS, MAX_VOICES};
pub use shaper::Curve;
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
    "delay",
    "reverb",
    "amp",
    "cab",
    "gain",
];

/// The stock pedals that are compositions of the pedal format's
/// primitives, as the definitions that ship in `pedals/`: name and
/// JSON. The rest of [`STOCK_NAMES`] stay native because the format
/// cannot express them (`docs/pedals.md` says which and why).
pub static STOCK_PEDAL_JSON: &[(&str, &str)] = &[
    (
        "overdrive",
        include_str!("../../../pedals/overdrive.pedal.json"),
    ),
    (
        "distortion",
        include_str!("../../../pedals/distortion.pedal.json"),
    ),
    ("fuzz", include_str!("../../../pedals/fuzz.pedal.json")),
    ("eq", include_str!("../../../pedals/eq.pedal.json")),
    ("chorus", include_str!("../../../pedals/chorus.pedal.json")),
    (
        "flanger",
        include_str!("../../../pedals/flanger.pedal.json"),
    ),
    ("phaser", include_str!("../../../pedals/phaser.pedal.json")),
    (
        "tremolo",
        include_str!("../../../pedals/tremolo.pedal.json"),
    ),
    ("amp", include_str!("../../../pedals/amp.pedal.json")),
];

/// The definition of a stock pedal that ships in the pedal format,
/// parsed once: what the designer opens as a starting point. `None`
/// for the native ones.
pub fn stock_definition(name: &str) -> Option<&'static PedalDef> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static DEFS: OnceLock<HashMap<&'static str, PedalDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        STOCK_PEDAL_JSON
            .iter()
            .map(|(name, text)| {
                let def = PedalDef::from_json(text)
                    .unwrap_or_else(|e| panic!("stock pedal {name} does not load: {e}"));
                (*name, def)
            })
            .collect()
    })
    .get(name.to_ascii_lowercase().as_str())
}

/// Builds a stock processor by name, unprepared and at default settings.
pub fn stock(name: &str) -> Option<Box<dyn Processor>> {
    if let Some(def) = stock_definition(name) {
        let pedal =
            compile(def).unwrap_or_else(|e| panic!("stock pedal {name} does not compile: {e}"));
        return Some(Box::new(pedal));
    }
    match name.to_ascii_lowercase().as_str() {
        "tuner" => Some(Box::new(Tuner::default())),
        "gate" => Some(Box::new(NoiseGate::default())),
        "compressor" => Some(Box::new(Compressor::default())),
        "delay" => Some(Box::new(Echo::default())),
        "reverb" => Some(Box::new(Reverb::default())),
        "cab" => Some(Box::new(Cabinet::default())),
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
        assert_eq!(stock("Overdrive").unwrap().name(), "Overdrive");
        assert_eq!(stock_definition("EQ").unwrap().name, "EQ");
        assert_eq!(STOCK_PEDAL_JSON.len(), 9);
    }
}
