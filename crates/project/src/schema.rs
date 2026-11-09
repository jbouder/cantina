//! The JSON schemas for the project, standalone pattern and tab formats,
//! derived from the model types so they cannot drift from the code. Doc
//! comments on the types become `description`s; the limits that live in
//! [`Project::validate`](crate::Project::validate) (step counts, swing,
//! velocities, pan) are added here from the same constants.
//!
//! Behind the `schema` feature. `schemas/*.schema.json` at the root of the
//! repository are written from here and from `cantina_dsp::schema`:
//!
//! ```sh
//! UPDATE_SCHEMAS=1 cargo test -p cantina-project schemas
//! ```
//!
//! regenerates them all (pedal, kit, kit recipe, pattern, tab and project), and
//! the same test fails when any is stale.

use serde_json::{json, Value};

use crate::model::{Pattern, Project, FORMAT_VERSION, MAX_PATTERN_STEPS};
use crate::tab::{Tab, MAX_BEND, MAX_FRET};

/// Where the docs site serves the schemas; each schema's `$id` is here.
pub const SITE: &str = "https://cantina-docs.vibeproject.workers.dev/schemas";

/// Steps per beat a pattern may use.
pub const STEPS_PER_BEAT: [u8; 6] = [1, 2, 3, 4, 6, 8];

/// The schema for a project's `project.json`.
pub fn project() -> Value {
    let mut s = schemars::schema_for!(Project).to_value();
    s["$id"] = json!(format!("{SITE}/project.schema.json"));
    s["title"] = json!("Cantina project");
    let props = &mut s["properties"];
    props["$schema"] = json!({ "type": "string" });
    props["version"]["const"] = json!(FORMAT_VERSION);
    props["sampleRate"]["exclusiveMinimum"] = json!(0);
    let defs = &mut s["$defs"];
    limit_pattern(&mut defs["Pattern"]);
    limit_row(defs);
    defs["Strip"]["properties"]["pan"]["minimum"] = json!(-1);
    defs["Strip"]["properties"]["pan"]["maximum"] = json!(1);
    limit_tempo(defs);
    limit_tab(&mut defs["Tab"]);
    limit_tab_defs(defs);
    for (def, field) in [("Clip", "length"), ("PatternClip", "length")] {
        defs[def]["properties"][field]["minimum"] = json!(1);
    }
    defs["Track"]["properties"]["kit"]["pattern"] = json!("^[^/\\\\]+$");
    tidy(&mut s);
    s
}

/// The schema for a standalone `<name>.pattern.json`: the object a
/// project holds in `patterns`, without its `id`, which the project
/// assigns on import.
pub fn pattern() -> Value {
    let mut s = schemars::schema_for!(Pattern).to_value();
    s["$id"] = json!(format!("{SITE}/pattern.schema.json"));
    s["title"] = json!("Cantina drum pattern");
    s["description"] = json!(
        "A step sequencer pattern as a file of its own: a grid of pads against steps, \
         with a velocity per cell. Pads are named by the ids the kit format uses \
         (`kick`, `snare`, `hh-closed`…), so the pattern plays on any kit with those pads."
    );
    let props = s["properties"].as_object_mut().unwrap();
    props.remove("id");
    props.insert("$schema".into(), json!({ "type": "string" }));
    let required = s["required"].as_array_mut().unwrap();
    required.retain(|r| r != "id");
    s["$defs"].as_object_mut().unwrap().remove("PatternId");
    // A stock groove says what it is for: optional, and only in a file.
    let groove = schemars::schema_for!(crate::Groove).to_value();
    let mut groove_def = groove.clone();
    if let Some(o) = groove_def.as_object_mut() {
        o.remove("$schema");
        o.remove("$defs");
        o.remove("title");
    }
    groove_def["description"] = json!(
        "What a stock groove is for: its genre, the section of a song, its time signature, \
         the tempos it suits and its feel. A project keeps the pattern without it."
    );
    groove_def["properties"]["tempo"]["description"] =
        json!("The tempos it suits, in beats per minute, 20 to 400, the slower first.");
    let defs = s["$defs"].as_object_mut().unwrap();
    if let Some(inner) = groove["$defs"].as_object() {
        for (k, v) in inner {
            defs.insert(k.clone(), v.clone());
        }
    }
    defs.insert("Groove".into(), groove_def);
    s["properties"]
        .as_object_mut()
        .unwrap()
        .insert("groove".into(), json!({ "$ref": "#/$defs/Groove" }));
    limit_pattern(&mut s);
    limit_row(&mut s["$defs"]);
    tidy(&mut s);
    s
}

/// The schema for a standalone `<name>.tab.json`: the tab a clip holds,
/// without its `origin`, which only means something on a clip.
pub fn tab() -> Value {
    let mut s = schemars::schema_for!(Tab).to_value();
    s["$id"] = json!(format!("{SITE}/tab.schema.json"));
    s["title"] = json!("Cantina tab");
    s["description"] = json!(
        "A guitar or bass part as strings and frets: the tuning, the capo, the tempo the \
         notes were written against, and the notes with their techniques. Strings count \
         from 1, the highest-pitched; frets count from the capo; times are ticks, 960 per \
         quarter note, from the start of the tab."
    );
    let props = s["properties"].as_object_mut().unwrap();
    props.remove("origin");
    props.insert("$schema".into(), json!({ "type": "string" }));
    limit_tab(&mut s);
    limit_tempo(&mut s["$defs"]);
    limit_tab_defs(&mut s["$defs"]);
    tidy(&mut s);
    s
}

/// A usable tempo and time signature.
fn limit_tempo(defs: &mut Value) {
    let t = &mut defs["Tempo"]["properties"];
    t["bpm"]["exclusiveMinimum"] = json!(0);
    t["numerator"]["minimum"] = json!(1);
    t["denominator"]["minimum"] = json!(1);
}

/// The limits `Tab::validate` can state per field, on a tab's schema.
/// The string count per instrument, and each note's string against it,
/// are left to `cantina-cli check`.
fn limit_tab(t: &mut Value) {
    let props = &mut t["properties"];
    props["tuning"]["minItems"] = json!(4);
    props["tuning"]["maxItems"] = json!(8);
    props["tuning"]["items"]["maximum"] = json!(127);
    props["capo"]["maximum"] = json!(MAX_FRET);
}

/// A note's string, fret, length and velocity, and a bend's size.
fn limit_tab_defs(defs: &mut Value) {
    let note = &mut defs["TabNote"]["properties"];
    note["string"]["minimum"] = json!(1);
    note["string"]["maximum"] = json!(8);
    note["fret"]["maximum"] = json!(MAX_FRET);
    note["length"]["minimum"] = json!(1);
    note["velocity"]["minimum"] = json!(0);
    note["velocity"]["maximum"] = json!(1);
    let bend = &mut defs["Bend"]["properties"];
    bend["semitones"]["exclusiveMinimum"] = json!(0);
    bend["semitones"]["maximum"] = json!(MAX_BEND);
    let heard = &mut defs["HeardNote"]["properties"];
    heard["pitch"]["maximum"] = json!(127);
    heard["onset"]["minimum"] = json!(0);
    heard["length"]["exclusiveMinimum"] = json!(0);
    heard["velocity"]["minimum"] = json!(0);
    heard["velocity"]["maximum"] = json!(1);
    heard["bend"]["minimum"] = json!(-MAX_BEND);
    heard["bend"]["maximum"] = json!(MAX_BEND);
}

/// The limits `Pattern::validate` enforces, on a pattern's schema.
fn limit_pattern(p: &mut Value) {
    let props = &mut p["properties"];
    props["stepsPerBeat"]["enum"] = json!(STEPS_PER_BEAT);
    props["length"]["minimum"] = json!(1);
    props["length"]["maximum"] = json!(MAX_PATTERN_STEPS);
    props["swing"]["minimum"] = json!(0);
    props["swing"]["maximum"] = json!(1);
    props["name"]["minLength"] = json!(1);
}

/// A row's pad is named and its steps are velocities from 0 to 1.
fn limit_row(defs: &mut Value) {
    let row = &mut defs["PatternRow"]["properties"];
    row["pad"]["minLength"] = json!(1);
    row["steps"]["items"] = json!({ "type": "number", "minimum": 0, "maximum": 1 });
}

/// Descriptions come from doc comments: one line each, without rustdoc's
/// link brackets.
fn tidy(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                match x {
                    Value::String(d) if k == "description" => {
                        *d = d.replace("[`", "`").replace("`]", "`").replace('\n', " ");
                    }
                    _ => tidy(x),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(tidy),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn all() -> [(&'static str, Value); 6] {
        [
            ("pedal", cantina_dsp::schema::pedal()),
            ("kit", cantina_dsp::schema::kit()),
            ("kit-recipe", cantina_dsp::schema::kit_recipe()),
            ("pattern", pattern()),
            ("tab", tab()),
            ("project", project()),
        ]
    }

    #[test]
    fn schemas_are_current() {
        let update = std::env::var_os("UPDATE_SCHEMAS").is_some();
        for (name, schema) in all() {
            let path = root().join(format!("schemas/{name}.schema.json"));
            let mut text = serde_json::to_string_pretty(&schema).unwrap();
            text.push('\n');
            if update {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &text).unwrap();
                continue;
            }
            let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(
                on_disk, text,
                "schemas/{name}.schema.json is stale; run UPDATE_SCHEMAS=1 cargo test -p cantina-project schemas"
            );
        }
    }

    fn validator(schema: &Value) -> jsonschema::Validator {
        jsonschema::draft202012::new(schema).unwrap()
    }

    fn assert_valid(v: &jsonschema::Validator, path: &Path) {
        let text = std::fs::read_to_string(path).unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = v
            .iter_errors(&json)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{}: {errors:#?}", path.display());
    }

    fn files(dir: &Path, suffix: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(files(&path, suffix));
            } else if path.to_string_lossy().ends_with(suffix) {
                out.push(path);
            }
        }
        out.sort();
        out
    }

    #[test]
    fn schemas_every_stock_file_validates() {
        let [pedal, kit, recipe, pattern_s, tab_s, project_s] = all().map(|(_, s)| validator(&s));
        let pedals = files(&root().join("pedals"), ".pedal.json");
        assert!(!pedals.is_empty(), "no stock pedals found");
        pedals.iter().for_each(|p| assert_valid(&pedal, p));
        let kits = files(&root().join("kits"), "kit.json");
        assert!(kits.len() >= 3, "no stock kits found");
        kits.iter().for_each(|k| assert_valid(&kit, k));
        let recipes = files(&root().join("recipes"), ".recipe.json");
        assert!(!recipes.is_empty(), "no stock kit recipes found");
        recipes.iter().for_each(|r| assert_valid(&recipe, r));
        let v8 = root().join("crates/project/tests/fixtures/v8.json");
        assert_valid(&project_s, &v8);
        // Each pattern in a project, without its id, is a pattern file.
        let project: Value = serde_json::from_str(&std::fs::read_to_string(v8).unwrap()).unwrap();
        for mut p in project["patterns"].as_array().unwrap().clone() {
            p.as_object_mut().unwrap().remove("id");
            assert!(pattern_s.is_valid(&p), "{p}");
        }
        // Each tab on a clip, without its origin, is a tab file.
        let mut tabs = 0;
        for track in project["tracks"].as_array().unwrap() {
            for clip in track["clips"].as_array().unwrap() {
                if let Some(mut t) = clip.get("tab").cloned() {
                    t.as_object_mut().unwrap().remove("origin");
                    assert!(tab_s.is_valid(&t), "{t}");
                    tabs += 1;
                }
            }
        }
        assert!(tabs > 0, "the fixture has no tab");
    }

    #[test]
    fn schemas_catch_what_validate_catches() {
        let v = validator(&pattern());
        let good = json!({ "name": "Beat", "stepsPerBeat": 4, "length": 2, "swing": 0,
            "rows": [{ "pad": "kick", "steps": [1, 0] }] });
        assert!(v.is_valid(&good));
        for (pointer, bad) in [
            ("/stepsPerBeat", json!(5)),
            ("/length", json!(0)),
            ("/length", json!(MAX_PATTERN_STEPS + 1)),
            ("/swing", json!(1.5)),
            ("/rows/0/steps/0", json!(2)),
            ("/name", json!("")),
        ] {
            let mut p = good.clone();
            *p.pointer_mut(pointer).unwrap() = bad;
            assert!(!v.is_valid(&p), "{pointer} accepted {p}");
        }
        let mut with_schema = good.clone();
        with_schema["$schema"] = json!(format!("{SITE}/pattern.schema.json"));
        assert!(v.is_valid(&with_schema));
    }

    #[test]
    fn schemas_catch_what_tab_validate_catches() {
        let v = validator(&tab());
        let good = json!({ "instrument": "guitar", "tuning": [40, 45, 50, 55, 59, 64],
            "notes": [{ "string": 1, "fret": 3, "start": 0, "length": 960,
                "bend": { "semitones": 1 } }] });
        assert!(v.is_valid(&good));
        crate::tab_from_json(&good.to_string()).unwrap();
        for (pointer, bad) in [
            ("/instrument", json!("banjo")),
            ("/tuning", json!([40, 45, 50])),
            ("/tuning/0", json!(128)),
            ("/notes/0/string", json!(0)),
            ("/notes/0/fret", json!(MAX_FRET + 1)),
            ("/notes/0/length", json!(0)),
            ("/notes/0/bend/semitones", json!(0)),
            ("/notes/0/bend/semitones", json!(MAX_BEND + 1.0)),
        ] {
            let mut t = good.clone();
            *t.pointer_mut(pointer).unwrap() = bad;
            assert!(!v.is_valid(&t), "{pointer} accepted {t}");
            assert!(crate::tab_from_json(&t.to_string()).is_err(), "{pointer}");
        }
    }

    #[test]
    fn schemas_files_that_name_their_schema_still_load() {
        let text = std::fs::read_to_string(root().join("crates/project/tests/fixtures/v8.json"))
            .unwrap()
            .replacen('{', r#"{ "$schema": "x","#, 1);
        crate::load_str(&text, Path::new("v8.json")).unwrap();
    }
}
