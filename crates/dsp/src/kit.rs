//! The drum kit format: pads mapped to samples, with velocity layers,
//! choke groups and per-pad gain and pan.
//!
//! A kit is a JSON file next to its samples:
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Acoustic",
//!   "pads": [
//!     {
//!       "id": "kick", "name": "Kick",
//!       "layers": [
//!         { "file": "kick-soft.wav", "velocity": [0, 0.6] },
//!         { "file": "kick-hard.wav", "velocity": [0.6, 1] }
//!       ]
//!     },
//!     { "id": "hh-closed", "name": "Closed hat", "choke": "hat", "pan": 0.3,
//!       "layers": [{ "file": "hh-closed.wav" }] },
//!     { "id": "hh-open", "name": "Open hat", "choke": "hat", "pan": 0.3,
//!       "layers": [{ "file": "hh-open.wav" }] }
//!   ]
//! }
//! ```
//!
//! [`Kit`] is the file as data, validated by [`Kit::from_json`]. A
//! [`LoadedKit`] is a kit with its samples in memory, built by whoever can
//! read files (the engine crate) and handed to the [`Sampler`]. This crate
//! does no I/O: it only knows the shape of a kit and the samples it is
//! given.
//!
//! [`Sampler`]: crate::Sampler

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db_to_linear;

/// The format version this crate reads and writes.
pub const KIT_VERSION: u32 = 1;
/// Most pads a kit may have.
pub const MAX_PADS: usize = 64;
/// Most velocity layers one pad may have.
pub const MAX_LAYERS: usize = 16;

/// A drum kit as its JSON file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kit {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub pads: Vec<Pad>,
}

/// One pad: a sound the sequencer can hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pad {
    /// Stable identifier patterns refer to, such as `kick` or `hh-closed`.
    pub id: String,
    /// Display name; the id when missing.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub gain_db: f32,
    /// -1 is hard left, 1 hard right.
    #[serde(default)]
    pub pan: f32,
    /// Pads in the same choke group cut each other: hitting one stops
    /// whatever else in the group is still ringing, the closed hat
    /// silencing the open one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke: Option<String>,
    pub layers: Vec<Layer>,
}

/// One sample of a pad, chosen by velocity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// A file next to the kit's JSON.
    pub file: String,
    /// The velocities this layer answers, inclusive, from 0 to 1. The
    /// whole range when missing.
    #[serde(default = "full_range")]
    pub velocity: [f32; 2],
    #[serde(default)]
    pub gain_db: f32,
}

fn full_range() -> [f32; 2] {
    [0.0, 1.0]
}

/// Why a kit could not be read or built.
#[derive(Debug, Clone, PartialEq)]
pub enum KitError {
    /// The text is not JSON, or not the kit shape.
    Json(String),
    /// The kit is JSON but breaks a rule; the message says which.
    Invalid(String),
    /// A sample could not be provided.
    Sample { file: String, why: String },
}

impl fmt::Display for KitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KitError::Json(why) => write!(f, "not a kit file: {why}"),
            KitError::Invalid(why) => write!(f, "invalid kit: {why}"),
            KitError::Sample { file, why } => write!(f, "sample {file:?}: {why}"),
        }
    }
}

impl std::error::Error for KitError {}

impl Kit {
    /// Reads and validates a kit from its JSON text.
    pub fn from_json(text: &str) -> Result<Kit, KitError> {
        let mut kit: Kit = serde_json::from_str(text).map_err(|e| KitError::Json(e.to_string()))?;
        for pad in &mut kit.pads {
            if pad.name.is_empty() {
                pad.name = pad.id.clone();
            }
        }
        kit.validate()?;
        Ok(kit)
    }

    /// The kit as pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a kit serializes")
    }

    pub fn pad(&self, id: &str) -> Option<&Pad> {
        self.pads.iter().find(|p| p.id == id)
    }

    /// Checks the rules a kit must satisfy: a known version, a name,
    /// between one and [`MAX_PADS`] pads with unique non-empty ids, each
    /// with between one and [`MAX_LAYERS`] layers naming a file next to
    /// the kit, velocity ranges inside 0 to 1, and finite gains and pans.
    pub fn validate(&self) -> Result<(), KitError> {
        let invalid = |why: String| Err(KitError::Invalid(why));
        if self.version == 0 || self.version > KIT_VERSION {
            return invalid(format!(
                "version {} is not supported; this Cantina reads version {KIT_VERSION}",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            return invalid("the kit has no name".into());
        }
        if self.pads.is_empty() {
            return invalid("the kit has no pads".into());
        }
        if self.pads.len() > MAX_PADS {
            return invalid(format!(
                "the kit has {} pads; at most {MAX_PADS} are allowed",
                self.pads.len()
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for pad in &self.pads {
            if pad.id.trim().is_empty() {
                return invalid("a pad has no id".into());
            }
            if !ids.insert(pad.id.as_str()) {
                return invalid(format!("pad id {:?} is used more than once", pad.id));
            }
            if !pad.gain_db.is_finite() || !pad.pan.is_finite() {
                return invalid(format!(
                    "pad {:?} has a gain or pan that is not a number",
                    pad.id
                ));
            }
            if !(-1.0..=1.0).contains(&pad.pan) {
                return invalid(format!(
                    "pad {:?} pans to {}, outside -1 to 1",
                    pad.id, pad.pan
                ));
            }
            if let Some(choke) = &pad.choke {
                if choke.trim().is_empty() {
                    return invalid(format!("pad {:?} has an empty choke group", pad.id));
                }
            }
            if pad.layers.is_empty() {
                return invalid(format!("pad {:?} has no layers", pad.id));
            }
            if pad.layers.len() > MAX_LAYERS {
                return invalid(format!(
                    "pad {:?} has {} layers; at most {MAX_LAYERS} are allowed",
                    pad.id,
                    pad.layers.len()
                ));
            }
            for layer in &pad.layers {
                check_file(&pad.id, &layer.file)?;
                let [lo, hi] = layer.velocity;
                if !(lo.is_finite() && hi.is_finite()) || lo < 0.0 || hi > 1.0 || lo > hi {
                    return invalid(format!(
                        "pad {:?} layer {:?} answers velocities {lo} to {hi}; \
                         the range must lie inside 0 to 1 with the low end first",
                        pad.id, layer.file
                    ));
                }
                if !layer.gain_db.is_finite() {
                    return invalid(format!(
                        "pad {:?} layer {:?} has a gain that is not a number",
                        pad.id, layer.file
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A sample file must be a plain name next to the kit, so a kit can
/// never reach outside its own folder.
fn check_file(pad: &str, file: &str) -> Result<(), KitError> {
    if file.is_empty() {
        return Err(KitError::Invalid(format!(
            "pad {pad:?} has a layer with no file"
        )));
    }
    if file.contains("..") || file.starts_with('/') || file.contains('\\') {
        return Err(KitError::Invalid(format!(
            "pad {pad:?} refers to {file:?}, which is not a file next to the kit"
        )));
    }
    Ok(())
}

/// A sample in memory: one or two channels of `f32` at its own rate. The
/// sampler plays it at the engine's rate, so a kit recorded at 44.1 kHz
/// sounds right in a 48 kHz project.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub sample_rate: f64,
    /// One channel for mono, two for stereo. Every channel is the same
    /// length.
    pub channels: Vec<Vec<f32>>,
}

impl Sample {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, |c| c.len())
    }
}

/// A layer with its sample.
#[derive(Debug, Clone)]
pub struct LoadedLayer {
    pub velocity: [f32; 2],
    /// Linear.
    pub gain: f32,
    pub sample: Arc<Sample>,
}

/// A pad with its samples, gains resolved to linear and the choke group
/// to a number.
#[derive(Debug, Clone)]
pub struct LoadedPad {
    pub id: String,
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    /// Index of the choke group among the kit's groups.
    pub choke: Option<usize>,
    pub layers: Vec<LoadedLayer>,
}

impl LoadedPad {
    /// The layer for a velocity: the first whose range holds it, else the
    /// one whose range is nearest.
    pub fn layer_for(&self, velocity: f32) -> usize {
        if let Some(i) = self
            .layers
            .iter()
            .position(|l| l.velocity[0] <= velocity && velocity <= l.velocity[1])
        {
            return i;
        }
        let distance = |l: &LoadedLayer| {
            let [lo, hi] = l.velocity;
            if velocity < lo {
                lo - velocity
            } else {
                velocity - hi
            }
        };
        self.layers
            .iter()
            .enumerate()
            .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
            .map_or(0, |(i, _)| i)
    }
}

/// A kit ready to play: every layer's sample is in memory.
#[derive(Debug, Clone, Default)]
pub struct LoadedKit {
    pub name: String,
    pads: Vec<LoadedPad>,
    /// Choke group names, indexed by [`LoadedPad::choke`].
    groups: Vec<String>,
}

impl LoadedKit {
    /// A kit with no pads, which plays silence. For a project whose kit
    /// is not installed.
    pub fn empty(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pads: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Builds a kit, asking `load` for each layer's sample by file name.
    /// A file used by several layers is asked for once per layer, so a
    /// loader that reads files should cache.
    pub fn build(
        kit: &Kit,
        mut load: impl FnMut(&str) -> Result<Arc<Sample>, String>,
    ) -> Result<Self, KitError> {
        kit.validate()?;
        let mut groups: Vec<String> = Vec::new();
        let mut pads = Vec::with_capacity(kit.pads.len());
        for pad in &kit.pads {
            let choke =
                pad.choke
                    .as_ref()
                    .map(|name| match groups.iter().position(|g| g == name) {
                        Some(i) => i,
                        None => {
                            groups.push(name.clone());
                            groups.len() - 1
                        }
                    });
            let mut layers = Vec::with_capacity(pad.layers.len());
            for layer in &pad.layers {
                let sample = load(&layer.file).map_err(|why| KitError::Sample {
                    file: layer.file.clone(),
                    why,
                })?;
                if sample.channels.is_empty() || sample.channels.len() > 2 {
                    return Err(KitError::Sample {
                        file: layer.file.clone(),
                        why: format!(
                            "has {} channels; samples are mono or stereo",
                            sample.channels.len()
                        ),
                    });
                }
                if sample.channels.iter().any(|c| c.len() != sample.frames()) {
                    return Err(KitError::Sample {
                        file: layer.file.clone(),
                        why: "channels differ in length".into(),
                    });
                }
                if !(sample.sample_rate.is_finite() && sample.sample_rate > 0.0) {
                    return Err(KitError::Sample {
                        file: layer.file.clone(),
                        why: format!("sample rate {} is not usable", sample.sample_rate),
                    });
                }
                layers.push(LoadedLayer {
                    velocity: layer.velocity,
                    gain: db_to_linear(layer.gain_db),
                    sample,
                });
            }
            pads.push(LoadedPad {
                id: pad.id.clone(),
                name: pad.name.clone(),
                gain: db_to_linear(pad.gain_db),
                pan: pad.pan,
                choke,
                layers,
            });
        }
        Ok(Self {
            name: kit.name.clone(),
            pads,
            groups,
        })
    }

    pub fn pads(&self) -> &[LoadedPad] {
        &self.pads
    }

    pub fn pad_index(&self, id: &str) -> Option<usize> {
        self.pads.iter().position(|p| p.id == id)
    }

    /// The choke group names, in the order pads first mentioned them.
    pub fn choke_groups(&self) -> &[String] {
        &self.groups
    }

    /// Frames of sample data held, summed over every layer.
    pub fn frames(&self) -> usize {
        self.pads
            .iter()
            .flat_map(|p| p.layers.iter())
            .map(|l| l.sample.frames())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIT: &str = r#"{
      "version": 1,
      "name": "Test",
      "pads": [
        { "id": "kick", "layers": [
          { "file": "kick-soft.wav", "velocity": [0, 0.6] },
          { "file": "kick-hard.wav", "velocity": [0.6, 1], "gainDb": -3 }
        ] },
        { "id": "hh-closed", "name": "Closed hat", "choke": "hat", "pan": 0.25,
          "layers": [{ "file": "hat.wav" }] },
        { "id": "hh-open", "name": "Open hat", "choke": "hat", "gainDb": -6,
          "layers": [{ "file": "hat.wav" }] }
      ]
    }"#;

    fn sample(frames: usize) -> Arc<Sample> {
        Arc::new(Sample {
            sample_rate: 48_000.0,
            channels: vec![vec![0.5; frames]],
        })
    }

    #[test]
    fn parses_fills_defaults_and_round_trips() {
        let kit = Kit::from_json(KIT).unwrap();
        assert_eq!(kit.name, "Test");
        assert_eq!(kit.pads.len(), 3);
        assert_eq!(kit.pads[0].name, "kick", "name defaults to the id");
        assert_eq!(kit.pads[1].name, "Closed hat");
        assert_eq!(kit.pads[1].layers[0].velocity, [0.0, 1.0]);
        assert_eq!(kit.pads[1].choke.as_deref(), Some("hat"));
        assert_eq!(kit.pad("hh-open").unwrap().gain_db, -6.0);
        let again = Kit::from_json(&kit.to_json()).unwrap();
        assert_eq!(again, kit);
        assert!(!kit.to_json().contains("\"choke\": null"));
    }

    #[test]
    fn rejects_broken_kits_with_a_reason() {
        let cases: &[(&str, &str)] = &[
            ("{", "not a kit file"),
            (r#"{"version":2,"name":"x","pads":[]}"#, "version 2"),
            (r#"{"version":1,"name":"  ","pads":[]}"#, "no name"),
            (r#"{"version":1,"name":"x","pads":[]}"#, "no pads"),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"","layers":[{"file":"a.wav"}]}]}"#,
                "no id",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[{"file":"a.wav"}]},{"id":"a","layers":[{"file":"a.wav"}]}]}"#,
                "more than once",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[]}]}"#,
                "no layers",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[{"file":"../a.wav"}]}]}"#,
                "not a file next to the kit",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[{"file":"a.wav","velocity":[0.7,0.2]}]}]}"#,
                "low end first",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[{"file":"a.wav","velocity":[0,2]}]}]}"#,
                "inside 0 to 1",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","pan":3,"layers":[{"file":"a.wav"}]}]}"#,
                "outside -1 to 1",
            ),
            (
                r#"{"version":1,"name":"x","pads":[{"id":"a","choke":"","layers":[{"file":"a.wav"}]}]}"#,
                "empty choke group",
            ),
        ];
        for (text, expected) in cases {
            let err = Kit::from_json(text).unwrap_err().to_string();
            assert!(err.contains(expected), "{text}: {err}");
        }
    }

    #[test]
    fn builds_with_samples_and_resolves_groups() {
        let kit = Kit::from_json(KIT).unwrap();
        let mut asked = Vec::new();
        let loaded = LoadedKit::build(&kit, |file| {
            asked.push(file.to_string());
            Ok(sample(10))
        })
        .unwrap();
        assert_eq!(
            asked,
            ["kick-soft.wav", "kick-hard.wav", "hat.wav", "hat.wav"]
        );
        assert_eq!(loaded.pads().len(), 3);
        assert_eq!(loaded.pad_index("hh-open"), Some(2));
        assert_eq!(loaded.pad_index("snare"), None);
        assert_eq!(loaded.choke_groups(), ["hat".to_string()]);
        assert_eq!(loaded.pads()[0].choke, None);
        assert_eq!(loaded.pads()[1].choke, Some(0));
        assert_eq!(loaded.pads()[2].choke, Some(0));
        assert!((loaded.pads()[2].gain - 0.501).abs() < 1e-3);
        assert!((loaded.pads()[0].layers[1].gain - 0.708).abs() < 1e-3);
        assert_eq!(loaded.frames(), 40);
        assert_eq!(loaded.pads()[0].layer_for(0.3), 0);
        assert_eq!(loaded.pads()[0].layer_for(0.6), 0, "the first match wins");
        assert_eq!(loaded.pads()[0].layer_for(0.9), 1);
    }

    #[test]
    fn nearest_layer_covers_a_gap() {
        let kit = Kit::from_json(
            r#"{"version":1,"name":"x","pads":[{"id":"a","layers":[
              {"file":"lo.wav","velocity":[0,0.3]},{"file":"hi.wav","velocity":[0.8,1]}]}]}"#,
        )
        .unwrap();
        let loaded = LoadedKit::build(&kit, |_| Ok(sample(1))).unwrap();
        assert_eq!(loaded.pads()[0].layer_for(0.4), 0);
        assert_eq!(loaded.pads()[0].layer_for(0.7), 1);
    }

    #[test]
    fn bad_samples_are_refused() {
        let kit = Kit::from_json(KIT).unwrap();
        let err = LoadedKit::build(&kit, |file| Err(format!("cannot read {file}"))).unwrap_err();
        assert_eq!(
            err,
            KitError::Sample {
                file: "kick-soft.wav".into(),
                why: "cannot read kick-soft.wav".into()
            }
        );
        let three = Arc::new(Sample {
            sample_rate: 48_000.0,
            channels: vec![vec![0.0; 4]; 3],
        });
        let err = LoadedKit::build(&kit, |_| Ok(Arc::clone(&three))).unwrap_err();
        assert!(err.to_string().contains("3 channels"), "{err}");
        let ragged = Arc::new(Sample {
            sample_rate: 48_000.0,
            channels: vec![vec![0.0; 4], vec![0.0; 5]],
        });
        let err = LoadedKit::build(&kit, |_| Ok(Arc::clone(&ragged))).unwrap_err();
        assert!(err.to_string().contains("differ in length"), "{err}");
        let empty = LoadedKit::empty("Missing");
        assert!(empty.pads().is_empty());
        assert_eq!(empty.name, "Missing");
    }
}
