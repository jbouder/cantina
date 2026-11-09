//! `cantina-cli`: everything Cantina reads, checked, rendered and listed
//! without the app, a window, a microphone or a person listening.
//!
//! - [`check()`] validates a pedal, kit, kit recipe, pattern, tab, rig or project
//!   against its JSON Schema and then against the rules the app enforces
//!   beyond it, with messages that name the node, pad or row.
//! - [`render`](mod@render) runs a pedal over a guitar DI, a tone or a WAV through the
//!   same compiled processor the app builds, or plays a pattern through a
//!   kit on the app's own sequencer and sampler, and measures the result.
//!   A rig plays a guitar or bass DI through its chain, built as the app
//!   builds it ([`rig`](mod@rig)).
//! - [`synth`](mod@synth) turns a kit recipe into a kit folder.
//! - [`kinds_table`] and [`Format::schema`] print the node catalog and
//!   the schemas.
//!
//! The binary in `main.rs` is a thin layer over these; the docs page is
//! `docs/src/content/docs/reference/cli.md`.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub mod check;
pub mod di;
pub mod render;
pub mod rig;
pub mod synth;

pub use check::{check, CheckOptions, Report};

/// Exit status for a file or render that passed.
pub const EXIT_OK: i32 = 0;
/// Exit status for a file that is invalid or a render that failed.
pub const EXIT_INVALID: i32 = 1;
/// Exit status for a mistake on the command line.
pub const EXIT_USAGE: i32 = 2;

/// The formats Cantina reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// A designed pedal, `<id>.pedal.json`.
    Pedal,
    /// A drum kit folder with its `kit.json`.
    Kit,
    /// A kit recipe, `<name>.recipe.json`, for `synth-kit`.
    #[value(name = "kit-recipe")]
    KitRecipe,
    /// A drum pattern, `<name>.pattern.json`.
    Pattern,
    /// A guitar or bass tab, `<name>.tab.json`.
    Tab,
    /// A project folder with its `project.json`.
    Project,
    /// A rig, `<name>.rig.json`: a chain of pedals with their settings.
    Rig,
}

impl Format {
    /// Every format.
    pub const ALL: [Format; 7] = [
        Format::Pedal,
        Format::Kit,
        Format::KitRecipe,
        Format::Pattern,
        Format::Tab,
        Format::Project,
        Format::Rig,
    ];

    /// The name messages and schema files use.
    pub fn name(self) -> &'static str {
        match self {
            Format::Pedal => "pedal",
            Format::Kit => "kit",
            Format::KitRecipe => "kit-recipe",
            Format::Pattern => "pattern",
            Format::Tab => "tab",
            Format::Project => "project",
            Format::Rig => "rig",
        }
    }

    /// The format's JSON Schema, as published in `schemas/`.
    pub fn schema(self) -> Value {
        match self {
            Format::Pedal => cantina_dsp::schema::pedal(),
            Format::Kit => cantina_dsp::schema::kit(),
            Format::KitRecipe => cantina_dsp::schema::kit_recipe(),
            Format::Pattern => cantina_project::schema::pattern(),
            Format::Tab => cantina_project::schema::tab(),
            Format::Project => cantina_project::schema::project(),
            Format::Rig => cantina_project::schema::rig(),
        }
    }

    /// Works out what a path holds: from a folder's contents, then the
    /// file name, then a `$schema` field, then the keys it has. Returns
    /// the format and the JSON file to read.
    pub fn detect(path: &Path) -> Result<(Format, PathBuf), String> {
        if path.is_dir() {
            for (file, format) in [
                (cantina_engine::kit::KIT_FILE, Format::Kit),
                (cantina_project::PROJECT_FILE, Format::Project),
            ] {
                if path.join(file).is_file() {
                    return Ok((format, path.join(file)));
                }
            }
            return Err(format!(
                "{} is a folder with neither a kit.json nor a project.json in it",
                path.display()
            ));
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let by_name = [
            (".pedal.json", Format::Pedal),
            (".pattern.json", Format::Pattern),
            (".tab.json", Format::Tab),
            (".recipe.json", Format::KitRecipe),
            (".rig.json", Format::Rig),
            ("kit.json", Format::Kit),
            ("project.json", Format::Project),
        ];
        if let Some((_, f)) = by_name.iter().find(|(s, _)| name.ends_with(s)) {
            return Ok((*f, path.to_path_buf()));
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
        Format::sniff(&value)
            .map(|f| (f, path.to_path_buf()))
            .ok_or_else(|| {
                format!(
                    "cannot tell what {} is; name it *.pedal.json, *.pattern.json, *.tab.json, \
                     *.recipe.json, *.rig.json, kit.json or project.json, or pass --as",
                    path.display()
                )
            })
    }

    /// The format a JSON value looks like.
    pub fn sniff(value: &Value) -> Option<Format> {
        if let Some(url) = value.get("$schema").and_then(Value::as_str) {
            if let Some(f) = Format::ALL
                .into_iter()
                .find(|f| url.ends_with(&format!("/{}.schema.json", f.name())))
            {
                return Some(f);
            }
        }
        let has = |k: &str| value.get(k).is_some();
        let pads_have_voices = value["pads"]
            .as_array()
            .is_some_and(|p| p.iter().any(|pad| pad.get("voice").is_some()));
        if has("nodes") {
            Some(Format::Pedal)
        } else if pads_have_voices {
            Some(Format::KitRecipe)
        } else if has("pads") {
            Some(Format::Kit)
        } else if has("stepsPerBeat") {
            Some(Format::Pattern)
        } else if has("chain") && has("instrument") {
            Some(Format::Rig)
        } else if has("tuning") {
            Some(Format::Tab)
        } else if has("sampleRate") || has("tracks") {
            Some(Format::Project)
        } else {
            None
        }
    }
}

/// Where kits are looked for when a check or render needs one by id:
/// the folders given, then the user's library, then the stock kits in
/// this repository when the binary was built from it.
pub fn kit_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = extra.to_vec();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Music/Cantina/Kits"));
    }
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../kits"));
    roots
}

/// The node catalog as a table: one block per kind, a line per field.
pub fn kinds_table() -> String {
    use cantina_dsp::pedal::{kinds, FieldKind};
    let mut out = String::new();
    for k in kinds() {
        let mut traits = Vec::new();
        if k.source {
            traits.push("source");
        }
        if k.control {
            traits.push("control");
        }
        if k.sums {
            traits.push("sums inputs");
        }
        let traits = if traits.is_empty() {
            String::new()
        } else {
            format!(" [{}]", traits.join(", "))
        };
        out.push_str(&format!("{}{traits}: {}\n", k.kind, k.summary));
        for f in k.fields {
            let takes = match f.kind {
                FieldKind::Number {
                    min,
                    max,
                    default,
                    unit,
                } => {
                    let unit = if unit.is_empty() {
                        String::new()
                    } else {
                        format!(" {unit}")
                    };
                    format!("{min} to {max}{unit}, default {default}")
                }
                FieldKind::Name { options, default } => {
                    format!(
                        "one of {}, default {}",
                        options.join(" | "),
                        options[default]
                    )
                }
                FieldKind::Bool { default } => format!("true or false, default {default}"),
            };
            let mut how = Vec::new();
            if f.modulatable {
                how.push("modulatable");
            }
            if f.smoothed {
                how.push("smoothed");
            }
            let how = if how.is_empty() {
                String::new()
            } else {
                format!(" ({})", how.join(", "))
            };
            out.push_str(&format!("  {:<12} {takes}{how}. {}\n", f.name, f.summary));
        }
    }
    out
}

/// The node catalog as JSON, the same as `ui/src/lib/pedal-kinds.json`.
pub fn kinds_json() -> String {
    serde_json::to_string_pretty(cantina_dsp::pedal::kinds()).expect("the catalog serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sniffs_formats_from_keys_and_schema_urls() {
        assert_eq!(Format::sniff(&json!({"nodes": []})), Some(Format::Pedal));
        assert_eq!(
            Format::sniff(&json!({"pads": [{"id": "k", "voice": "kick"}]})),
            Some(Format::KitRecipe)
        );
        assert_eq!(
            Format::sniff(&json!({"pads": [{"id": "k", "layers": []}]})),
            Some(Format::Kit)
        );
        assert_eq!(
            Format::sniff(&json!({"stepsPerBeat": 4})),
            Some(Format::Pattern)
        );
        assert_eq!(
            Format::sniff(&json!({"tuning": [40], "notes": []})),
            Some(Format::Tab)
        );
        assert_eq!(
            Format::sniff(&json!({"sampleRate": 48000})),
            Some(Format::Project)
        );
        let url = format!("{}/pattern.schema.json", cantina_project::schema::SITE);
        assert_eq!(
            Format::sniff(&json!({"$schema": url, "nodes": []})),
            Some(Format::Pattern),
            "the schema wins"
        );
        assert_eq!(
            Format::sniff(&json!({"instrument": "bass", "chain": []})),
            Some(Format::Rig)
        );
        assert_eq!(Format::sniff(&json!({"what": 1})), None);
    }

    #[test]
    fn the_kinds_json_is_the_uis() {
        let ui = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/pedal-kinds.json"),
        )
        .unwrap();
        let a: Value = serde_json::from_str(&ui).unwrap();
        let b: Value = serde_json::from_str(&kinds_json()).unwrap();
        assert_eq!(a, b);
        assert!(kinds_table().contains("lfo"));
    }
}
