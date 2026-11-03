//! The JSON schemas for the project and standalone pattern formats,
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
//! regenerates all four, and the same test fails when any is stale.

use serde_json::{json, Value};

use crate::model::{Pattern, Project, FORMAT_VERSION, MAX_PATTERN_STEPS};

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
    defs["Tempo"]["properties"]["bpm"]["exclusiveMinimum"] = json!(0);
    defs["Tempo"]["properties"]["numerator"]["minimum"] = json!(1);
    defs["Tempo"]["properties"]["denominator"]["minimum"] = json!(1);
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
    limit_pattern(&mut s);
    limit_row(&mut s["$defs"]);
    tidy(&mut s);
    s
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

    fn all() -> [(&'static str, Value); 4] {
        [
            ("pedal", cantina_dsp::schema::pedal()),
            ("kit", cantina_dsp::schema::kit()),
            ("pattern", pattern()),
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
        let [pedal, kit, pattern_s, project_s] = all().map(|(_, s)| validator(&s));
        let pedals = files(&root().join("pedals"), ".pedal.json");
        assert!(!pedals.is_empty(), "no stock pedals found");
        pedals.iter().for_each(|p| assert_valid(&pedal, p));
        let kits = files(&root().join("kits"), "kit.json");
        assert!(kits.len() >= 3, "no stock kits found");
        kits.iter().for_each(|k| assert_valid(&kit, k));
        let v5 = root().join("crates/project/tests/fixtures/v5.json");
        assert_valid(&project_s, &v5);
        // Each pattern in a project, without its id, is a pattern file.
        let project: Value = serde_json::from_str(&std::fs::read_to_string(v5).unwrap()).unwrap();
        for mut p in project["patterns"].as_array().unwrap().clone() {
            p.as_object_mut().unwrap().remove("id");
            assert!(pattern_s.is_valid(&p), "{p}");
        }
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
    fn schemas_files_that_name_their_schema_still_load() {
        let text = std::fs::read_to_string(root().join("crates/project/tests/fixtures/v5.json"))
            .unwrap()
            .replacen('{', r#"{ "$schema": "x","#, 1);
        crate::load_str(&text, Path::new("v5.json")).unwrap();
    }
}
