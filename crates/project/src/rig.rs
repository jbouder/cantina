//! Rigs: a chain of pedals with their settings, as a file of its own,
//! `<name>.rig.json`. A rig is what "a metalcore tone" is to Cantina: a
//! name, a genre, a line on what it is for, the instrument it suits, the
//! input gain it expects and the pedals in order.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Metalcore",
//!   "genre": "Metal",
//!   "description": "Tight, gated high gain with a scooped mid.",
//!   "instrument": "guitar",
//!   "inputGainDb": 0,
//!   "chain": [
//!     { "pedal": "gate", "params": { "threshold": -55 } },
//!     { "pedal": "overdrive", "params": { "drive": 4, "tone": 3500, "level": 0 } },
//!     { "pedal": "amp", "params": { "gain": 32, "mid": -4 } },
//!     { "pedal": "cab:4x12-modern" }
//!   ]
//! }
//! ```
//!
//! A pedal is named the way a project names one: a stock name, `cab:<impulse>`,
//! `pedal:<designed pedal id>`, `au:…` or `clap:…`. Its parameters are named
//! rather than listed in order, by the pedal format's id or the label the
//! pedal shows, case aside; a parameter left out starts at its default. A
//! choice may be given by its option's name and a switch as `true` or
//! `false`. A plugin also carries its saved state, base64, in `state`.
//!
//! This module is the shape and the rules that need no audio: what a
//! parameter name resolves to is [`resolve_values`], given the parameters
//! the pedal turns out to have, which only whoever builds it knows.
//! `rig.schema.json` describes the file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tab::Instrument;

/// What a rig file's name ends with.
pub const RIG_SUFFIX: &str = ".rig.json";

/// The version a rig is written at. An older one is upgraded on read.
pub const RIG_VERSION: u32 = 1;

/// The input gain a rig may ask for, in decibels, either way.
pub const MAX_INPUT_GAIN_DB: f32 = 24.0;

/// A chain of pedals with their settings, as a file.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rig {
    /// The format version, 1.
    pub version: u32,
    /// Shown in the rig picker.
    pub name: String,
    /// The style it is for, as the picker groups rigs: `Metal`, `Rock`, `Blues`…
    #[serde(default)]
    pub genre: String,
    /// One line on what it sounds like.
    #[serde(default)]
    pub description: String,
    /// What it is for: a guitar or a bass.
    pub instrument: Instrument,
    /// Gain in decibels before the first pedal, −24 to 24: how hot the rig
    /// wants its input. The instrument profile trims it further in the app.
    #[serde(default)]
    pub input_gain_db: f32,
    /// The pedals, first to last.
    pub chain: Vec<RigPedal>,
}

/// One pedal in a rig.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigPedal {
    /// What it is: a stock name (`overdrive`), `cab:<impulse id>`,
    /// `pedal:<designed pedal id>`, `au:…` or `clap:…`.
    pub pedal: String,
    /// Parameter values by the pedal format's id or the pedal's label, case
    /// aside. One left out starts at its default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// A plugin's saved state, base64, applied before `params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Bypassed, the signal goes round the pedal; it keeps its settings.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypassed: bool,
}

/// A parameter's value: a number, a choice's option by name, or a switch.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    /// The value itself, in the parameter's unit.
    Number(f32),
    /// A switch: on or off.
    Switch(bool),
    /// One of a choice's options, by name.
    Option(String),
}

/// A parameter as [`resolve_values`] matches names against: what the
/// processor reports, plus the pedal format's id when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSlot {
    /// The pedal format's id (`drive`), for pedals defined in it.
    pub id: Option<String>,
    /// The label the pedal shows (`Drive`).
    pub name: String,
    /// The smallest value.
    pub min: f32,
    /// The largest value.
    pub max: f32,
    /// What it starts at.
    pub default: f32,
    /// A choice's options, in order; empty for anything else.
    pub options: Vec<String>,
    /// An on/off switch.
    pub switch: bool,
}

/// Why a rig does not read or a pedal's parameters do not resolve.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RigError {
    /// Not JSON, or not shaped like a rig.
    #[error("not a rig: {0}")]
    Shape(String),
    /// Every problem the rules found, the first first.
    #[error("{}", .0.join("; "))]
    Invalid(Vec<String>),
}

impl RigError {
    /// One line per problem.
    pub fn problems(&self) -> Vec<String> {
        match self {
            RigError::Shape(s) => vec![s.clone()],
            RigError::Invalid(p) => p.clone(),
        }
    }
}

/// Reads and checks a rig file's text. A `"$schema"` field is ignored.
pub fn rig_from_json(text: &str) -> Result<Rig, RigError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| RigError::Shape(e.to_string()))?;
    if let Some(o) = value.as_object_mut() {
        o.remove("$schema");
    }
    let rig: Rig = serde_json::from_value(value).map_err(|e| RigError::Shape(e.to_string()))?;
    rig.validate()?;
    Ok(rig)
}

/// A rig as the text of its file.
pub fn rig_to_json(rig: &Rig) -> String {
    let mut text = serde_json::to_string_pretty(rig).expect("a rig serializes");
    text.push('\n');
    text
}

/// The file name for a rig id.
pub fn rig_file_name(id: &str) -> String {
    format!("{id}{RIG_SUFFIX}")
}

impl Rig {
    /// The rules beyond the shape: a version this build reads, a name, an
    /// input gain in range, every pedal named, values that are numbers and
    /// state only on a plugin.
    pub fn validate(&self) -> Result<(), RigError> {
        let mut problems = Vec::new();
        if self.version == 0 || self.version > RIG_VERSION {
            problems.push(format!(
                "version {} is not one this Cantina reads (1 to {RIG_VERSION})",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            problems.push("the name is empty".to_string());
        }
        if !self.input_gain_db.is_finite() || self.input_gain_db.abs() > MAX_INPUT_GAIN_DB {
            problems.push(format!(
                "inputGainDb {} is outside −{MAX_INPUT_GAIN_DB} to {MAX_INPUT_GAIN_DB}",
                self.input_gain_db
            ));
        }
        for (i, p) in self.chain.iter().enumerate() {
            let at = format!("chain {i}");
            if p.pedal.trim().is_empty() {
                problems.push(format!("{at}: the pedal is not named"));
                continue;
            }
            let at = format!("{at} ({})", p.pedal);
            for (k, v) in &p.params {
                if matches!(v, ParamValue::Number(n) if !n.is_finite()) {
                    problems.push(format!("{at}: {k} is not a number"));
                }
            }
            if let Some(state) = &p.state {
                if !p.is_plugin() {
                    problems.push(format!("{at}: only a plugin carries a state"));
                } else if decode_base64(state).is_none() {
                    problems.push(format!("{at}: the state is not base64"));
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(RigError::Invalid(problems))
        }
    }
}

impl RigPedal {
    /// An Audio Unit or a CLAP plugin.
    pub fn is_plugin(&self) -> bool {
        self.pedal.starts_with("au:") || self.pedal.starts_with("clap:")
    }

    /// The plugin state, decoded. `None` without one or when it is not
    /// base64, which [`Rig::validate`] has already refused.
    pub fn state_bytes(&self) -> Option<Vec<u8>> {
        self.state.as_deref().and_then(decode_base64)
    }

    /// A rig pedal from a chain's: every parameter named, by the pedal
    /// format's id where there is one, and for a plugin its state instead,
    /// which carries the parameters with it.
    pub fn from_values(
        pedal: &str,
        values: &[f32],
        slots: &[ParamSlot],
        state: Option<&[u8]>,
        bypassed: bool,
    ) -> Self {
        let mut params = BTreeMap::new();
        if state.is_none() {
            for (slot, v) in slots.iter().zip(values) {
                let key = slot.id.clone().unwrap_or_else(|| slot.name.to_lowercase());
                let value = if slot.switch {
                    ParamValue::Switch(*v >= 0.5)
                } else if let Some(o) = slot.options.get(v.round().max(0.0) as usize) {
                    ParamValue::Option(o.clone())
                } else {
                    ParamValue::Number(*v)
                };
                params.entry(key).or_insert(value);
            }
        }
        Self {
            pedal: pedal.to_string(),
            params,
            state: state.map(encode_base64),
            bypassed,
        }
    }
}

/// The values a rig pedal sets, one per slot in order, with the slots it
/// leaves out at their defaults. A name that matches no parameter, or two,
/// a value outside a parameter's range, or an option a choice does not
/// have, is a problem.
pub fn resolve_values(pedal: &RigPedal, slots: &[ParamSlot]) -> Result<Vec<f32>, Vec<String>> {
    let mut values: Vec<f32> = slots.iter().map(|s| s.default).collect();
    let mut problems = Vec::new();
    for (key, value) in &pedal.params {
        let wanted = key.to_lowercase();
        let by_id: Vec<usize> = (0..slots.len())
            .filter(|i| slots[*i].id.as_deref().map(str::to_lowercase) == Some(wanted.clone()))
            .collect();
        let found = if by_id.is_empty() {
            (0..slots.len())
                .filter(|i| slots[*i].name.to_lowercase() == wanted)
                .collect()
        } else {
            by_id
        };
        let index = match found.as_slice() {
            [i] => *i,
            [] => {
                let names: Vec<String> = slots
                    .iter()
                    .map(|s| s.id.clone().unwrap_or_else(|| s.name.to_lowercase()))
                    .collect();
                problems.push(format!(
                    "{} has no parameter {key:?}; it has {}",
                    pedal.pedal,
                    if names.is_empty() {
                        "none".to_string()
                    } else {
                        names.join(", ")
                    }
                ));
                continue;
            }
            _ => {
                problems.push(format!(
                    "{} has more than one parameter called {key:?}; use its id",
                    pedal.pedal
                ));
                continue;
            }
        };
        let slot = &slots[index];
        let v = match value {
            ParamValue::Number(n) => {
                if *n < slot.min - 1e-4 || *n > slot.max + 1e-4 {
                    problems.push(format!(
                        "{} {key} is {n}, outside {} to {}",
                        pedal.pedal, slot.min, slot.max
                    ));
                    continue;
                }
                *n
            }
            ParamValue::Switch(on) => {
                if !slot.switch {
                    problems.push(format!("{} {key} is not a switch", pedal.pedal));
                    continue;
                }
                if *on {
                    1.0
                } else {
                    0.0
                }
            }
            ParamValue::Option(name) => {
                let Some(i) = slot
                    .options
                    .iter()
                    .position(|o| o.eq_ignore_ascii_case(name))
                else {
                    problems.push(if slot.options.is_empty() {
                        format!("{} {key} takes a number, not {name:?}", pedal.pedal)
                    } else {
                        format!(
                            "{} {key} has no option {name:?}; it has {}",
                            pedal.pedal,
                            slot.options.join(", ")
                        )
                    });
                    continue;
                };
                i as f32
            }
        };
        values[index] = v;
    }
    if problems.is_empty() {
        Ok(values)
    } else {
        Err(problems)
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64, padded or not, whitespace ignored. `None` when it is
/// not base64.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .take_while(|b| *b != b'=')
        .map(|b| BASE64.iter().position(|c| *c == b).map(|i| i as u8))
        .collect::<Option<_>>()?;
    if digits.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut n = 0u32;
        for (i, d) in chunk.iter().enumerate() {
            n |= (*d as u32) << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: Option<&str>, name: &str, min: f32, max: f32, default: f32) -> ParamSlot {
        ParamSlot {
            id: id.map(str::to_string),
            name: name.to_string(),
            min,
            max,
            default,
            options: Vec::new(),
            switch: false,
        }
    }

    fn drive_slots() -> Vec<ParamSlot> {
        vec![
            slot(Some("drive"), "Drive", 0.0, 40.0, 20.0),
            slot(Some("level"), "Level", -24.0, 6.0, -6.0),
            ParamSlot {
                options: vec!["soft".into(), "hard".into()],
                ..slot(Some("curve"), "Curve", 0.0, 1.0, 0.0)
            },
            ParamSlot {
                switch: true,
                ..slot(None, "Sync", 0.0, 1.0, 0.0)
            },
        ]
    }

    #[test]
    fn a_rig_reads_and_writes_back_the_same() {
        let text = r#"{
            "$schema": "https://example/rig.schema.json",
            "version": 1, "name": "Crunch", "genre": "Rock",
            "description": "A little hair.", "instrument": "guitar", "inputGainDb": -3,
            "chain": [
                { "pedal": "overdrive", "params": { "drive": 12, "curve": "hard", "sync": true } },
                { "pedal": "cab:2x12-closed", "bypassed": true },
                { "pedal": "au:aufx:dely:appl", "state": "YnBsaXN0" }
            ]
        }"#;
        let rig = rig_from_json(text).unwrap();
        assert_eq!(rig.chain.len(), 3);
        assert_eq!(rig.chain[2].state_bytes().unwrap(), b"bplist");
        assert_eq!(rig_from_json(&rig_to_json(&rig)).unwrap(), rig);
    }

    #[test]
    fn the_rules_catch_what_the_shape_does_not() {
        let mut rig = Rig {
            version: 2,
            name: " ".into(),
            genre: String::new(),
            description: String::new(),
            instrument: Instrument::Bass,
            input_gain_db: 30.0,
            chain: vec![
                RigPedal {
                    pedal: "".into(),
                    params: BTreeMap::new(),
                    state: None,
                    bypassed: false,
                },
                RigPedal {
                    pedal: "fuzz".into(),
                    params: BTreeMap::new(),
                    state: Some("AAAA".into()),
                    bypassed: false,
                },
                RigPedal {
                    pedal: "clap:x".into(),
                    params: BTreeMap::new(),
                    state: Some("!!".into()),
                    bypassed: false,
                },
            ],
        };
        let problems = rig.validate().unwrap_err().problems();
        assert_eq!(problems.len(), 6, "{problems:#?}");
        rig.version = 1;
        rig.name = "Fine".into();
        rig.input_gain_db = 0.0;
        rig.chain.clear();
        rig.validate().unwrap();
    }

    #[test]
    fn params_resolve_by_id_or_label_with_the_rest_at_defaults() {
        let pedal: RigPedal = serde_json::from_str(
            r#"{ "pedal": "overdrive", "params": { "DRIVE": 30, "Curve": "Hard", "sync": true } }"#,
        )
        .unwrap();
        assert_eq!(
            resolve_values(&pedal, &drive_slots()).unwrap(),
            vec![30.0, -6.0, 1.0, 1.0]
        );
    }

    #[test]
    fn bad_params_name_what_is_wrong() {
        let pedal: RigPedal = serde_json::from_str(
            r#"{ "pedal": "overdrive", "params": {
                "volume": 1, "drive": 99, "curve": "fuzzy", "level": true, "sync": "x" } }"#,
        )
        .unwrap();
        let problems = resolve_values(&pedal, &drive_slots()).unwrap_err();
        assert_eq!(problems.len(), 5, "{problems:#?}");
        assert!(problems
            .iter()
            .any(|p| p.contains("no parameter \"volume\"")));
        assert!(problems.iter().any(|p| p.contains("outside 0 to 40")));
        assert!(problems.iter().any(|p| p.contains("no option \"fuzzy\"")));
    }

    #[test]
    fn a_label_two_params_share_needs_the_id() {
        let slots = vec![
            slot(Some("gain1"), "Gain", -18.0, 18.0, 0.0),
            slot(Some("gain2"), "Gain", -18.0, 18.0, 0.0),
        ];
        let by_label: RigPedal =
            serde_json::from_str(r#"{ "pedal": "eq", "params": { "gain": 3 } }"#).unwrap();
        assert!(resolve_values(&by_label, &slots).unwrap_err()[0].contains("more than one"));
        let by_id: RigPedal =
            serde_json::from_str(r#"{ "pedal": "eq", "params": { "gain2": 3 } }"#).unwrap();
        assert_eq!(resolve_values(&by_id, &slots).unwrap(), vec![0.0, 3.0]);
    }

    #[test]
    fn a_chain_pedal_becomes_a_rig_pedal_that_resolves_back() {
        let slots = drive_slots();
        let values = vec![33.0, -2.5, 1.0, 1.0];
        let p = RigPedal::from_values("overdrive", &values, &slots, None, true);
        assert_eq!(p.params["curve"], ParamValue::Option("hard".into()));
        assert_eq!(p.params["sync"], ParamValue::Switch(true));
        assert_eq!(resolve_values(&p, &slots).unwrap(), values);
        let plugin = RigPedal::from_values("au:x", &values, &slots, Some(b"state"), false);
        assert!(plugin.params.is_empty());
        assert_eq!(plugin.state_bytes().unwrap(), b"state");
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..20 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let text = encode_base64(&bytes);
            assert_eq!(decode_base64(&text).unwrap(), bytes, "{text}");
        }
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(decode_base64("TWE").unwrap(), b"Ma");
        assert!(decode_base64("T").is_none());
        assert!(decode_base64("T*==").is_none());
    }
}
