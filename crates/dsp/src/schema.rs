//! The JSON schemas for the pedal and drum kit formats, derived from the
//! types that read them. The derive gives the shape; the limits that live
//! in validation (counts, ranges, each node kind's fields from the kind
//! catalog) are added here from the same constants and catalog.
//!
//! Behind the `schema` feature. `schemas/*.schema.json` at the root of the
//! repository are written from here and from `cantina_project::schema`;
//! regenerate them with `UPDATE_SCHEMAS=1 cargo test -p cantina-project
//! schemas`. The docs site serves them under `/schemas/`.

use serde_json::{json, Map, Value};

use crate::kit::{Kit, KIT_VERSION, MAX_LAYERS, MAX_PADS, MAX_TUNE, MIN_DECAY};
use crate::pedal::{kinds, FieldKind, PedalDef, MAX_NODES, MAX_PARAMS, PEDAL_VERSION};

/// Where the docs site serves the schemas; each schema's `$id` is here.
pub const SITE: &str = "https://cantina-docs.vibeproject.workers.dev/schemas";

/// The schema for a `.pedal.json` file.
pub fn pedal() -> Value {
    let mut s = schemars::schema_for!(PedalDef).to_value();
    s["$id"] = json!(format!("{SITE}/pedal.schema.json"));
    s["title"] = json!("Cantina pedal");
    let props = &mut s["properties"];
    props["$schema"] = json!({ "type": "string" });
    props["version"]["const"] = json!(PEDAL_VERSION);
    props["nodes"]["maxItems"] = json!(MAX_NODES);
    props["params"]["maxItems"] = json!(MAX_PARAMS);
    props["edges"]["description"] = json!("Audio connections, each `[from, to]` by node id.");

    // A node's fields are flattened into it; each kind says which it takes.
    let node = &mut s["$defs"]["NodeDef"];
    let all: Vec<&str> = kinds().iter().map(|k| k.kind).collect();
    node["properties"]["kind"]["enum"] = json!(all);
    node["allOf"] = kinds()
        .iter()
        .map(|k| {
            let mut fields = Map::new();
            for f in k.fields {
                let own = match f.kind {
                    FieldKind::Number {
                        min,
                        max,
                        default,
                        unit,
                    } => json!({
                        "type": "number",
                        "minimum": min,
                        "maximum": max,
                        "default": default,
                        "description": with_unit(f.summary, unit),
                    }),
                    FieldKind::Name { options, default } => json!({
                        "enum": options,
                        "default": options[default],
                        "description": f.summary,
                    }),
                    FieldKind::Bool { default } => json!({
                        "type": "boolean",
                        "default": default,
                        "description": f.summary,
                    }),
                };
                fields.insert(
                    f.name.to_string(),
                    json!({ "anyOf": [own, { "$ref": "#/$defs/Binding" }] }),
                );
            }
            let mut names = vec!["id", "kind", "pos"];
            names.extend(k.fields.iter().map(|f| f.name));
            json!({
                "if": { "properties": { "kind": { "const": k.kind } } },
                "then": {
                    "description": k.summary,
                    "properties": fields,
                    "propertyNames": { "enum": names },
                },
            })
        })
        .collect();
    tidy(&mut s);
    s
}

/// Descriptions come from doc comments: one line each, without rustdoc's
/// link brackets.
pub fn tidy(v: &mut Value) {
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

fn with_unit(summary: &str, unit: &str) -> String {
    if unit.is_empty() {
        summary.to_string()
    } else {
        format!("{summary} In {unit}.")
    }
}

/// The schema for a kit's `kit.json`.
pub fn kit() -> Value {
    let mut s = schemars::schema_for!(Kit).to_value();
    s["$id"] = json!(format!("{SITE}/kit.schema.json"));
    s["title"] = json!("Cantina drum kit");
    let props = &mut s["properties"];
    props["$schema"] = json!({ "type": "string" });
    // Version 1, without tune and decay, still reads.
    props["version"]["minimum"] = json!(1);
    props["version"]["maximum"] = json!(KIT_VERSION);
    props["pads"]["minItems"] = json!(1);
    props["pads"]["maxItems"] = json!(MAX_PADS);
    let pad = &mut s["$defs"]["Pad"]["properties"];
    pad["pan"]["minimum"] = json!(-1);
    pad["pan"]["maximum"] = json!(1);
    pad["tune"]["minimum"] = json!(-MAX_TUNE);
    pad["tune"]["maximum"] = json!(MAX_TUNE);
    // MIN_DECAY, written as the decimal it is rather than as an f32.
    pad["decay"]["minimum"] = json!((f64::from(MIN_DECAY) * 1000.0).round() / 1000.0);
    pad["decay"]["maximum"] = json!(1);
    pad["layers"]["minItems"] = json!(1);
    pad["layers"]["maxItems"] = json!(MAX_LAYERS);
    s["$defs"]["Layer"]["properties"]["velocity"]["items"] =
        json!({ "type": "number", "minimum": 0, "maximum": 1 });
    tidy(&mut s);
    s
}

/// The schema for a kit recipe, `<name>.recipe.json`, which
/// `cantina-cli synth-kit` turns into a kit folder.
pub fn kit_recipe() -> Value {
    use crate::recipe::{KitRecipe, MAX_DECAY, MAX_RECIPE_LAYERS, RECIPE_VERSION};
    let mut s = schemars::schema_for!(KitRecipe).to_value();
    s["$id"] = json!(format!("{SITE}/kit-recipe.schema.json"));
    s["title"] = json!("Cantina kit recipe");
    let props = &mut s["properties"];
    props["version"]["const"] = json!(RECIPE_VERSION);
    props["name"]["minLength"] = json!(1);
    props["pads"]["minItems"] = json!(1);
    props["pads"]["maxItems"] = json!(MAX_PADS);
    let pad = &mut s["$defs"]["PadRecipe"]["properties"];
    pad["id"]["pattern"] = json!("^[A-Za-z0-9_-]+$");
    for (field, lo, hi) in [
        ("pitch", 20.0, 12_000.0),
        ("decay", 0.005, MAX_DECAY),
        ("tone", 0.0, 1.0),
        ("noise", 0.0, 1.0),
        ("gainDb", -60.0, 12.0),
        ("pan", -1.0, 1.0),
    ] {
        pad[field]["minimum"] = json!(lo);
        pad[field]["maximum"] = json!(hi);
    }
    pad["layers"]["minimum"] = json!(1);
    pad["layers"]["maximum"] = json!(MAX_RECIPE_LAYERS);
    tidy(&mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_draft_2020_12_with_site_ids() {
        for (s, name) in [
            (pedal(), "pedal"),
            (kit(), "kit"),
            (kit_recipe(), "kit-recipe"),
        ] {
            assert_eq!(s["$id"], format!("{SITE}/{name}.schema.json"));
            assert!(s["$schema"].as_str().unwrap().contains("2020-12"));
        }
    }

    #[test]
    fn files_that_name_their_schema_still_load() {
        let pedal = r#"{"$schema": "x", "version": 1, "name": "P",
            "nodes": [{"id": "in", "kind": "input"}, {"id": "out", "kind": "output"}],
            "edges": [["in", "out"]]}"#;
        PedalDef::from_json(pedal).unwrap();
        let kit = r#"{"$schema": "x", "version": 1, "name": "K",
            "pads": [{"id": "kick", "layers": [{"file": "k.wav"}]}]}"#;
        Kit::from_json(kit).unwrap();
    }
}
