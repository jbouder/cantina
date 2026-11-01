//! User-designed pedals: a JSON graph of DSP primitives, compiled into
//! a real-time-safe [`Processor`].
//!
//! A pedal file names its parameters and a graph of nodes joined by
//! edges. Each node is a primitive this crate already runs its stock
//! pedals on (a gain, a biquad, a waveshaper behind the oversampler, a
//! delay line, an oscillator, a cascade of allpasses) and every field
//! of a node is either a constant, a parameter with a range mapped onto
//! it, or a signal from another node:
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Warm Drive",
//!   "params": [
//!     { "id": "drive", "label": "Drive", "unit": "dB", "min": 0, "max": 30, "default": 12 }
//!   ],
//!   "nodes": [
//!     { "id": "in", "kind": "input" },
//!     { "id": "pre", "kind": "biquad", "type": "highpass", "freq": 80 },
//!     { "id": "gain", "kind": "gain", "db": { "param": "drive" } },
//!     { "id": "clip", "kind": "waveshaper", "curve": "soft", "oversample": 4 },
//!     { "id": "tone", "kind": "biquad", "type": "lowpass", "freq": 4500 },
//!     { "id": "out", "kind": "output" }
//!   ],
//!   "edges": [["in", "pre"], ["pre", "gain"], ["gain", "clip"], ["clip", "tone"], ["tone", "out"]]
//! }
//! ```
//!
//! [`PedalDef`] is the file as data; [`compile`] validates it, naming
//! the node behind every problem, and builds a [`PedalProcessor`]. All
//! allocation happens there: `process` only ever walks buffers reserved
//! in `prepare`. The same format describes the stock pedals that are
//! compositions of these primitives, so a designer can open any of them
//! as a starting point. The reference is `docs/pedal-format.md`.
//!
//! [`Processor`]: crate::Processor

mod compile;
mod kinds;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;

use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

pub use compile::{compile, PedalProcessor};
pub use kinds::{kinds, FieldInfo, FieldKind, KindInfo};

/// The format version this crate reads and writes.
pub const PEDAL_VERSION: u32 = 1;
/// Most nodes a pedal may have.
pub const MAX_NODES: usize = 64;
/// Most parameters a pedal may expose.
pub const MAX_PARAMS: usize = 32;

/// A pedal as its JSON file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalDef {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    #[serde(default)]
    pub nodes: Vec<NodeDef>,
    /// `[from, to]` pairs of node ids: audio flows from the first to
    /// the second.
    #[serde(default)]
    pub edges: Vec<(String, String)>,
}

/// A knob the pedal exposes. Nodes read it through a binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamDef {
    /// What bindings refer to, such as `drive`.
    pub id: String,
    /// Display name; the id when missing.
    #[serde(default)]
    pub label: String,
    /// `dB`, `Hz`, `s`, `ms`, `%` or nothing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(default, serialize_with = "tidy")]
    pub min: f32,
    #[serde(default = "one", serialize_with = "tidy")]
    pub max: f32,
    #[serde(default, serialize_with = "tidy")]
    pub default: f32,
    /// A pick from a list: the value is the index, the range is set
    /// from the list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// A switch: 0 or 1.
    #[serde(default, skip_serializing_if = "is_false")]
    pub toggle: bool,
}

/// One node of the graph: a primitive and its fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: String,
    pub kind: String,
    /// Where the designer drew it; nothing to the sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[Num; 2]>,
    /// The kind's fields, by name. Unknown names are an error.
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

/// A field's value: a number, a name from a list, a switch, or a
/// binding to a parameter or another node's signal.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Name(String),
    Bound(Binding),
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Number(n) => Num(*n).serialize(s),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Name(n) => s.serialize_str(n),
            Value::Bound(b) => b.serialize(s),
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<&str> for Value {
    fn from(n: &str) -> Self {
        Value::Name(n.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

/// How a field follows a parameter or a signal. Exactly one of
/// `param` and `node` is set.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    /// Follow this parameter, at block rate, smoothed where the field
    /// says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Follow this node's output, sample by sample. An `lfo` is the
    /// usual source; any node will do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The field's value at the bottom and top of the source's range:
    /// a parameter's `min` to `max`, a signal's 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<[Num; 2]>,
    /// Interpolate the map exponentially, for frequencies.
    #[serde(default, skip_serializing_if = "is_false")]
    pub log: bool,
    /// For a choice or toggle parameter: the field's value for each
    /// index. Numbers or names, as the field takes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Value>,
}

impl Binding {
    /// A field that follows `param` as it is.
    pub fn param(id: &str) -> Self {
        Self {
            param: Some(id.to_string()),
            ..Self::default()
        }
    }

    /// A field that follows `param` over `lo` to `hi`.
    pub fn mapped(id: &str, lo: f64, hi: f64) -> Self {
        Self {
            param: Some(id.to_string()),
            map: Some([Num(lo), Num(hi)]),
            ..Self::default()
        }
    }

    /// A field that follows `node`'s signal over `lo` to `hi`.
    pub fn signal(node: &str, lo: f64, hi: f64) -> Self {
        Self {
            node: Some(node.to_string()),
            map: Some([Num(lo), Num(hi)]),
            ..Self::default()
        }
    }

    pub fn log(mut self) -> Self {
        self.log = true;
        self
    }

    /// A field that takes `values[index]` for a choice parameter.
    pub fn table(id: &str, values: impl IntoIterator<Item = Value>) -> Self {
        Self {
            param: Some(id.to_string()),
            values: values.into_iter().collect(),
            ..Self::default()
        }
    }
}

/// A number that serializes without a fraction when it has none, so a
/// file says `"freq": 80` rather than `80.0`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Deserialize)]
#[serde(transparent)]
pub struct Num(pub f64);

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let n = self.0;
        if n.fract() == 0.0 && n.abs() < 1e15 {
            s.serialize_i64(n as i64)
        } else {
            s.serialize_f64(n)
        }
    }
}

fn tidy<S: Serializer>(value: &f32, s: S) -> Result<S::Ok, S::Error> {
    Num(*value as f64).serialize(s)
}

fn one() -> f32 {
    1.0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One thing wrong with a pedal, and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The node it is about, if one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The field of that node, if one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

impl Problem {
    fn pedal(message: impl Into<String>) -> Self {
        Self {
            node: None,
            field: None,
            message: message.into(),
        }
    }

    fn node(id: &str, message: impl Into<String>) -> Self {
        Self {
            node: Some(id.to_string()),
            field: None,
            message: message.into(),
        }
    }

    fn field(id: &str, field: &str, message: impl Into<String>) -> Self {
        Self {
            node: Some(id.to_string()),
            field: Some(field.to_string()),
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.node, &self.field) {
            (Some(node), Some(field)) => write!(f, "node {node:?}, {field}: {}", self.message),
            (Some(node), None) => write!(f, "node {node:?}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

/// Why a pedal could not be read or built.
#[derive(Debug, Clone, PartialEq)]
pub enum PedalError {
    /// The text is not JSON, or not the pedal shape.
    Json(String),
    /// The pedal is JSON but breaks the rules; every problem found.
    Invalid(Vec<Problem>),
}

impl PedalError {
    /// Every problem as one line each.
    pub fn problems(&self) -> Vec<Problem> {
        match self {
            PedalError::Json(why) => vec![Problem::pedal(format!("not a pedal file: {why}"))],
            PedalError::Invalid(problems) => problems.clone(),
        }
    }
}

impl fmt::Display for PedalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PedalError::Json(why) => write!(f, "not a pedal file: {why}"),
            PedalError::Invalid(problems) => {
                let lines: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
                write!(f, "invalid pedal: {}", lines.join("; "))
            }
        }
    }
}

impl std::error::Error for PedalError {}

impl PedalDef {
    /// A pedal with nothing but an input wired to an output.
    pub fn empty(name: &str) -> Self {
        Self {
            version: PEDAL_VERSION,
            name: name.to_string(),
            params: Vec::new(),
            nodes: vec![NodeDef::new("in", "input"), NodeDef::new("out", "output")],
            edges: vec![("in".into(), "out".into())],
        }
    }

    /// Reads a pedal from its JSON text and validates it.
    pub fn from_json(text: &str) -> Result<Self, PedalError> {
        let def: PedalDef =
            serde_json::from_str(text).map_err(|e| PedalError::Json(e.to_string()))?;
        def.validate()?;
        Ok(def)
    }

    /// The pedal as pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a pedal serializes")
    }

    /// Checks every rule, reporting every problem found rather than the
    /// first. Compiling does the same checks.
    pub fn validate(&self) -> Result<(), PedalError> {
        compile::check(self)
    }

    pub fn node(&self, id: &str) -> Option<&NodeDef> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn param(&self, id: &str) -> Option<&ParamDef> {
        self.params.iter().find(|p| p.id == id)
    }
}

impl NodeDef {
    pub fn new(id: &str, kind: &str) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            pos: None,
            fields: BTreeMap::new(),
        }
    }

    /// Builder: sets a field.
    pub fn with(mut self, field: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(field.to_string(), value.into());
        self
    }

    /// Builder: binds a field.
    pub fn bound(mut self, field: &str, binding: Binding) -> Self {
        self.fields.insert(field.to_string(), Value::Bound(binding));
        self
    }
}

impl ParamDef {
    /// A continuous parameter.
    pub fn new(id: &str, label: &str, unit: &str, min: f32, max: f32, default: f32) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            unit: unit.to_string(),
            min,
            max,
            default,
            choices: Vec::new(),
            toggle: false,
        }
    }

    /// A pick from `choices`, starting at `default`.
    pub fn choice(id: &str, label: &str, choices: &[&str], default: usize) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            unit: String::new(),
            min: 0.0,
            max: choices.len().saturating_sub(1) as f32,
            default: default as f32,
            choices: choices.iter().map(|c| c.to_string()).collect(),
            toggle: false,
        }
    }

    /// A switch.
    pub fn toggle(id: &str, label: &str, on: bool) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            unit: String::new(),
            min: 0.0,
            max: 1.0,
            default: if on { 1.0 } else { 0.0 },
            choices: Vec::new(),
            toggle: true,
        }
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn values_round_trip_through_json_without_stray_fractions() {
        let node = NodeDef::new("g", "gain")
            .with("db", 6.0)
            .with("smooth", 0.25)
            .bound("linear", Binding::mapped("mix", 0.0, 1.0))
            .bound("x", Binding::signal("lfo", 1.0, 0.0).log())
            .bound("c", Binding::table("curve", ["soft".into(), 4.0.into()]))
            .with("flag", true)
            .with("name", "soft");
        let text = serde_json::to_string(&node).unwrap();
        assert!(text.contains("\"db\":6,"), "{text}");
        assert!(text.contains("\"smooth\":0.25"), "{text}");
        assert!(text.contains("\"map\":[0,1]"), "{text}");
        assert!(text.contains("\"log\":true"), "{text}");
        assert!(text.contains("\"values\":[\"soft\",4]"), "{text}");
        assert!(text.contains("\"flag\":true"), "{text}");
        let back: NodeDef = serde_json::from_str(&text).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn a_binding_with_an_unknown_key_is_rejected() {
        let text = r#"{"id":"g","kind":"gain","db":{"param":"x","mpa":[0,1]}}"#;
        let err = serde_json::from_str::<NodeDef>(text)
            .unwrap_err()
            .to_string();
        // Untagged enums report generically; what matters is that the
        // typo does not silently become a bare name or get dropped.
        assert!(err.contains("did not match any variant"), "{err}");
    }

    #[test]
    fn params_serialize_tidily() {
        let p = ParamDef::new("drive", "Drive", "dB", 0.0, 40.0, 20.0);
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(
            text,
            r#"{"id":"drive","label":"Drive","unit":"dB","min":0,"max":40,"default":20}"#
        );
        let c = ParamDef::choice("curve", "Curve", &["Soft", "Hard"], 1);
        let text = serde_json::to_string(&c).unwrap();
        assert!(
            text.contains(r#""max":1,"default":1,"choices":["Soft","Hard"]"#),
            "{text}"
        );
        let t = ParamDef::toggle("sync", "Sync", true);
        assert!(serde_json::to_string(&t)
            .unwrap()
            .ends_with(r#""toggle":true}"#));
    }

    #[test]
    fn positions_are_optional_and_kept() {
        let text = r#"{"id":"in","kind":"input","pos":[10,20.5]}"#;
        let node: NodeDef = serde_json::from_str(text).unwrap();
        assert_eq!(node.pos, Some([Num(10.0), Num(20.5)]));
        assert!(node.fields.is_empty());
        assert_eq!(serde_json::to_string(&node).unwrap(), text);
    }

    #[test]
    fn problems_display_with_their_place() {
        assert_eq!(Problem::pedal("no name").to_string(), "no name");
        assert_eq!(
            Problem::node("g", "no input").to_string(),
            "node \"g\": no input"
        );
        assert_eq!(
            Problem::field("g", "db", "not a number").to_string(),
            "node \"g\", db: not a number"
        );
        let err = PedalError::Invalid(vec![Problem::pedal("a"), Problem::node("n", "b")]);
        assert_eq!(err.to_string(), "invalid pedal: a; node \"n\": b");
        assert_eq!(err.problems().len(), 2);
        assert_eq!(
            PedalError::Json("x".into()).problems()[0].message,
            "not a pedal file: x"
        );
    }
}
