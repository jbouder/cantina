//! `check`: a file against its schema, then against the rules the app
//! enforces beyond it.
//!
//! The schema catches shape: a missing field, a number out of range, a
//! node field its kind does not take. The rules catch the rest: a pedal
//! with two outputs or a binding to nothing, a kit naming a sample that
//! is not there, a pattern row of the wrong length, a project whose chain
//! names a pedal it does not carry or whose patterns use pads its kit
//! lacks. Problems name the node, pad, row or track.

use std::path::{Path, PathBuf};

use cantina_dsp::{compile, stock, KitRecipe, PedalDef, Processor};
use cantina_project::{groove_from_json, tab_from_json, Project, Tab};
use serde_json::Value;

use crate::Format;

/// What a check may consult beyond the file.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Read the file as this format instead of working it out.
    pub format: Option<Format>,
    /// For a pattern: the kit whose pads its rows must name.
    pub kit: Option<PathBuf>,
    /// More folders of kits, searched first, for a project's drum tracks.
    pub kit_roots: Vec<PathBuf>,
    /// More folders of impulse responses, searched first, for a rig's cabinets.
    pub ir_roots: Vec<PathBuf>,
    /// More folders of designed pedals, searched first, for a rig's `pedal:<id>`s.
    pub pedal_roots: Vec<PathBuf>,
}

/// What a passing check found.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The format the file was read as.
    pub format: Format,
    /// The JSON file read.
    pub file: PathBuf,
    /// One line on what is in it.
    pub summary: String,
    /// Things that are not wrong but may not be what was meant.
    pub warnings: Vec<String>,
}

/// Why a check failed: every problem found, the first first.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// The format, when it could be worked out.
    pub format: Option<Format>,
    /// One line each.
    pub problems: Vec<String>,
}

impl Failure {
    fn one(format: Option<Format>, problem: impl Into<String>) -> Self {
        Self {
            format,
            problems: vec![problem.into()],
        }
    }
}

/// Checks `path`, a file or a kit or project folder.
pub fn check(path: &Path, options: &CheckOptions) -> Result<Report, Failure> {
    let (format, file) = match options.format {
        Some(f) => {
            let file = if path.is_dir() {
                path.join(match f {
                    Format::Kit => cantina_engine::kit::KIT_FILE,
                    _ => cantina_project::PROJECT_FILE,
                })
            } else {
                path.to_path_buf()
            };
            (f, file)
        }
        None => Format::detect(path).map_err(|e| Failure::one(None, e))?,
    };
    let fail = |problems: Vec<String>| Failure {
        format: Some(format),
        problems,
    };
    let text = std::fs::read_to_string(&file)
        .map_err(|e| fail(vec![format!("reading {}: {e}", file.display())]))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| fail(vec![format!("not valid JSON: {e}")]))?;

    // A project from an older Cantina is upgraded on load, so the schema,
    // which describes the current version, is only for current files.
    let upgrades = format == Format::Project
        && value["version"]
            .as_u64()
            .is_some_and(|v| v < cantina_project::FORMAT_VERSION as u64);
    if !upgrades {
        let problems = schema_problems(format, &value);
        if !problems.is_empty() {
            return Err(fail(problems));
        }
    }
    let mut warnings = Vec::new();
    if upgrades {
        warnings.push(format!(
            "version {} project: Cantina upgrades it to version {} on open",
            value["version"],
            cantina_project::FORMAT_VERSION
        ));
    }
    let summary = match format {
        Format::Pedal => check_pedal(&text),
        Format::Kit => check_kit(file.parent().unwrap_or(Path::new("."))),
        Format::KitRecipe => check_recipe(&text),
        Format::Pattern => check_pattern(&text, options.kit.as_deref(), &mut warnings),
        Format::Tab => check_tab(&text, &mut warnings),
        Format::Project => check_project(&text, &file, options, &mut warnings),
        Format::Rig => check_rig(&text, options, &mut warnings),
    }
    .map_err(fail)?;
    Ok(Report {
        format,
        file,
        summary,
        warnings,
    })
}

/// Every place `value` breaks the format's schema, as `at <pointer>:
/// <what>`, in document order.
pub fn schema_problems(format: Format, value: &Value) -> Vec<String> {
    let schema = format.schema();
    let validator =
        jsonschema::draft202012::new(&schema).expect("the generated schemas are valid schemas");
    let mut problems: Vec<(String, String)> = validator
        .iter_errors(value)
        .map(|e| {
            let at = e.instance_path().to_string();
            (at, e.to_string())
        })
        .collect();
    // A node's fields are checked under an if/then per kind, which the
    // validator also reports as the enclosing failure; keep the specific.
    problems.sort_by(|a, b| a.0.cmp(&b.0));
    problems.dedup();
    problems
        .into_iter()
        .map(|(at, what)| {
            let at = if at.is_empty() { "/".to_string() } else { at };
            format!("at {at}: {what}{}", hint(value, &at))
        })
        .collect()
}

/// For a problem inside a pedal node, which node it is, by id and kind.
fn hint(value: &Value, at: &str) -> String {
    let mut parts = at.trim_start_matches('/').split('/');
    if parts.next() == Some("nodes") {
        if let Some(node) = parts
            .next()
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| value["nodes"].get(i))
        {
            if let (Some(id), Some(kind)) = (node["id"].as_str(), node["kind"].as_str()) {
                return format!(" (node {id:?}, a {kind})");
            }
        }
    }
    String::new()
}

fn check_pedal(text: &str) -> Result<String, Vec<String>> {
    let def = PedalDef::from_json(text).map_err(|e| lines(&e))?;
    let mut pedal = compile(&def).map_err(|e| lines(&e))?;
    pedal.prepare(48_000.0, 64);
    Ok(format!(
        "pedal {:?}: {} nodes, {} params, {} samples latency",
        def.name,
        def.nodes.len(),
        def.params.len(),
        pedal.latency()
    ))
}

fn lines(e: &cantina_dsp::PedalError) -> Vec<String> {
    e.problems().iter().map(|p| p.to_string()).collect()
}

fn check_rig(
    text: &str,
    options: &CheckOptions,
    warnings: &mut Vec<String>,
) -> Result<String, Vec<String>> {
    let rig = cantina_project::rig_from_json(text).map_err(|e| e.problems())?;
    let roots = crate::rig::RigRoots::with_defaults(&options.ir_roots, &options.pedal_roots);
    let mut built = crate::rig::build(&rig, &roots, 2)?;
    warnings.append(&mut built.missing);
    let mut latency = 0;
    for p in built.pedals.iter_mut() {
        p.processor.prepare(48_000.0, 64);
        if !rig.chain[p.index].bypassed {
            latency += p.processor.latency();
        }
    }
    Ok(format!(
        "rig {:?} ({}{}): {} pedals, {} samples latency",
        rig.name,
        match rig.instrument {
            cantina_project::Instrument::Guitar => "guitar",
            cantina_project::Instrument::Bass => "bass",
        },
        if rig.genre.is_empty() {
            String::new()
        } else {
            format!(", {}", rig.genre)
        },
        rig.chain.len(),
        latency
    ))
}

fn check_kit(dir: &Path) -> Result<String, Vec<String>> {
    let (kit, _) = cantina_engine::kit::load(dir).map_err(|e| vec![format!("{e:#}")])?;
    let layers: usize = kit.pads.iter().map(|p| p.layers.len()).sum();
    let ids: Vec<&str> = kit.pads.iter().map(|p| p.id.as_str()).collect();
    Ok(format!(
        "kit {:?}: {} pads ({}), {layers} samples",
        kit.name,
        kit.pads.len(),
        ids.join(", ")
    ))
}

fn check_recipe(text: &str) -> Result<String, Vec<String>> {
    let recipe = KitRecipe::from_json(text).map_err(|e| vec![e])?;
    let ids: Vec<&str> = recipe.pads.iter().map(|p| p.id.as_str()).collect();
    Ok(format!(
        "kit recipe {:?}: {} pads ({})",
        recipe.name,
        recipe.pads.len(),
        ids.join(", ")
    ))
}

fn check_pattern(
    text: &str,
    kit: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<String, Vec<String>> {
    let (pattern, groove) = groove_from_json(text).map_err(|e| vec![e])?;
    if let Some(dir) = kit {
        let (kit, _) = cantina_engine::kit::load(dir).map_err(|e| vec![format!("{e:#}")])?;
        let missing: Vec<String> = pattern
            .rows
            .iter()
            .filter(|r| kit.pad(&r.pad).is_none())
            .map(|r| {
                format!(
                    "row {:?} names a pad kit {:?} does not have",
                    r.pad, kit.name
                )
            })
            .collect();
        if !missing.is_empty() {
            let mut problems = missing;
            let pads: Vec<&str> = kit.pads.iter().map(|p| p.id.as_str()).collect();
            problems.push(format!("the kit's pads are {}", pads.join(", ")));
            return Err(problems);
        }
    }
    let hits: usize = pattern
        .rows
        .iter()
        .map(|r| r.steps.iter().filter(|v| **v > 0.0).count())
        .sum();
    if hits == 0 {
        warnings.push("the pattern has no hits; it plays silence".into());
    }
    let (numerator, denominator) = groove.as_ref().map_or((4, 4), |g| {
        (g.time_signature.numerator, g.time_signature.denominator)
    });
    let beats_per_bar = f32::from(numerator) * 4.0 / f32::from(denominator);
    let bars = pattern.length as f32 / (pattern.steps_per_beat as f32 * beats_per_bar);
    let bars = if bars == 1.0 {
        "1 bar".to_string()
    } else {
        format!("{bars} bars")
    };
    let about = groove.map_or(String::new(), |g| {
        format!(
            "; a {} {} groove, {:?} feel, {}-{} bpm",
            g.genre,
            serde_json::to_value(g.section)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            g.feel,
            g.tempo.min,
            g.tempo.max
        )
    });
    Ok(format!(
        "pattern {:?}: {} steps at {} per beat ({bars} of {numerator}/{denominator}), {} rows, {hits} hits{about}",
        pattern.name,
        pattern.length,
        pattern.steps_per_beat,
        pattern.rows.len()
    ))
}

fn check_tab(text: &str, warnings: &mut Vec<String>) -> Result<String, Vec<String>> {
    let tab = tab_from_json(text).map_err(|e| vec![e])?;
    tab_warnings(&tab, "", warnings);
    let tuning: Vec<String> = tab.tuning.iter().map(|&n| note_name(n)).collect();
    let bars = tab.end().div_ceil(tab.bar_ticks());
    Ok(format!(
        "tab {:?}: {}, {} strings ({}){}, {} notes over {bars} {} at {} bpm",
        tab.name,
        match tab.instrument {
            cantina_project::Instrument::Guitar => "guitar",
            cantina_project::Instrument::Bass => "bass",
        },
        tab.strings(),
        tuning.join(" "),
        if tab.capo > 0 {
            format!(", capo {}", tab.capo)
        } else {
            String::new()
        },
        tab.notes.len(),
        if bars == 1 { "bar" } else { "bars" },
        tab.tempo.bpm
    ))
}

/// What is allowed in a tab but almost always a mistake. `at` names
/// where the tab is, for a tab inside a project.
fn tab_warnings(tab: &Tab, at: &str, warnings: &mut Vec<String>) {
    if tab.notes.is_empty() {
        warnings.push(format!("{at}the tab has no notes"));
    }
    for (a, b) in tab.overlaps() {
        let (a, b) = (&tab.notes[a], &tab.notes[b]);
        warnings.push(format!(
            "{at}string {} has a note at tick {} while the one at tick {} still sounds; \
             a string plays one note at a time",
            b.string, b.start, a.start
        ));
    }
}

/// A MIDI note as a name: 40 is E2.
fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(midi % 12)],
        i32::from(midi / 12) - 1
    )
}

fn check_project(
    text: &str,
    file: &Path,
    options: &CheckOptions,
    warnings: &mut Vec<String>,
) -> Result<String, Vec<String>> {
    let project = cantina_project::load_str(text, file).map_err(|e| vec![e.to_string()])?;
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut problems = Vec::new();
    for track in &project.tracks {
        for pedal in &track.chain {
            let at = format!("track {:?} pedal {}", track.name, pedal.id);
            match pedal_params(&pedal.spec, dir) {
                // The app sets the values it has, in order, and leaves the
                // rest at their defaults; extra values are ignored.
                Ok(Some(count)) if count != pedal.values.len() => warnings.push(format!(
                    "{at} ({}) has {} values for {count} parameters; {}",
                    pedal.spec,
                    pedal.values.len(),
                    if pedal.values.len() < count {
                        "the rest start at their defaults"
                    } else {
                        "the extra ones are ignored"
                    }
                )),
                Ok(Some(_)) => {}
                Ok(None) => warnings.push(format!(
                    "{at} is the plugin {}, which is not checked here",
                    pedal.spec
                )),
                Err(why) => problems.push(format!("{at}: {why}")),
            }
        }
        if let Some(kit_id) = &track.kit {
            check_drum_track(&project, track, kit_id, options, &mut problems, warnings);
        }
        for clip in &track.clips {
            if let Some(tab) = &clip.tab {
                let at = format!("track {:?} clip {:?}: ", track.name, clip.name);
                tab_warnings(tab, &at, warnings);
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let tabs = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| c.tab.is_some())
        .count();
    Ok(format!(
        "project {:?}: {} tracks, {} patterns, {tabs} {}, {} bpm",
        project.name,
        project.tracks.len(),
        project.patterns.len(),
        if tabs == 1 { "tab" } else { "tabs" },
        project.tempo.bpm
    ))
}

/// How many parameters the pedal a spec names has; `None` for a plugin.
fn pedal_params(spec: &str, project_dir: &Path) -> Result<Option<usize>, String> {
    if spec.starts_with("au:") || spec.starts_with("clap:") {
        return Ok(None);
    }
    if let Some(id) = spec.strip_prefix("pedal:") {
        let path = project_dir
            .join(cantina_project::PEDALS_DIR)
            .join(format!("{id}.pedal.json"));
        let text = std::fs::read_to_string(&path).map_err(|_| {
            format!(
                "uses the designed pedal {id:?}, but {} is not there",
                path.display()
            )
        })?;
        let def = PedalDef::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let pedal = compile(&def).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(Some(pedal.params().len()));
    }
    stock(spec).map(|p| Some(p.params().len())).ok_or_else(|| {
        format!(
            "{spec:?} is not a stock pedal ({}), a pedal:<id>, au: or clap:",
            cantina_dsp::STOCK_NAMES.join(", ")
        )
    })
}

fn check_drum_track(
    project: &Project,
    track: &cantina_project::Track,
    kit_id: &str,
    options: &CheckOptions,
    problems: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let roots = crate::kit_roots(&options.kit_roots);
    let Some(dir) = roots
        .iter()
        .map(|r| r.join(kit_id))
        .find(|d| d.join(cantina_engine::kit::KIT_FILE).is_file())
    else {
        warnings.push(format!(
            "track {:?} plays kit {kit_id:?}, which is not in {}; its pads are not checked",
            track.name,
            roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return;
    };
    let kit = match cantina_engine::kit::load(&dir) {
        Ok((kit, _)) => kit,
        Err(e) => {
            problems.push(format!("track {:?}: {e:#}", track.name));
            return;
        }
    };
    let mut seen = std::collections::HashSet::new();
    for clip in &track.pattern_clips {
        let Some(pattern) = project.pattern(clip.pattern) else {
            continue;
        };
        if !seen.insert(pattern.id) {
            continue;
        }
        for row in &pattern.rows {
            if kit.pad(&row.pad).is_none() {
                problems.push(format!(
                    "track {:?} plays pattern {:?}, whose row {:?} names a pad kit {kit_id:?} \
                     does not have",
                    track.name, pattern.name, row.pad
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn problems(path: &Path, options: &CheckOptions) -> Vec<String> {
        check(path, options).unwrap_err().problems
    }

    #[test]
    fn every_stock_file_passes() {
        let o = CheckOptions::default();
        for entry in std::fs::read_dir(root().join("pedals")).unwrap() {
            let path = entry.unwrap().path();
            let r = check(&path, &o).unwrap_or_else(|f| panic!("{path:?}: {:?}", f.problems));
            assert_eq!(r.format, Format::Pedal);
        }
        for kit in ["acoustic", "electronic", "metal", "rock"] {
            let r = check(&root().join("kits").join(kit), &o).unwrap();
            assert_eq!(r.format, Format::Kit);
        }
        for recipe in ["lofi", "metal"] {
            let path = root().join(format!("recipes/{recipe}.recipe.json"));
            let r = check(&path, &o).unwrap();
            assert_eq!(r.format, Format::KitRecipe);
        }
        let r = check(&root().join("rigs/bass-metal.rig.json"), &o).unwrap();
        assert_eq!(r.format, Format::Rig);
        assert!(r.summary.contains("\"Bass Metal\" (bass, Metal): 5 pedals"), "{}", r.summary);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let r = check(
            &root().join("crates/project/tests/fixtures/v5.json"),
            &CheckOptions {
                format: Some(Format::Project),
                ..o.clone()
            },
        )
        .unwrap();
        assert!(r.summary.contains("2 tracks"), "{}", r.summary);
        let old = check(
            &root().join("crates/project/tests/fixtures/v1.json"),
            &CheckOptions {
                format: Some(Format::Project),
                ..o
            },
        )
        .unwrap();
        assert!(old.warnings[0].contains("upgrades"), "{:?}", old.warnings);
    }

    #[test]
    fn tabs_check_and_say_what_they_hold() {
        let dir = tempfile::tempdir().unwrap();
        let o = CheckOptions::default();
        let good = r#"{
            "name": "Riff",
            "instrument": "guitar",
            "tuning": [38, 45, 50, 55, 59, 64],
            "capo": 2,
            "notes": [
                { "string": 6, "fret": 0, "start": 0, "length": 960, "palmMute": true },
                { "string": 3, "fret": 7, "start": 960, "length": 3840,
                  "bend": { "semitones": 2, "release": true } }
            ]
        }"#;
        let r = check(&write(dir.path(), "riff.tab.json", good), &o).unwrap();
        assert_eq!(r.format, Format::Tab);
        assert_eq!(
            r.summary,
            "tab \"Riff\": guitar, 6 strings (D2 A2 D3 G3 B3 E4), capo 2, 2 notes over 2 bars at 120 bpm"
        );
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);

        // A seven-string bass is caught by the rules beyond the schema.
        let bass = r#"{ "instrument": "bass", "tuning": [23, 28, 33, 38, 43, 48, 53],
            "notes": [] }"#;
        let p = problems(&write(dir.path(), "bass.tab.json", bass), &o);
        assert!(p[0].contains("a bass has 4 to 6"), "{p:?}");
        // A string past the tuning.
        let high = good.replace(r#""string": 6"#, r#""string": 7"#);
        let p = problems(&write(dir.path(), "high.tab.json", &high), &o);
        assert!(p[0].contains("strings run from 1 to 6"), "{p:?}");
        // Overlapping notes on a string are a warning, not a failure.
        let overlap = good
            .replace(r#""start": 960"#, r#""start": 480"#)
            .replace(r#""string": 3"#, r#""string": 6"#);
        let r = check(&write(dir.path(), "overlap.tab.json", &overlap), &o).unwrap();
        assert!(
            r.warnings[0].contains("string 6 has a note at tick 480"),
            "{:?}",
            r.warnings
        );
    }

    #[test]
    fn a_project_with_a_tab_checks() {
        let r = check(
            &root().join("crates/project/tests/fixtures/v7.json"),
            &CheckOptions {
                format: Some(Format::Project),
                ..CheckOptions::default()
            },
        )
        .unwrap();
        assert!(r.summary.contains("1 tab"), "{}", r.summary);
        assert!(
            !r.warnings.iter().any(|w| w.contains("clip")),
            "{:?}",
            r.warnings
        );
    }

    #[test]
    fn pedal_problems_name_the_node() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(
            dir.path(),
            "bad.pedal.json",
            r#"{"version":1,"name":"Bad","nodes":[
                {"id":"in","kind":"input"},
                {"id":"lp","kind":"biquad","freq":99999},
                {"id":"out","kind":"output"}],
               "edges":[["in","lp"],["lp","out"]]}"#,
        );
        let got = problems(&p, &CheckOptions::default());
        assert!(
            got[0].contains("/nodes/1/freq") && got[0].contains("node \"lp\", a biquad"),
            "{got:?}"
        );
        let p = write(
            dir.path(),
            "two.pedal.json",
            r#"{"version":1,"name":"Two","nodes":[
                {"id":"in","kind":"input"},{"id":"out","kind":"output"},
                {"id":"out2","kind":"output"}],
               "edges":[["in","out"],["in","out2"]]}"#,
        );
        let got = problems(&p, &CheckOptions::default());
        assert!(got.iter().any(|g| g.contains("output")), "{got:?}");
    }

    #[test]
    fn pattern_problems_name_the_row_and_the_kit() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(
            dir.path(),
            "b.pattern.json",
            r#"{"name":"B","stepsPerBeat":4,"length":4,"swing":0,
               "rows":[{"pad":"kick","steps":[1,0,0]}]}"#,
        );
        let got = problems(&p, &CheckOptions::default());
        assert_eq!(got, ["the pattern row \"kick\" has 3 steps, the pattern 4"]);
        let p = write(
            dir.path(),
            "c.pattern.json",
            r#"{"name":"C","stepsPerBeat":4,"length":2,"swing":0,
               "rows":[{"pad":"gong","steps":[1,0]},{"pad":"kick","steps":[1,0]}]}"#,
        );
        let got = problems(
            &p,
            &CheckOptions {
                kit: Some(root().join("kits/rock")),
                ..Default::default()
            },
        );
        assert!(got[0].starts_with("row \"gong\""), "{got:?}");
        assert!(got[1].contains("kick"), "{got:?}");
        let ok = check(&p, &CheckOptions::default()).unwrap();
        assert!(ok.summary.contains("2 hits"), "{}", ok.summary);
    }

    #[test]
    fn project_problems_name_the_track() {
        let dir = tempfile::tempdir().unwrap();
        let text = std::fs::read_to_string(root().join("crates/project/tests/fixtures/v5.json"))
            .unwrap()
            .replace(
                r#""spec": "overdrive", "values": [20.0, 3000.0, -6.0]"#,
                r#""spec": "pedal:fuzzy", "values": [1]"#,
            )
            .replace(r#""kit": "acoustic""#, r#""kit": "rock""#);
        let p = write(dir.path(), "project.json", &text);
        let got = problems(&p, &CheckOptions::default());
        assert!(
            got[0].contains("track \"Guitar\" pedal 3") && got[0].contains("fuzzy"),
            "{got:?}"
        );
        std::fs::create_dir(dir.path().join("pedals")).unwrap();
        std::fs::copy(
            root().join("pedals/fuzz.pedal.json"),
            dir.path().join("pedals/fuzzy.pedal.json"),
        )
        .unwrap();
        let r = check(&p, &CheckOptions::default()).unwrap();
        assert!(
            r.warnings
                .iter()
                .any(|w| w.contains("1 values for 5 parameters")),
            "{:?}",
            r.warnings
        );
        // A pad the kit lacks.
        let text = text.replace(r#""pad": "hh-closed""#, r#""pad": "gong""#);
        let p = write(dir.path(), "project.json", &text);
        let got = problems(&p, &CheckOptions::default());
        assert!(
            got[0].contains("\"gong\"") && got[0].contains("\"rock\""),
            "{got:?}"
        );
    }

    #[test]
    fn a_kit_with_a_missing_sample_says_which() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "kit.json",
            r#"{"version":1,"name":"K","pads":[{"id":"kick","layers":[{"file":"nope.wav"}]}]}"#,
        );
        let got = problems(dir.path(), &CheckOptions::default());
        assert!(got[0].contains("nope.wav"), "{got:?}");
    }

    #[test]
    fn unknown_files_say_how_to_name_them() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "x.json", r#"{"what":1}"#);
        let f = check(&p, &CheckOptions::default()).unwrap_err();
        assert_eq!(f.format, None);
        assert!(f.problems[0].contains("--as"), "{:?}", f.problems);
    }
}
