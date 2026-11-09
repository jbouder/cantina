//! The assistant's tools: what the model can read of the project and
//! the engine, and the edits it can make.
//!
//! Every tool is one entry in [`TOOLS`]: a name, what it is for, a JSON
//! Schema for its arguments, whether it asks the player first, and the
//! function that runs it. The definitions the model sees are generated
//! from that table, and arguments are checked against the schema before
//! a tool runs.
//!
//! Tools run against a [`Host`], which the shell's `Engine` implements.
//! Every edit is a project `Command` applied through `Engine::apply`, so
//! it is undoable, journalled and reconciled into the live graph, the
//! same as an edit from the UI; one tool call is one undo step.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use cantina_dsp::{ParamInfo, Unit};
use cantina_project::{
    Clip, ClipId, Command, LoopRegion, Pattern, PatternClip, PatternClipId, PatternId, PatternRow,
    PedalId, Project, Tempo, Track, TrackId, MAX_PATTERN_STEPS, PPQ,
};
use serde_json::{json, Value};

use super::author::{self, Kind};
use super::chat::Outcome;
use crate::library::Folders;

/// What the tools work on: the open project and what the engine knows
/// about pedals, kits and devices. `Engine` is the real one.
pub trait Host {
    /// The project as it is now.
    fn project(&self) -> &Project;
    /// The project's revision, one more with every edit, undo and redo.
    fn revision(&self) -> u64;
    /// Applies an edit through the same path as the UI's edits.
    fn apply(&mut self, command: Command) -> Result<()>;
    /// The kinds of pedal `add_pedal` takes: spec and name.
    fn pedal_kinds(&self) -> Vec<(String, String)>;
    /// A kind's name and parameters.
    fn pedal_kind(&mut self, spec: &str) -> Result<(String, Vec<ParamInfo>)>;
    /// Adds a pedal to a track's chain at `index` (the end with none),
    /// with these parameter values or the defaults, as one edit; returns
    /// its id.
    fn add_pedal(
        &mut self,
        track: TrackId,
        spec: &str,
        index: Option<usize>,
        values: Option<Vec<f32>>,
    ) -> Result<PedalId>;
    /// The installed drum kits: id and name.
    fn kits(&self) -> Vec<(String, String)>;
    /// A kit's pad ids, in the order the grid shows them.
    fn pads(&self, kit: &str) -> Vec<String>;
    /// Adds a drum track with a kit, the first installed with none;
    /// returns its id.
    fn add_drum_track(&mut self, kit: Option<String>) -> Result<TrackId>;
    /// Removes a track. Refused while it is recording.
    fn remove_track(&mut self, track: TrackId) -> Result<()>;
    /// The engine as the model should see it: whether audio runs, the
    /// devices, the sample rate and buffer, latency, meters, plugins.
    fn engine_report(&self) -> Value;
    /// The library folders the app watches.
    fn library(&self) -> Folders;
    /// An installed kit's folder.
    fn kit_dir(&self, kit: &str) -> Option<PathBuf>;
    /// Scans the library folders again, so a file just saved is offered
    /// at once.
    fn rescan_library(&mut self);
    /// Adds a pattern file to the project, as Import Pattern does;
    /// returns its id.
    fn import_pattern(&mut self, path: &Path, track: Option<TrackId>) -> Result<PatternId>;
    /// Gives a track these pedals, in order, after its chain or in place
    /// of it, as one edit; returns their ids.
    fn set_chain(
        &mut self,
        track: TrackId,
        pedals: Vec<(String, Vec<f32>)>,
        replace: bool,
    ) -> Result<Vec<PedalId>>;
}

/// What a tool did: one line for the card, what goes back to the model,
/// and whether it changed the project.
pub struct Done {
    pub summary: String,
    pub result: Value,
    pub edited: bool,
}

impl Done {
    fn read(summary: impl Into<String>, result: Value) -> Result<Done> {
        Ok(Done {
            summary: summary.into(),
            result,
            edited: false,
        })
    }

    fn edit(summary: impl Into<String>) -> Result<Done> {
        Ok(Done {
            summary: summary.into(),
            result: json!({ "ok": true }),
            edited: true,
        })
    }
}

/// A tool's question for the player, given the host and the arguments;
/// `None` when this call needs none.
pub type Ask = fn(&dyn Host, &Value) -> Option<String>;

/// One tool.
pub struct Tool {
    pub name: &'static str,
    /// What it is for, as the model reads it.
    pub description: &'static str,
    /// Its arguments' JSON Schema.
    pub schema: fn() -> Value,
    /// For a tool that may destroy something: the question to ask first,
    /// when this call would.
    pub ask: Option<Ask>,
    pub run: fn(&mut dyn Host, &Value) -> Result<Done>,
}

/// An object schema with these properties, all but `optional` required.
fn object(properties: Value, optional: &[&str]) -> Value {
    let required: Vec<&String> = properties
        .as_object()
        .map(|p| {
            p.keys()
                .filter(|k| !optional.contains(&k.as_str()))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

const TRACK: &str = "A track's id from get_project, or its exact name.";
const STEPS: &str = "One character per step: X a hard hit, x a normal hit, o a ghost note, . or - a rest. Spaces and | are ignored, so \"x... x... | x.x. x...\" is 16 steps.";

fn track_arg() -> Value {
    json!({ "type": ["integer", "string"], "description": TRACK })
}

/// Every tool, in the order the model is told about them.
pub static TOOLS: &[Tool] = &[
    Tool {
        name: "get_project",
        description: "Reads the open project: tempo, time signature, loop, every track with its pedal chain (each pedal's id, kind and parameters with values and ranges), mixer settings, clips and placed patterns, and the drum patterns. Call this before changing anything, to get ids.",
        schema: || object(json!({}), &[]),
        ask: None,
        run: |host, _| {
            let summary = format!("Read the project {}", host.project().name);
            Done::read(summary, describe(host))
        },
    },
    Tool {
        name: "get_pattern",
        description: "Reads a drum pattern step by step, one row per pad, in the same step letters set_pattern takes.",
        schema: || object(json!({ "pattern": { "type": "integer", "description": "The pattern's id." } }), &[]),
        ask: None,
        run: |host, args| {
            let pattern = pattern_ref(host.project(), &args["pattern"])?.clone();
            let rows: serde_json::Map<String, Value> = pattern
                .rows
                .iter()
                .map(|r| (r.pad.clone(), Value::String(steps_text(&r.steps))))
                .collect();
            Done::read(
                format!("Read the pattern {}", pattern.name),
                json!({
                    "id": pattern.id.0,
                    "name": pattern.name,
                    "stepsPerBeat": pattern.steps_per_beat,
                    "length": pattern.length,
                    "swing": pattern.swing,
                    "rows": rows,
                }),
            )
        },
    },
    Tool {
        name: "list_pedal_kinds",
        description: "Lists the kinds of pedal add_pedal takes: the stock pedals and the player's library pedals (kind \"pedal:<id>\").",
        schema: || object(json!({}), &[]),
        ask: None,
        run: |host, _| {
            let kinds: Vec<Value> = host
                .pedal_kinds()
                .into_iter()
                .map(|(spec, name)| json!({ "kind": spec, "name": name }))
                .collect();
            Done::read("Listed the pedals", json!({ "kinds": kinds }))
        },
    },
    Tool {
        name: "list_kits",
        description: "Lists the installed drum kits with their pads, for add_drum_track and drum patterns.",
        schema: || object(json!({}), &[]),
        ask: None,
        run: |host, _| {
            let kits: Vec<Value> = host
                .kits()
                .into_iter()
                .map(|(id, name)| json!({ "id": id, "name": name, "pads": host.pads(&id) }))
                .collect();
            Done::read("Listed the drum kits", json!({ "kits": kits }))
        },
    },
    Tool {
        name: "get_engine",
        description: "Reads the audio engine: whether it is running, the input and output devices, sample rate, buffer size, latency, the input and output meters, and plugins that failed to load.",
        schema: || object(json!({}), &[]),
        ask: None,
        run: |host, _| Done::read("Read the audio engine", host.engine_report()),
    },
    Tool {
        name: "set_tempo",
        description: "Sets the project's tempo and, optionally, its time signature.",
        schema: || {
            object(
                json!({
                    "bpm": { "type": "number", "minimum": 20, "maximum": 400 },
                    "beats_per_bar": { "type": "integer", "minimum": 1, "maximum": 16 },
                    "beat_unit": { "type": "integer", "enum": [2, 4, 8, 16] },
                }),
                &["beats_per_bar", "beat_unit"],
            )
        },
        ask: None,
        run: |host, args| {
            let old = host.project().tempo.clone();
            let tempo = Tempo {
                bpm: args["bpm"].as_f64().unwrap_or(old.bpm),
                numerator: args["beats_per_bar"].as_u64().map_or(old.numerator, |n| n as u8),
                denominator: args["beat_unit"].as_u64().map_or(old.denominator, |n| n as u8),
            };
            let summary = format!(
                "Set the tempo to {} bpm in {}/{}",
                trim_number(tempo.bpm),
                tempo.numerator,
                tempo.denominator
            );
            host.apply(Command::SetTempo { tempo })?;
            Done::edit(summary)
        },
    },
    Tool {
        name: "add_track",
        description: "Adds an audio track (for guitar, bass or anything played in) at the end, with an empty pedal chain.",
        schema: || object(json!({ "name": { "type": "string", "minLength": 1 } }), &["name"]),
        ask: None,
        run: |host, args| {
            let project = host.project();
            let id = TrackId(project.fresh_id());
            let name = args["name"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map_or_else(|| format!("Track {}", project.tracks.len() + 1), str::to_string);
            host.apply(Command::InsertTrack {
                index: usize::MAX,
                track: Track::new(id, name.clone()),
            })?;
            Ok(Done {
                summary: format!("Added the track {name}"),
                result: json!({ "ok": true, "track": id.0 }),
                edited: true,
            })
        },
    },
    Tool {
        name: "add_drum_track",
        description: "Adds a drum track with a kit from list_kits (the first installed kit if none is given).",
        schema: || object(json!({ "kit": { "type": "string" } }), &["kit"]),
        ask: None,
        run: |host, args| {
            let id = host.add_drum_track(args["kit"].as_str().map(str::to_string))?;
            let name = track_name(host.project(), id);
            Ok(Done {
                summary: format!("Added the drum track {name}"),
                result: json!({ "ok": true, "track": id.0 }),
                edited: true,
            })
        },
    },
    Tool {
        name: "rename_track",
        description: "Renames a track.",
        schema: || object(json!({ "track": track_arg(), "name": { "type": "string", "minLength": 1 } }), &[]),
        ask: None,
        run: |host, args| {
            let id = track_ref(host.project(), &args["track"])?;
            let old = track_name(host.project(), id);
            let name = args["name"].as_str().unwrap_or_default().trim().to_string();
            if name.is_empty() {
                bail!("a track needs a name");
            }
            host.apply(Command::RenameTrack { id, name: name.clone() })?;
            Done::edit(format!("Renamed {old} to {name}"))
        },
    },
    Tool {
        name: "remove_track",
        description: "Removes a track with its clips and pedals. The player is asked first.",
        schema: || object(json!({ "track": track_arg() }), &[]),
        ask: Some(|host, args| {
            Some(match track_ref(host.project(), &args["track"]) {
                Ok(id) => format!("Remove the track {}, with its clips and pedals?", track_name(host.project(), id)),
                Err(_) => "Remove a track?".into(),
            })
        }),
        run: |host, args| {
            let id = track_ref(host.project(), &args["track"])?;
            let name = track_name(host.project(), id);
            host.remove_track(id)?;
            Done::edit(format!("Removed the track {name}"))
        },
    },
    Tool {
        name: "set_mix",
        description: "Sets a track's or the master's fader, pan, mute or solo. Give only what should change.",
        schema: || {
            object(
                json!({
                    "track": { "type": ["integer", "string"], "description": "A track's id or exact name, or \"master\"." },
                    "gain_db": { "type": "number", "minimum": -60, "maximum": 12 },
                    "pan": { "type": "number", "minimum": -1, "maximum": 1, "description": "-1 is hard left, 1 hard right." },
                    "mute": { "type": "boolean" },
                    "solo": { "type": "boolean" },
                }),
                &["gain_db", "pan", "mute", "solo"],
            )
        },
        ask: None,
        run: |host, args| {
            let master = args["track"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("master"));
            let track = if master { None } else { Some(track_ref(host.project(), &args["track"])?) };
            let who = track.map_or_else(|| "the master".to_string(), |t| track_name(host.project(), t));
            let mut commands = Vec::new();
            let mut said = Vec::new();
            if let Some(db) = args["gain_db"].as_f64() {
                commands.push(Command::SetGain { track, db: db as f32 });
                said.push(format!("fader to {} dB", trim_number(db)));
            }
            if let Some(on) = args["mute"].as_bool() {
                commands.push(Command::SetMute { track, on });
                said.push(if on { "muted" } else { "unmuted" }.to_string());
            }
            if let Some(track) = track {
                if let Some(pan) = args["pan"].as_f64() {
                    commands.push(Command::SetPan { track, pan: pan as f32 });
                    said.push(format!("pan to {}", trim_number(pan)));
                }
                if let Some(on) = args["solo"].as_bool() {
                    commands.push(Command::SetSolo { track, on });
                    said.push(if on { "soloed" } else { "unsoloed" }.to_string());
                }
            } else if !args["pan"].is_null() || !args["solo"].is_null() {
                bail!("the master has no pan or solo");
            }
            if commands.is_empty() {
                bail!("give at least one of gain_db, pan, mute or solo");
            }
            host.apply(batch(commands))?;
            Done::edit(format!("Set {who}: {}", said.join(", ")))
        },
    },
    Tool {
        name: "arm_track",
        description: "Arms a track to record and to play its input through its chain, or disarms it.",
        schema: || object(json!({ "track": track_arg(), "armed": { "type": "boolean" } }), &[]),
        ask: None,
        run: |host, args| {
            let id = track_ref(host.project(), &args["track"])?;
            let armed = args["armed"].as_bool().unwrap_or(true);
            host.apply(Command::ArmTrack { id, armed })?;
            let name = track_name(host.project(), id);
            Done::edit(format!("{} {name}", if armed { "Armed" } else { "Disarmed" }))
        },
    },
    Tool {
        name: "add_pedal",
        description: "Adds a pedal to a track's chain, at the end or at a position (0 is first, nearest the input). Kinds come from list_pedal_kinds. Optionally sets parameters by name in the same step.",
        schema: || {
            object(
                json!({
                    "track": track_arg(),
                    "kind": { "type": "string", "description": "A kind from list_pedal_kinds, such as \"overdrive\"." },
                    "position": { "type": "integer", "minimum": 0 },
                    "params": {
                        "type": "object",
                        "description": "Parameter values by name, in the parameter's unit; a choice by its name.",
                        "additionalProperties": { "type": ["number", "string", "boolean"] },
                    },
                }),
                &["position", "params"],
            )
        },
        ask: None,
        run: |host, args| {
            let track = track_ref(host.project(), &args["track"])?;
            let spec = args["kind"].as_str().unwrap_or_default().trim().to_string();
            let (name, params) = host
                .pedal_kind(&spec)
                .with_context(|| format!("{spec:?} is not a pedal kind; call list_pedal_kinds"))?;
            // Check the values before adding anything, so a mistake adds nothing.
            let mut values: Vec<f32> = params.iter().map(|p| p.default).collect();
            let mut said = Vec::new();
            if let Some(given) = args["params"].as_object() {
                for (key, value) in given {
                    let (i, info) = param_ref(&name, &params, key)?;
                    values[i] = param_value(info, value)?;
                    said.push((i, format!("{} {}", info.name, show_value(info, values[i]))));
                }
            }
            let index = args["position"].as_u64().map(|p| p as usize);
            let id = host.add_pedal(track, &spec, index, Some(values))?;
            let track_name = track_name(host.project(), track);
            said.sort();
            let said: Vec<String> = said.into_iter().map(|(_, s)| s).collect();
            let with = if said.is_empty() { String::new() } else { format!(" ({})", said.join(", ")) };
            Ok(Done {
                summary: format!("Added {name} to {track_name}{with}"),
                result: json!({ "ok": true, "pedal": id.0 }),
                edited: true,
            })
        },
    },
    Tool {
        name: "remove_pedal",
        description: "Removes a pedal from its chain.",
        schema: || object(json!({ "pedal": { "type": "integer", "description": "The pedal's id." } }), &[]),
        ask: None,
        run: |host, args| {
            let (id, name, track) = pedal_ref(host, &args["pedal"])?;
            host.apply(Command::RemovePedal { id })?;
            Done::edit(format!("Removed {name} from {track}"))
        },
    },
    Tool {
        name: "move_pedal",
        description: "Moves a pedal to a position in its chain: 0 is first, nearest the input.",
        schema: || {
            object(
                json!({ "pedal": { "type": "integer" }, "position": { "type": "integer", "minimum": 0 } }),
                &[],
            )
        },
        ask: None,
        run: |host, args| {
            let (id, name, track) = pedal_ref(host, &args["pedal"])?;
            let to = args["position"].as_u64().unwrap_or(0) as usize;
            host.apply(Command::MovePedal { id, to })?;
            Done::edit(format!("Moved {name} to position {} on {track}", to + 1))
        },
    },
    Tool {
        name: "set_pedal_params",
        description: "Sets one or more of a pedal's parameters by name, in the parameter's unit (dB, Hz, ms, %, or 0 to 1); a choice by its name, a switch by true or false. Values outside the range are held to it.",
        schema: || {
            object(
                json!({
                    "pedal": { "type": "integer" },
                    "params": {
                        "type": "object",
                        "minProperties": 1,
                        "additionalProperties": { "type": ["number", "string", "boolean"] },
                    },
                }),
                &[],
            )
        },
        ask: None,
        run: |host, args| {
            let (id, name, track) = pedal_ref(host, &args["pedal"])?;
            let pedal = host.project().pedal(id).map(|(_, p)| p.clone()).context("no such pedal")?;
            let (_, params) = host.pedal_kind(&pedal.spec)?;
            let mut values = pedal.values.clone();
            values.resize(params.len(), 0.0);
            for (i, p) in params.iter().enumerate() {
                if pedal.values.get(i).is_none() {
                    values[i] = p.default;
                }
            }
            let mut said = Vec::new();
            for (key, value) in args["params"].as_object().into_iter().flatten() {
                let (i, info) = param_ref(&name, &params, key)?;
                values[i] = param_value(info, value)?;
                said.push((i, format!("{} to {}", info.name, show_value(info, values[i]))));
            }
            host.apply(Command::SetPedalValues { id, values })?;
            said.sort();
            let said: Vec<String> = said.into_iter().map(|(_, s)| s).collect();
            Done::edit(format!("Set {name}'s {} on {track}", said.join(", ")))
        },
    },
    Tool {
        name: "bypass_pedal",
        description: "Bypasses a pedal, or turns it back on.",
        schema: || object(json!({ "pedal": { "type": "integer" }, "bypassed": { "type": "boolean" } }), &[]),
        ask: None,
        run: |host, args| {
            let (id, name, track) = pedal_ref(host, &args["pedal"])?;
            let on = args["bypassed"].as_bool().unwrap_or(true);
            host.apply(Command::SetBypass { pedal: id, on })?;
            Done::edit(format!("{} {name} on {track}", if on { "Bypassed" } else { "Turned on" }))
        },
    },
    Tool {
        name: "add_pattern",
        description: "Adds a drum pattern. Rows are by pad id, from list_kits or the drum track's kit; every row must have the pattern's length in steps.",
        schema: || {
            object(
                json!({
                    "name": { "type": "string", "minLength": 1 },
                    "steps_per_beat": { "type": "integer", "enum": [1, 2, 3, 4, 6, 8], "description": "4 for sixteenths, 2 for eighths, 3 or 6 for triplets." },
                    "length": { "type": "integer", "minimum": 1, "maximum": MAX_PATTERN_STEPS },
                    "swing": { "type": "number", "minimum": 0, "maximum": 1 },
                    "rows": { "type": "object", "description": STEPS, "additionalProperties": { "type": "string" } },
                }),
                &["steps_per_beat", "swing", "rows"],
            )
        },
        ask: None,
        run: |host, args| {
            let project = host.project();
            let id = PatternId(project.fresh_id());
            let mut pattern = Pattern::new(id, args["name"].as_str().unwrap_or("Pattern").trim());
            if let Some(n) = args["steps_per_beat"].as_u64() {
                pattern.steps_per_beat = n as u8;
            }
            pattern.length = args["length"].as_u64().unwrap_or(16) as u16;
            pattern.swing = args["swing"].as_f64().unwrap_or(0.0) as f32;
            pattern.rows = rows(&args["rows"], pattern.length)?;
            pattern.validate().map_err(|e| anyhow!(e))?;
            let name = pattern.name.clone();
            host.apply(Command::InsertPattern {
                index: usize::MAX,
                pattern,
            })?;
            Ok(Done {
                summary: format!("Added the pattern {name}"),
                result: json!({ "ok": true, "pattern": id.0 }),
                edited: true,
            })
        },
    },
    Tool {
        name: "set_pattern",
        description: "Rewrites a drum pattern's rows, and optionally its length, step size and swing. Rows not given are left as they are; a row of only rests removes it.",
        schema: || {
            object(
                json!({
                    "pattern": { "type": "integer" },
                    "rows": { "type": "object", "description": STEPS, "additionalProperties": { "type": "string" } },
                    "length": { "type": "integer", "minimum": 1, "maximum": MAX_PATTERN_STEPS },
                    "steps_per_beat": { "type": "integer", "enum": [1, 2, 3, 4, 6, 8] },
                    "swing": { "type": "number", "minimum": 0, "maximum": 1 },
                }),
                &["rows", "length", "steps_per_beat", "swing"],
            )
        },
        ask: None,
        run: |host, args| {
            let mut pattern = pattern_ref(host.project(), &args["pattern"])?.clone();
            if let Some(n) = args["steps_per_beat"].as_u64() {
                pattern.steps_per_beat = n as u8;
            }
            if let Some(s) = args["swing"].as_f64() {
                pattern.swing = s as f32;
            }
            if let Some(length) = args["length"].as_u64() {
                pattern.length = length as u16;
                for row in &mut pattern.rows {
                    row.steps.resize(length as usize, 0.0);
                }
            }
            for row in rows(&args["rows"], pattern.length)? {
                pattern.rows.retain(|r| r.pad != row.pad);
                if row.steps.iter().any(|v| *v > 0.0) {
                    pattern.rows.push(row);
                }
            }
            pattern.validate().map_err(|e| anyhow!(e))?;
            let name = pattern.name.clone();
            host.apply(Command::ReplacePattern { pattern })?;
            Done::edit(format!("Rewrote the pattern {name}"))
        },
    },
    Tool {
        name: "delete_pattern",
        description: "Deletes a drum pattern and every placement of it on the timeline. The player is asked first.",
        schema: || object(json!({ "pattern": { "type": "integer" } }), &[]),
        ask: Some(|host, args| {
            Some(match pattern_ref(host.project(), &args["pattern"]) {
                Ok(p) => format!("Delete the pattern {}, and everywhere it is placed?", p.name),
                Err(_) => "Delete a pattern?".into(),
            })
        }),
        run: |host, args| {
            let pattern = pattern_ref(host.project(), &args["pattern"])?;
            let (id, name) = (pattern.id, pattern.name.clone());
            host.apply(Command::RemovePattern { id })?;
            Done::edit(format!("Deleted the pattern {name}"))
        },
    },
    Tool {
        name: "place_pattern",
        description: "Places a drum pattern on a drum track's timeline at a bar (1 is the first), playing for a number of bars (its own length if not given; it loops to fill).",
        schema: || {
            object(
                json!({
                    "pattern": { "type": "integer" },
                    "track": track_arg(),
                    "bar": { "type": "number", "minimum": 1 },
                    "bars": { "type": "number", "exclusiveMinimum": 0 },
                }),
                &["bars"],
            )
        },
        ask: None,
        run: |host, args| {
            let project = host.project();
            let pattern = pattern_ref(project, &args["pattern"])?;
            let track = track_ref(project, &args["track"])?;
            if project.track(track).is_some_and(|t| !t.is_drums()) {
                bail!("{} is not a drum track", track_name(project, track));
            }
            let bar = bar_ticks(&project.tempo);
            let start = ((args["bar"].as_f64().unwrap_or(1.0) - 1.0).max(0.0) * bar as f64).round() as u64;
            let length = args["bars"]
                .as_f64()
                .map_or_else(|| pattern.ticks(), |b| (b * bar as f64).round() as u64)
                .max(1);
            let clip = PatternClip {
                id: PatternClipId(project.fresh_id()),
                pattern: pattern.id,
                start,
                offset: 0,
                length,
                fill: None,
            };
            let summary = format!(
                "Placed {} on {} at bar {}",
                pattern.name,
                track_name(project, track),
                trim_number(args["bar"].as_f64().unwrap_or(1.0))
            );
            host.apply(Command::InsertPatternClip { track, clip })?;
            Done::edit(summary)
        },
    },
    Tool {
        name: "delete_clip",
        description: "Deletes a recorded clip or a placed pattern from the timeline, by its id from get_project. The audio file is kept. The player is asked first.",
        schema: || object(json!({ "clip": { "type": "integer" } }), &[]),
        ask: Some(|host, args| {
            Some(match clip_ref(host.project(), &args["clip"]) {
                Ok(ClipRef::Audio(c, t)) => format!("Delete the clip {} from {t}?", c.name),
                Ok(ClipRef::Pattern(_, t)) => format!("Delete the placed pattern from {t}?"),
                Err(_) => "Delete a clip?".into(),
            })
        }),
        run: |host, args| {
            match clip_ref(host.project(), &args["clip"])? {
                ClipRef::Audio(c, t) => {
                    let summary = format!("Deleted the clip {} from {t}", c.name);
                    host.apply(Command::RemoveClip { id: c.id })?;
                    Done::edit(summary)
                }
                ClipRef::Pattern(c, t) => {
                    host.apply(Command::RemovePatternClip { id: c.id })?;
                    Done::edit(format!("Deleted a placed pattern from {t}"))
                }
            }
        },
    },
    Tool {
        name: "set_loop",
        description: "Sets the loop to bars start_bar up to (not including) end_bar, 1 being the first, and turns looping on or off; or clears it with clear true.",
        schema: || {
            object(
                json!({
                    "start_bar": { "type": "number", "minimum": 1 },
                    "end_bar": { "type": "number", "minimum": 1 },
                    "on": { "type": "boolean" },
                    "clear": { "type": "boolean" },
                }),
                &["start_bar", "end_bar", "on", "clear"],
            )
        },
        ask: None,
        run: |host, args| {
            if args["clear"].as_bool() == Some(true) {
                host.apply(Command::SetLoop { region: None })?;
                return Done::edit("Cleared the loop");
            }
            let project = host.project();
            let bar = bar_samples(project);
            let (Some(a), Some(b)) = (args["start_bar"].as_f64(), args["end_bar"].as_f64()) else {
                bail!("give start_bar and end_bar, or clear");
            };
            if b <= a {
                bail!("end_bar must be after start_bar");
            }
            let on = args["on"].as_bool().unwrap_or(true);
            let region = LoopRegion {
                start: ((a - 1.0) * bar).round() as u64,
                end: ((b - 1.0) * bar).round() as u64,
                on,
            };
            host.apply(Command::SetLoop { region: Some(region) })?;
            Done::edit(format!(
                "Looped bars {} to {}{}",
                trim_number(a),
                trim_number(b),
                if on { "" } else { " (off)" }
            ))
        },
    },
    Tool {
        name: "write_pedal",
        description: "Writes a new pedal in the pedal format, checks it and renders it over a guitar DI as cantina-cli does, and saves it to the player's library (~/Music/Cantina/Pedals/<id>.pedal.json), where it appears in the pedal picker under Your pedals. If it fails, nothing is saved and you get the problems to fix. Optionally adds it to a track.",
        schema: || {
            object(
                json!({
                    "id": { "type": "string", "description": "The file's id: lower case letters, digits and dashes, such as \"warm-drive\"." },
                    "definition": { "type": "object", "description": "The pedal, in the pedal format." },
                    "replace": { "type": "boolean", "description": "Overwrite a library pedal with this id. The player is asked first." },
                    "add_to_track": { "type": ["integer", "string"], "description": "A track to add it to once saved." },
                }),
                &["replace", "add_to_track"],
            )
        },
        ask: Some(|host, args| replace_question(host, Kind::Pedal, args)),
        run: |host, args| write_file(host, Kind::Pedal, args),
    },
    Tool {
        name: "write_pattern",
        description: "Writes a drum pattern in the pattern format, checks it (against a kit's pads when one is given or the track has one), plays it through that kit to make sure it sounds, and saves it to ~/Music/Cantina/Patterns/<name>.pattern.json, where it is offered under Import from the library. Optionally adds it to the project for a drum track; then place it with place_pattern.",
        schema: || {
            object(
                json!({
                    "name": { "type": "string", "minLength": 1, "description": "The file's name, such as \"Blues shuffle\"." },
                    "definition": { "type": "object", "description": "The pattern, in the pattern format." },
                    "kit": { "type": "string", "description": "A kit id from list_kits to check the pads against and play it on." },
                    "replace": { "type": "boolean", "description": "Overwrite a library pattern with this name. The player is asked first." },
                    "add_to_track": { "type": ["integer", "string"], "description": "A drum track whose project gets the pattern, checked against its kit." },
                }),
                &["kit", "replace", "add_to_track"],
            )
        },
        ask: Some(|host, args| replace_question(host, Kind::Pattern, args)),
        run: |host, args| write_file(host, Kind::Pattern, args),
    },
    Tool {
        name: "write_kit_recipe",
        description: "Writes a drum kit recipe in the kit-recipe format, checks it, synthesises the kit from it as cantina-cli synth-kit does, and installs it in ~/Music/Cantina/Kits/<id>, where drum tracks can pick it. If it fails, nothing is installed. Optionally adds a drum track that uses it.",
        schema: || {
            object(
                json!({
                    "id": { "type": "string", "description": "The kit's folder: lower case letters, digits and dashes." },
                    "recipe": { "type": "object", "description": "The recipe, in the kit-recipe format." },
                    "add_drum_track": { "type": "boolean" },
                }),
                &["add_drum_track"],
            )
        },
        ask: None,
        run: |host, args| write_file(host, Kind::KitRecipe, args),
    },
    Tool {
        name: "search_docs",
        description: "Searches Cantina's own guides and reference pages, the docs of this build. Returns the best pages with their address and the passage that matched. Use it to ground answers about the app, and link the page.",
        schema: || object(json!({ "query": { "type": "string", "minLength": 2 } }), &[]),
        ask: None,
        run: |_, args| {
            let query = args["query"].as_str().unwrap_or_default();
            let found = super::docs::search(query, 5);
            Done::read(format!("Searched the docs for \"{}\"", query.trim()), json!({ "results": found }))
        },
    },
    Tool {
        name: "read_docs",
        description: "Reads one docs page in full by its page (such as \"guides/latency\") or address, with the address of each section to link.",
        schema: || object(json!({ "page": { "type": "string" } }), &[]),
        ask: None,
        run: |_, args| {
            let page = args["page"].as_str().unwrap_or_default();
            let found = super::docs::read(page)
                .with_context(|| format!("no docs page {page:?}; use search_docs to find one"))?;
            Done::read(format!("Read the guide {}", found["title"].as_str().unwrap_or(page)), found)
        },
    },
    Tool {
        name: "apply_chain",
        description: "Builds a suggested pedal chain on a track as one undoable step: these pedals in order from the input, with their settings, in place of the track's chain (or after it with replace false). Use it only once the player has said to apply your suggestion; they are asked to confirm.",
        schema: || {
            object(
                json!({
                    "track": track_arg(),
                    "pedals": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 12,
                        "items": object(
                            json!({
                                "kind": { "type": "string" },
                                "params": { "type": "object", "additionalProperties": { "type": ["number", "string", "boolean"] } },
                            }),
                            &["params"],
                        ),
                    },
                    "replace": { "type": "boolean", "description": "Replace the track's chain (the default), or add after it." },
                }),
                &["replace"],
            )
        },
        ask: Some(|host, args| {
            let project = host.project();
            let Ok(track) = track_ref(project, &args["track"]) else {
                return Some("Apply the suggested chain?".into());
            };
            let names: Vec<String> = args["pedals"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    let kind = p["kind"].as_str().unwrap_or("?");
                    host.pedal_kinds()
                        .into_iter()
                        .find(|(spec, _)| spec == kind)
                        .map_or_else(|| kind.to_string(), |(_, name)| name)
                })
                .collect();
            let replacing = args["replace"].as_bool() != Some(false)
                && project.track(track).is_some_and(|t| !t.chain.is_empty());
            Some(format!(
                "Set {}'s chain to {}{}?",
                track_name(project, track),
                names.join(" → "),
                if replacing { ", replacing what is there" } else { "" }
            ))
        }),
        run: |host, args| {
            let track = track_ref(host.project(), &args["track"])?;
            let mut pedals = Vec::new();
            let mut names = Vec::new();
            for (n, p) in args["pedals"].as_array().into_iter().flatten().enumerate() {
                let spec = p["kind"].as_str().unwrap_or_default().trim().to_string();
                let (name, params) = host
                    .pedal_kind(&spec)
                    .with_context(|| format!("pedal {}: {spec:?} is not a pedal kind; call list_pedal_kinds", n + 1))?;
                let mut values: Vec<f32> = params.iter().map(|q| q.default).collect();
                for (key, value) in p["params"].as_object().into_iter().flatten() {
                    let (i, info) = param_ref(&name, &params, key)?;
                    values[i] = param_value(info, value)?;
                }
                names.push(name);
                pedals.push((spec, values));
            }
            let replace = args["replace"].as_bool() != Some(false);
            let ids = host.set_chain(track, pedals, replace)?;
            Ok(Done {
                summary: format!("Set {}'s chain to {}", track_name(host.project(), track), names.join(" → ")),
                result: json!({ "ok": true, "pedals": ids.iter().map(|i| i.0).collect::<Vec<_>>() }),
                edited: true,
            })
        },
    },
];

/// The model's tool definitions, from the table.
pub fn specs() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": { "name": t.name, "description": t.description, "parameters": (t.schema)() },
            })
        })
        .collect()
}

fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The compiled schemas, built once.
fn validators() -> &'static Vec<(&'static str, jsonschema::Validator)> {
    static V: OnceLock<Vec<(&'static str, jsonschema::Validator)>> = OnceLock::new();
    V.get_or_init(|| {
        TOOLS
            .iter()
            .map(|t| {
                let v =
                    jsonschema::validator_for(&(t.schema)()).expect("every tool's schema compiles");
                (t.name, v)
            })
            .collect()
    })
}

/// The question to ask before running a destructive tool.
pub fn confirm(host: &dyn Host, name: &str, args: &Value) -> Option<String> {
    find(name)?.ask.and_then(|ask| ask(host, args))
}

/// Runs a tool: checks its arguments, runs it, and turns what came of
/// it into an outcome. Any failure goes back to the model as an error to
/// recover from.
pub fn run(host: &mut dyn Host, name: &str, args: &Value) -> Outcome {
    let Some(tool) = find(name) else {
        return Outcome::error(
            format!("Asked for {name}, which does not exist"),
            format!("there is no tool called {name}"),
        );
    };
    if let Some((_, validator)) = validators().iter().find(|(n, _)| *n == name) {
        let problems: Vec<String> = validator
            .iter_errors(args)
            .map(|e| {
                let at = e.instance_path().to_string();
                if at.is_empty() {
                    e.to_string()
                } else {
                    format!("{at}: {e}")
                }
            })
            .collect();
        if !problems.is_empty() {
            return Outcome::error(
                format!("Could not {}: the request was malformed", human(name)),
                format!(
                    "the arguments do not match the schema: {}",
                    problems.join("; ")
                ),
            );
        }
    }
    let before = host.revision();
    match (tool.run)(host, args) {
        Ok(done) => Outcome {
            summary: done.summary,
            result: done.result,
            ok: true,
            undo: (done.edited && host.revision() != before).then(|| host.revision()),
        },
        Err(e) => Outcome::error(
            format!("Could not {}: {e:#}", human(name)),
            format!("{e:#}"),
        ),
    }
}

/// A tool's name as words: `add_pedal` → "add pedal".
fn human(name: &str) -> String {
    name.replace('_', " ")
}

/// The question before an authoring tool overwrites a library file:
/// only when asked to replace one that is there.
fn replace_question(host: &dyn Host, kind: Kind, args: &Value) -> Option<String> {
    let stem = args[if kind == Kind::Pattern { "name" } else { "id" }]
        .as_str()?
        .trim();
    let exists = author::destination(kind, stem, &host.library()).exists();
    (args["replace"].as_bool() == Some(true) && exists).then(|| {
        format!(
            "Replace your {} {stem:?} in the library with the new one?",
            if kind == Kind::Pattern {
                "pattern"
            } else {
                "pedal"
            }
        )
    })
}

/// Writes, checks and saves a pedal, pattern or kit, then loads it if
/// asked to.
fn write_file(host: &mut dyn Host, kind: Kind, args: &Value) -> Result<Done> {
    let (stem, content) = match kind {
        Kind::Pattern => (
            args["name"].as_str().unwrap_or_default().trim(),
            &args["definition"],
        ),
        Kind::Pedal => (
            args["id"].as_str().unwrap_or_default().trim(),
            &args["definition"],
        ),
        Kind::KitRecipe => (
            args["id"].as_str().unwrap_or_default().trim(),
            &args["recipe"],
        ),
    };
    let stem = stem.to_string();
    let track = match &args["add_to_track"] {
        Value::Null => None,
        v => Some(track_ref(host.project(), v)?),
    };
    let kit = match (kind, args["kit"].as_str(), track) {
        (Kind::Pattern, Some(kit), _) => Some(kit.to_string()),
        (Kind::Pattern, None, Some(t)) => {
            let kit = host.project().track(t).and_then(|t| t.kit.clone());
            if kit.is_none() {
                bail!("{} is not a drum track", track_name(host.project(), t));
            }
            kit
        }
        _ => None,
    };
    let kit_dir = match &kit {
        Some(k) => Some(
            host.kit_dir(k)
                .with_context(|| format!("no kit {k:?} is installed; call list_kits"))?,
        ),
        None => None,
    };
    let replace = args["replace"].as_bool() == Some(true);
    let written = author::write(
        kind,
        &stem,
        content,
        &host.library(),
        kit_dir.as_deref(),
        replace,
    )
    .map_err(|problems| anyhow!("not saved, because:\n- {}", problems.join("\n- ")))?;
    host.rescan_library();
    let mut result = written.result();
    let level = written
        .stats
        .as_ref()
        .map(|s| format!(", peak {:.1} dBFS", s.peak_db))
        .unwrap_or_default();
    let mut summary = match kind {
        Kind::Pedal => format!("Wrote the pedal {}{level}", written.name),
        Kind::Pattern => format!("Wrote the pattern {}", written.name),
        Kind::KitRecipe => format!("Built the kit {}", written.name),
    };
    let mut edited = false;
    match kind {
        Kind::Pedal => {
            if let Some(track) = track {
                let id = host.add_pedal(track, &format!("pedal:{stem}"), None, None)?;
                result["pedal"] = json!(id.0);
                summary += &format!(" and added it to {}", track_name(host.project(), track));
                edited = true;
            }
        }
        Kind::Pattern => {
            if let Some(track) = track {
                let id = host.import_pattern(&written.path, Some(track))?;
                result["pattern"] = json!(id.0);
                summary += " and added it to the project";
                edited = true;
            }
        }
        Kind::KitRecipe => {
            if args["add_drum_track"].as_bool() == Some(true) {
                let id = host.add_drum_track(Some(stem.clone()))?;
                result["track"] = json!(id.0);
                summary += &format!(
                    " and added the drum track {}",
                    track_name(host.project(), id)
                );
                edited = true;
            }
        }
    }
    Ok(Done {
        summary,
        result,
        edited,
    })
}

/// One command, or a batch of several as one undo step.
fn batch(mut commands: Vec<Command>) -> Command {
    if commands.len() == 1 {
        commands.remove(0)
    } else {
        Command::Batch { commands }
    }
}

/// A number without a pointless `.0`.
fn trim_number(x: f64) -> String {
    let s = format!("{:.2}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn track_name(project: &Project, id: TrackId) -> String {
    project
        .track(id)
        .map_or_else(|| format!("track {id}"), |t| t.name.clone())
}

/// A track by id, or by exact (case-insensitive) name.
fn track_ref(project: &Project, value: &Value) -> Result<TrackId> {
    if let Some(id) = value.as_u64() {
        return project
            .track(TrackId(id))
            .map(|t| t.id)
            .with_context(|| format!("there is no track {id}; call get_project for the ids"));
    }
    let name = value
        .as_str()
        .context("give the track's id or name")?
        .trim();
    if let Ok(id) = name.parse::<u64>() {
        return track_ref(project, &json!(id));
    }
    let found: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|t| t.name.eq_ignore_ascii_case(name))
        .collect();
    match found.as_slice() {
        [t] => Ok(t.id),
        [] => bail!("there is no track called {name:?}; call get_project for the tracks"),
        _ => bail!("more than one track is called {name:?}; use its id"),
    }
}

fn pattern_ref<'a>(project: &'a Project, value: &Value) -> Result<&'a Pattern> {
    let id = value.as_u64().context("give the pattern's id")?;
    project
        .pattern(PatternId(id))
        .with_context(|| format!("there is no pattern {id}; call get_project for the ids"))
}

/// A pedal by id, with its kind's name and its track's name.
fn pedal_ref(host: &mut dyn Host, value: &Value) -> Result<(PedalId, String, String)> {
    let id = PedalId(value.as_u64().context("give the pedal's id")?);
    let (track, pedal) = host
        .project()
        .pedal(id)
        .with_context(|| format!("there is no pedal {id}; call get_project for the ids"))?;
    let (track, spec) = (track.name.clone(), pedal.spec.clone());
    let name = host.pedal_kind(&spec).map_or(spec, |(n, _)| n);
    Ok((id, name, track))
}

enum ClipRef {
    Audio(Clip, String),
    Pattern(PatternClip, String),
}

fn clip_ref(project: &Project, value: &Value) -> Result<ClipRef> {
    let id = value.as_u64().context("give the clip's id")?;
    for track in &project.tracks {
        if let Some(c) = track.clips.iter().find(|c| c.id == ClipId(id)) {
            return Ok(ClipRef::Audio(c.clone(), track.name.clone()));
        }
        if let Some(c) = track
            .pattern_clips
            .iter()
            .find(|c| c.id == PatternClipId(id))
        {
            return Ok(ClipRef::Pattern(c.clone(), track.name.clone()));
        }
    }
    bail!("there is no clip {id}; call get_project for the ids")
}

/// A parameter by name, case and spacing aside, or by index.
fn param_ref<'a>(
    pedal: &str,
    params: &'a [ParamInfo],
    key: &str,
) -> Result<(usize, &'a ParamInfo)> {
    let squash = |s: &str| s.to_lowercase().replace([' ', '_', '-'], "");
    if let Some((i, p)) = params
        .iter()
        .enumerate()
        .find(|(_, p)| squash(&p.name) == squash(key))
    {
        return Ok((i, p));
    }
    if let Some(p) = key
        .parse::<usize>()
        .ok()
        .and_then(|i| params.get(i).map(|p| (i, p)))
    {
        return Ok(p);
    }
    let names: Vec<&str> = params.iter().map(|p| p.name.as_ref()).collect();
    bail!(
        "{pedal} has no parameter {key:?}; it has {}",
        names.join(", ")
    )
}

/// A value the model gave, in the parameter's terms, held to its range.
fn param_value(info: &ParamInfo, value: &Value) -> Result<f32> {
    let raw = match (value, info.unit) {
        (Value::Bool(b), _) => f32::from(u8::from(*b)),
        (Value::String(s), Unit::Choice(names)) => names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(s.trim()))
            .with_context(|| format!("{} is one of {}", info.name, names.join(", ")))?
            as f32,
        (Value::String(s), Unit::Toggle) => match s.to_lowercase().as_str() {
            "on" | "true" | "yes" => 1.0,
            "off" | "false" | "no" => 0.0,
            _ => bail!("{} is on or off", info.name),
        },
        (Value::String(s), _) => s
            .trim()
            .trim_end_matches(|c: char| c.is_alphabetic() || c == '%' || c == ' ')
            .parse::<f32>()
            .with_context(|| format!("{} takes a number", info.name))?,
        (v, _) => v
            .as_f64()
            .with_context(|| format!("{} takes a number", info.name))? as f32,
    };
    if !raw.is_finite() {
        bail!("{} takes a finite number", info.name);
    }
    Ok(info.clamp(raw))
}

/// A value as the card shows it: "-50 dB", "Fuzz", "on".
fn show_value(info: &ParamInfo, value: f32) -> String {
    match info.unit {
        Unit::Choice(names) => names
            .get(value as usize)
            .map_or_else(|| value.to_string(), |n| n.to_string()),
        Unit::Toggle => if value >= 0.5 { "on" } else { "off" }.into(),
        Unit::Decibels => format!("{} dB", trim_number(value.into())),
        Unit::Hertz => format!("{} Hz", trim_number(value.into())),
        Unit::Seconds => format!("{} s", trim_number(value.into())),
        Unit::Milliseconds => format!("{} ms", trim_number(value.into())),
        Unit::Percent => format!("{}%", trim_number(value.into())),
        Unit::Generic => trim_number(value.into()),
    }
}

/// Steps as letters: X hard, x normal, o ghost, . rest.
pub fn steps_text(steps: &[f32]) -> String {
    steps
        .iter()
        .map(|v| match *v {
            v if v <= 0.0 => '.',
            v if v < 0.55 => 'o',
            v if v < 0.95 => 'x',
            _ => 'X',
        })
        .collect()
}

/// Letters as steps; spaces and bars are ignored.
pub fn parse_steps(text: &str) -> Result<Vec<f32>> {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '|')
        .map(|c| match c {
            'X' => Ok(1.0),
            'x' => Ok(0.8),
            'o' | 'O' => Ok(0.4),
            '.' | '-' | '_' => Ok(0.0),
            other => Err(anyhow!("{other:?} is not a step; use X, x, o or .")),
        })
        .collect()
}

fn rows(value: &Value, length: u16) -> Result<Vec<PatternRow>> {
    let mut out = Vec::new();
    for (pad, text) in value.as_object().into_iter().flatten() {
        let text = text
            .as_str()
            .with_context(|| format!("the row for {pad} should be step letters"))?;
        let steps = parse_steps(text).with_context(|| format!("the row for {pad}"))?;
        if steps.len() != length as usize {
            bail!(
                "the row for {pad} has {} steps but the pattern has {length}",
                steps.len()
            );
        }
        out.push(PatternRow {
            pad: pad.clone(),
            steps,
            ..Default::default()
        });
    }
    Ok(out)
}

/// A bar in timeline ticks.
fn bar_ticks(tempo: &Tempo) -> u64 {
    u64::from(tempo.numerator.max(1)) * u64::from(PPQ) * 4 / u64::from(tempo.denominator.max(1))
}

/// A bar in samples at the project's tempo.
fn bar_samples(project: &Project) -> f64 {
    let quarter = 60.0 / project.tempo.bpm.max(1.0) * project.sample_rate;
    bar_ticks(&project.tempo) as f64 / f64::from(PPQ) * quarter
}

/// The project as the model reads it.
pub fn describe(host: &mut dyn Host) -> Value {
    let project = host.project().clone();
    let quarter = 60.0 / project.tempo.bpm.max(1.0);
    let bar_s = bar_samples(&project);
    let seconds = |samples: u64| (samples as f64 / project.sample_rate * 100.0).round() / 100.0;
    let ticks_bar = bar_ticks(&project.tempo) as f64;
    let bar_of_ticks = |t: u64| (t as f64 / ticks_bar * 100.0).round() / 100.0 + 1.0;
    let tracks: Vec<Value> = project
        .tracks
        .iter()
        .map(|t| {
            let chain: Vec<Value> = t
                .chain
                .iter()
                .enumerate()
                .map(|(position, p)| {
                    let (name, params) = host
                        .pedal_kind(&p.spec)
                        .unwrap_or((p.spec.clone(), Vec::new()));
                    let params: Vec<Value> = params
                        .iter()
                        .enumerate()
                        .map(|(i, info)| {
                            let v = p.values.get(i).copied().unwrap_or(info.default);
                            let mut out = json!({
                                "name": info.name,
                                "value": show_value(info, v),
                                "min": info.min,
                                "max": info.max,
                            });
                            if let Unit::Choice(names) = info.unit {
                                out["choices"] = json!(names);
                            }
                            out
                        })
                        .collect();
                    json!({
                        "pedal": p.id.0,
                        "position": position,
                        "kind": p.spec,
                        "name": name,
                        "bypassed": p.bypassed,
                        "params": params,
                    })
                })
                .collect();
            json!({
                "track": t.id.0,
                "name": t.name,
                "type": if t.is_drums() { "drums" } else { "audio" },
                "kit": t.kit,
                "armed": t.armed,
                "gainDb": t.strip.gain_db,
                "pan": t.strip.pan,
                "mute": t.strip.mute,
                "solo": t.strip.solo,
                "chain": chain,
                "clips": t.clips.iter().map(|c| json!({
                    "clip": c.id.0,
                    "name": c.name,
                    "startBar": (c.start as f64 / bar_s * 100.0).round() / 100.0 + 1.0,
                    "seconds": seconds(c.length),
                })).collect::<Vec<_>>(),
                "patterns": t.pattern_clips.iter().map(|c| json!({
                    "clip": c.id.0,
                    "pattern": c.pattern.0,
                    "startBar": bar_of_ticks(c.start),
                    "bars": (c.length as f64 / ticks_bar * 100.0).round() / 100.0,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "name": project.name,
        "sampleRate": project.sample_rate,
        "tempo": { "bpm": project.tempo.bpm, "timeSignature": format!("{}/{}", project.tempo.numerator, project.tempo.denominator), "secondsPerBeat": quarter },
        "loop": project.loop_region.map(|l| json!({
            "startBar": (l.start as f64 / bar_s * 100.0).round() / 100.0 + 1.0,
            "endBar": (l.end as f64 / bar_s * 100.0).round() / 100.0 + 1.0,
            "on": l.on,
        })),
        "master": { "gainDb": project.master.gain_db, "mute": project.master.mute },
        "tracks": tracks,
        "patterns": project.patterns.iter().map(|p| json!({
            "pattern": p.id.0,
            "name": p.name,
            "stepsPerBeat": p.steps_per_beat,
            "length": p.length,
            "swing": p.swing,
            "pads": p.rows.iter().map(|r| r.pad.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// A few lines about the project for the system prompt, so the model
/// knows what is there before it reads the details.
pub fn brief(project: &Project) -> String {
    let tracks: Vec<String> = project
        .tracks
        .iter()
        .map(|t| {
            format!(
                "{} (id {}, {}, {} pedal{})",
                t.name,
                t.id,
                if t.is_drums() { "drums" } else { "audio" },
                t.chain.len(),
                if t.chain.len() == 1 { "" } else { "s" }
            )
        })
        .collect();
    format!(
        "The open project is \"{}\" at {} bpm in {}/{}, with {} track{}: {}. It has {} drum pattern{}.",
        project.name,
        trim_number(project.tempo.bpm),
        project.tempo.numerator,
        project.tempo.denominator,
        project.tracks.len(),
        if project.tracks.len() == 1 { "" } else { "s" },
        if tracks.is_empty() { "none".into() } else { tracks.join("; ") },
        project.patterns.len(),
        if project.patterns.len() == 1 { "" } else { "s" },
    )
}

#[cfg(test)]
pub(crate) mod fake {
    //! A host with a project, a history and two pedal kinds, and no engine.

    use super::*;
    use cantina_project::{History, Pedal};

    pub struct FakeHost {
        pub project: Project,
        pub history: History,
        pub applied: Vec<String>,
        /// Library folders, for the authoring tools.
        pub library: Option<crate::assistant::author::testing::TempLibrary>,
        pub rescans: u32,
    }

    impl FakeHost {
        /// A Guitar track with a gain pedal, and a Drums track.
        pub fn new() -> Self {
            let mut project = Project::new("Song", 48_000.0);
            let mut guitar = Track::new(TrackId(1), "Guitar");
            guitar.chain.push(Pedal {
                id: PedalId(2),
                spec: "gain".into(),
                values: vec![0.0],
                bypassed: false,
            });
            project.tracks.push(guitar);
            project
                .tracks
                .push(Track::drums(TrackId(3), "Drums", "acoustic".to_string()));
            Self {
                project,
                history: History::new().coalesce_window(std::time::Duration::ZERO),
                applied: Vec::new(),
                library: None,
                rescans: 0,
            }
        }
    }

    impl Host for FakeHost {
        fn project(&self) -> &Project {
            &self.project
        }
        fn revision(&self) -> u64 {
            self.project.revision
        }
        fn apply(&mut self, command: Command) -> Result<()> {
            self.applied.push(command.label());
            self.history
                .apply(&mut self.project, command)
                .map_err(|e| anyhow!("{e}"))?;
            self.project.revision += 1;
            Ok(())
        }
        fn pedal_kinds(&self) -> Vec<(String, String)> {
            vec![
                ("gain".into(), "Gain".into()),
                ("overdrive".into(), "Overdrive".into()),
            ]
        }
        fn pedal_kind(&mut self, spec: &str) -> Result<(String, Vec<ParamInfo>)> {
            match spec {
                "gain" => Ok((
                    "Gain".into(),
                    vec![ParamInfo::new("Gain", Unit::Decibels, -24.0, 24.0, 0.0)],
                )),
                "overdrive" => Ok((
                    "Overdrive".into(),
                    vec![
                        ParamInfo::new("Drive", Unit::Decibels, 0.0, 40.0, 20.0),
                        ParamInfo::new("Tone", Unit::Hertz, 500.0, 8000.0, 3000.0),
                        ParamInfo::choice("Voice", &["Smooth", "Crunch"], 0),
                    ],
                )),
                _ => {
                    // A library pedal, read from its file as the engine would.
                    let id = spec
                        .strip_prefix("pedal:")
                        .with_context(|| format!("no pedal kind {spec}"))?;
                    let path = self.library().pedals.join(format!("{id}.pedal.json"));
                    let text = std::fs::read_to_string(&path)
                        .with_context(|| format!("no pedal kind {spec}"))?;
                    let def =
                        cantina_dsp::PedalDef::from_json(&text).map_err(|e| anyhow!("{e}"))?;
                    let pedal = cantina_dsp::compile(&def).map_err(|e| anyhow!("{e}"))?;
                    Ok((
                        def.name.clone(),
                        cantina_dsp::Processor::params(&pedal).to_vec(),
                    ))
                }
            }
        }
        fn add_pedal(
            &mut self,
            track: TrackId,
            spec: &str,
            index: Option<usize>,
            values: Option<Vec<f32>>,
        ) -> Result<PedalId> {
            let (_, params) = self.pedal_kind(spec)?;
            let id = PedalId(self.project.fresh_id());
            self.apply(Command::InsertPedal {
                track,
                index: index.unwrap_or(usize::MAX),
                pedal: Pedal {
                    id,
                    spec: spec.into(),
                    values: values.unwrap_or_else(|| params.iter().map(|p| p.default).collect()),
                    bypassed: false,
                },
            })?;
            Ok(id)
        }
        fn kits(&self) -> Vec<(String, String)> {
            vec![("acoustic".into(), "Acoustic".into())]
        }
        fn pads(&self, _: &str) -> Vec<String> {
            vec!["kick".into(), "snare".into(), "hat".into()]
        }
        fn add_drum_track(&mut self, kit: Option<String>) -> Result<TrackId> {
            let id = TrackId(self.project.fresh_id());
            self.apply(Command::InsertTrack {
                index: usize::MAX,
                track: Track::drums(id, "Drums 2", kit.unwrap_or_else(|| "acoustic".into())),
            })?;
            Ok(id)
        }
        fn remove_track(&mut self, track: TrackId) -> Result<()> {
            self.apply(Command::RemoveTrack { id: track })
        }
        fn engine_report(&self) -> Value {
            json!({ "running": false })
        }
        fn library(&self) -> Folders {
            self.library.as_ref().map_or_else(
                || Folders {
                    pedals: "/nonexistent/Pedals".into(),
                    kits: "/nonexistent/Kits".into(),
                    patterns: "/nonexistent/Patterns".into(),
                    rigs: "/nonexistent/Rigs".into(),
                },
                |l| l.folders.clone(),
            )
        }
        fn kit_dir(&self, kit: &str) -> Option<PathBuf> {
            let dir = self.library().kits.join(kit);
            dir.exists().then_some(dir)
        }
        fn rescan_library(&mut self) {
            self.rescans += 1;
        }
        fn set_chain(
            &mut self,
            track: TrackId,
            pedals: Vec<(String, Vec<f32>)>,
            replace: bool,
        ) -> Result<Vec<PedalId>> {
            let mut commands = Vec::new();
            if replace {
                for p in &self.project.track(track).context("no track")?.chain {
                    commands.push(Command::RemovePedal { id: p.id });
                }
            }
            let first = self.project.fresh_id();
            let ids: Vec<PedalId> = (0..pedals.len() as u64)
                .map(|i| PedalId(first + i))
                .collect();
            for (id, (spec, values)) in ids.iter().zip(pedals) {
                commands.push(Command::InsertPedal {
                    track,
                    index: usize::MAX,
                    pedal: Pedal {
                        id: *id,
                        spec,
                        values,
                        bypassed: false,
                    },
                });
            }
            self.apply(Command::Batch { commands })?;
            Ok(ids)
        }
        fn import_pattern(&mut self, path: &Path, _: Option<TrackId>) -> Result<PatternId> {
            let text = std::fs::read_to_string(path)?;
            let pattern = cantina_project::pattern_from_json(&text).map_err(|e| anyhow!(e))?;
            let id = PatternId(self.project.fresh_id());
            self.apply(Command::InsertPattern {
                index: usize::MAX,
                pattern: Pattern { id, ..pattern },
            })?;
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHost;
    use super::*;

    #[test]
    fn every_schema_compiles_and_every_name_is_unique() {
        assert_eq!(validators().len(), TOOLS.len());
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        for spec in specs() {
            assert_eq!(spec["function"]["parameters"]["type"], "object");
            assert!(!spec["function"]["description"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn an_edit_is_one_undoable_step_with_a_card_and_an_undo_revision() {
        let mut host = FakeHost::new();
        let out = run(
            &mut host,
            "add_pedal",
            &json!({"track": "guitar", "kind": "overdrive", "position": 0, "params": {"drive": 30, "Voice": "crunch"}}),
        );
        assert!(out.ok, "{:?}", out.result);
        assert_eq!(
            out.summary,
            "Added Overdrive to Guitar (Drive 30 dB, Voice Crunch)"
        );
        assert_eq!(out.undo, Some(host.revision()));
        let chain = host.project.tracks[0].chain.clone();
        assert_eq!(chain[0].spec, "overdrive");
        assert_eq!(chain[0].values, [30.0, 3000.0, 1.0]);
        assert_eq!(
            host.applied.len(),
            1,
            "a pedal with its knobs set is one step"
        );
        let out = run(
            &mut host,
            "set_pedal_params",
            &json!({"pedal": chain[0].id.0, "params": {"tone": "1200 Hz", "drive": 99}}),
        );
        assert!(out.ok, "{:?}", out.result);
        assert_eq!(
            out.summary,
            "Set Overdrive's Drive to 40 dB, Tone to 1200 Hz on Guitar"
        );
        let out = run(
            &mut host,
            "set_mix",
            &json!({"track": 1, "gain_db": -6, "pan": -0.5}),
        );
        assert!(out.ok);
        assert_eq!(host.project.tracks[0].strip.pan, -0.5);
        // The mix change was one batch: one undo puts both back.
        host.history.undo(&mut host.project).unwrap();
        assert_eq!(host.project.tracks[0].strip.pan, 0.0);
        assert_eq!(host.project.tracks[0].strip.gain_db, 0.0);
    }

    #[test]
    fn reads_change_nothing_and_offer_no_undo() {
        let mut host = FakeHost::new();
        for name in ["get_project", "list_pedal_kinds", "list_kits", "get_engine"] {
            let out = run(&mut host, name, &json!({}));
            assert!(out.ok && out.undo.is_none(), "{name}");
        }
        assert!(host.applied.is_empty());
        let project = describe(&mut host);
        assert_eq!(project["tracks"][0]["chain"][0]["name"], "Gain");
        assert_eq!(
            project["tracks"][0]["chain"][0]["params"][0]["value"],
            "0 dB"
        );
        assert_eq!(project["tracks"][1]["type"], "drums");
        assert!(brief(&host.project).contains("Guitar (id 1, audio, 1 pedal)"));
    }

    #[test]
    fn mistakes_go_back_to_the_model_and_change_nothing() {
        let mut host = FakeHost::new();
        for (name, args, says) in [
            (
                "add_pedal",
                json!({"track": 1, "kind": "wah"}),
                "not a pedal kind",
            ),
            (
                "add_pedal",
                json!({"track": 1, "kind": "overdrive", "params": {"fizz": 1}}),
                "has no parameter",
            ),
            (
                "add_pedal",
                json!({"track": 9, "kind": "overdrive"}),
                "no track 9",
            ),
            ("set_tempo", json!({"bpm": "fast"}), "schema"),
            ("set_tempo", json!({"bpm": 900}), "schema"),
            (
                "rename_track",
                json!({"track": "Bass", "name": "x"}),
                "no track called",
            ),
            (
                "place_pattern",
                json!({"pattern": 5, "track": 3, "bar": 1}),
                "no pattern 5",
            ),
            (
                "add_pattern",
                json!({"name": "Beat", "length": 8, "rows": {"kick": "x..x"}}),
                "has 4 steps",
            ),
            ("nonsense", json!({}), "no tool"),
        ] {
            let out = run(&mut host, name, &args);
            assert!(!out.ok, "{name} {args}");
            assert!(
                out.result["error"].as_str().unwrap().contains(says),
                "{name}: {}",
                out.result
            );
            assert!(out.undo.is_none());
        }
        assert!(host.applied.is_empty());
    }

    #[test]
    fn patterns_are_written_read_placed_and_looped() {
        let mut host = FakeHost::new();
        let out = run(
            &mut host,
            "add_pattern",
            &json!({
                "name": "Shuffle", "steps_per_beat": 3, "length": 12, "swing": 0,
                "rows": {"kick": "X.. ... x.. ...", "hat": "x.x x.x x.x x.x"},
            }),
        );
        assert!(out.ok, "{:?}", out.result);
        let id = out.result["pattern"].as_u64().unwrap();
        let read = run(&mut host, "get_pattern", &json!({"pattern": id}));
        assert_eq!(read.result["rows"]["kick"], "X.....x.....");
        let out = run(
            &mut host,
            "set_pattern",
            &json!({"pattern": id, "rows": {"hat": "............", "snare": "...X.....X.."}}),
        );
        assert!(out.ok);
        let pattern = host.project.pattern(PatternId(id)).unwrap();
        assert!(pattern.row("hat").is_none());
        assert_eq!(pattern.velocity("snare", 3), 1.0);
        let out = run(
            &mut host,
            "place_pattern",
            &json!({"pattern": id, "track": "Drums", "bar": 3, "bars": 2}),
        );
        assert!(out.ok, "{:?}", out.result);
        let clip = &host.project.tracks[1].pattern_clips[0];
        assert_eq!(clip.start, 2 * 4 * u64::from(PPQ));
        assert_eq!(clip.length, 2 * 4 * u64::from(PPQ));
        let out = run(
            &mut host,
            "place_pattern",
            &json!({"pattern": id, "track": "Guitar", "bar": 1}),
        );
        assert!(!out.ok);
        let out = run(
            &mut host,
            "set_loop",
            &json!({"start_bar": 2, "end_bar": 4}),
        );
        assert!(out.ok);
        let l = host.project.loop_region.unwrap();
        assert_eq!((l.start, l.end, l.on), (96_000, 288_000, true));
    }

    #[test]
    fn destructive_tools_ask_first_in_words() {
        let host = FakeHost::new();
        assert_eq!(
            confirm(&host, "remove_track", &json!({"track": 1})).unwrap(),
            "Remove the track Guitar, with its clips and pedals?"
        );
        assert!(confirm(&host, "delete_pattern", &json!({"pattern": 1})).is_some());
        assert!(confirm(&host, "delete_clip", &json!({"clip": 1})).is_some());
        assert!(confirm(&host, "add_pedal", &json!({})).is_none());
        let mut host = host;
        let out = run(&mut host, "remove_track", &json!({"track": 1}));
        assert!(out.ok);
        assert_eq!(host.project.tracks.len(), 1);
    }

    #[test]
    fn a_suggested_chain_asks_and_is_one_step() {
        let mut host = FakeHost::new();
        let args = json!({"track": "Guitar", "pedals": [
            {"kind": "gain", "params": {"Gain": 6}},
            {"kind": "overdrive", "params": {"Drive": 25, "Voice": "Crunch"}},
        ]});
        assert_eq!(
            confirm(&host, "apply_chain", &args).unwrap(),
            "Set Guitar's chain to Gain → Overdrive, replacing what is there?"
        );
        let out = run(&mut host, "apply_chain", &args);
        assert!(out.ok, "{:?}", out.result);
        assert_eq!(out.summary, "Set Guitar's chain to Gain → Overdrive");
        let chain = &host.project.tracks[0].chain;
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1].values, [25.0, 3000.0, 1.0]);
        assert_eq!(host.applied.len(), 1, "one step");
        host.history.undo(&mut host.project).unwrap();
        assert_eq!(
            host.project.tracks[0].chain.len(),
            1,
            "undo puts the old chain back"
        );
        assert_eq!(host.project.tracks[0].chain[0].id, PedalId(2));
        // A mistake in the third pedal adds none of them.
        let bad = json!({"track": 1, "pedals": [{"kind": "gain"}, {"kind": "wah"}]});
        assert!(!run(&mut host, "apply_chain", &bad).ok);
        assert_eq!(host.project.tracks[0].chain.len(), 1);
    }

    #[test]
    fn the_docs_are_searched_and_read_without_the_host() {
        let mut host = FakeHost::new();
        let out = run(
            &mut host,
            "search_docs",
            &json!({"query": "latency buffer"}),
        );
        assert!(out.ok && out.undo.is_none());
        assert!(!out.result["results"].as_array().unwrap().is_empty());
        let out = run(
            &mut host,
            "read_docs",
            &json!({"page": "guides/troubleshooting"}),
        );
        assert!(out.ok);
        assert!(out.result["url"]
            .as_str()
            .unwrap()
            .ends_with("guides/troubleshooting/"));
        assert!(!run(&mut host, "read_docs", &json!({"page": "nope"})).ok);
    }

    #[test]
    fn steps_round_trip_through_letters() {
        assert_eq!(
            parse_steps("X x o . - | x").unwrap(),
            [1.0, 0.8, 0.4, 0.0, 0.0, 0.8]
        );
        assert_eq!(steps_text(&[1.0, 0.8, 0.4, 0.0]), "Xxo.");
        assert!(parse_steps("x?").is_err());
    }
}
