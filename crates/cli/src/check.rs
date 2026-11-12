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
        Format::Jam => check_jam(&text, options, &mut warnings),
        Format::Exercise => check_exercise(&text, options, &mut warnings),
        Format::Course => check_course(&text, &mut warnings),
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
    let mut problems: Vec<(String, String)> = Vec::new();
    for e in validator.iter_errors(value) {
        let at = e.instance_path().to_string();
        match tagged_branch(&schema, e.schema_path().to_string().as_str(), value, &at) {
            Some(inner) => problems.extend(inner),
            None => problems.push((at, e.to_string())),
        }
    }
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

/// A `oneOf` of objects told apart by a `kind` field (a jam's backing,
/// an exercise's phrase) fails as a whole, which says nothing useful.
/// When the value names a `kind` one branch has, the problems are that
/// branch's instead, at their own pointers.
fn tagged_branch(
    schema: &Value,
    schema_path: &str,
    value: &Value,
    at: &str,
) -> Option<Vec<(String, String)>> {
    if !schema_path.ends_with("/oneOf") {
        return None;
    }
    let instance = value.pointer(at)?;
    let kind = instance.get("kind")?.as_str()?;
    let branches = schema.pointer(schema_path.trim_start_matches('#'))?;
    let branch = branches
        .as_array()?
        .iter()
        .find(|b| b.pointer("/properties/kind/const").and_then(Value::as_str) == Some(kind))?;
    let mut sub = branch.clone();
    sub["$defs"] = schema.get("$defs").cloned().unwrap_or(Value::Null);
    let validator = jsonschema::draft202012::new(&sub).ok()?;
    let inner: Vec<(String, String)> = validator
        .iter_errors(instance)
        .map(|e| (format!("{at}{}", e.instance_path()), e.to_string()))
        .collect();
    (!inner.is_empty()).then_some(inner)
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

/// A jam: its rules, then what it names. A groove that is not a stock
/// genre fails it, unless the jam has patterns of its own to play; a kit,
/// rig or audio file that is not installed is a warning, since the app
/// plays the first kit and leaves out the rest.
fn check_jam(
    text: &str,
    options: &CheckOptions,
    warnings: &mut Vec<String>,
) -> Result<String, Vec<String>> {
    use cantina_project::{Backing, Section};
    let jam = cantina_project::jam_from_json(text)?;
    let mut sections: Vec<Section> = jam.drums.patterns.keys().copied().collect();
    if let Some(genre) = &jam.drums.groove {
        let mut found = stock_grooves(genre);
        if found.is_empty() {
            let problem = format!("there is no stock groove for the genre {genre:?}");
            if jam.drums.patterns.is_empty() {
                return Err(vec![problem]);
            }
            warnings.push(problem);
        }
        // As the app does: the genre's grooves in the jam's meter.
        found.retain(|g| g.time_signature == jam.time_signature);
        if found.is_empty() && jam.drums.patterns.is_empty() {
            return Err(vec![format!(
                "the {genre} grooves are not in {}/{}",
                jam.time_signature.numerator, jam.time_signature.denominator
            )]);
        }
        for g in &found {
            if !(g.tempo.min..=g.tempo.max).contains(&jam.tempo) {
                warnings.push(format!(
                    "{} bpm is outside the {} to {} bpm the {genre} {} suits",
                    jam.tempo,
                    g.tempo.min,
                    g.tempo.max,
                    cantina_project::section_name(g.section)
                ));
            }
            if !sections.contains(&g.section) {
                sections.push(g.section);
            }
        }
    }
    sections.sort();
    let kits = crate::kit_roots(&options.kit_roots);
    match kits
        .iter()
        .map(|r| r.join(&jam.drums.kit))
        .find(|d| d.join(cantina_engine::kit::KIT_FILE).is_file())
    {
        None => warnings.push(format!(
            "the kit {:?} is not installed; the app plays the first kit it has",
            jam.drums.kit
        )),
        Some(dir) => {
            if let Ok((kit, _)) = cantina_engine::kit::load(&dir) {
                for (section, p) in &jam.drums.patterns {
                    for row in &p.rows {
                        if !kit.pads.iter().any(|pad| pad.id == row.pad) {
                            warnings.push(format!(
                                "the {} pattern plays {:?}, which the kit {:?} has no pad for",
                                cantina_project::section_name(*section),
                                row.pad,
                                jam.drums.kit
                            ));
                        }
                    }
                }
            }
        }
    }
    let roots = crate::rig::RigRoots::with_defaults(&options.ir_roots, &options.pedal_roots);
    let mut rig = |id: &str, what: &str| {
        let Some(path) = find_rig(id) else {
            warnings.push(format!(
                "{what} rig {id:?} is not installed; the app leaves it out"
            ));
            return;
        };
        let read = std::fs::read_to_string(&path)
            .map_err(|e| vec![e.to_string()])
            .and_then(|t| cantina_project::rig_from_json(&t).map_err(|e| e.problems()));
        match read.and_then(|r| crate::rig::build(&r, &roots, 2)) {
            Ok(mut built) => warnings.append(&mut built.missing),
            Err(p) => warnings.push(format!(
                "{what} rig {id:?} does not build: {}",
                p.join("; ")
            )),
        }
    };
    if let Some(id) = &jam.rig {
        rig(id, "the input's");
    }
    let backing = match &jam.backing {
        Backing::None => "drums only".to_string(),
        Backing::Progression {
            chords,
            instrument,
            rig: voice_rig,
            ..
        } => {
            if let Some(id) = voice_rig {
                rig(id, "the backing's");
            }
            let tonic = cantina_project::pitch_class(&jam.key.tonic).unwrap_or(0);
            let bars = cantina_project::parse_progression(chords, tonic)
                .map(|b| b.len())
                .unwrap_or(0);
            format!(
                "{} plays {bars} bars of chords",
                match instrument {
                    cantina_project::Instrument::Guitar => "a guitar",
                    cantina_project::Instrument::Bass => "a bass",
                }
            )
        }
        Backing::Audio { file, bpm, .. } => {
            if !Path::new(file).is_file() {
                warnings.push(format!("the backing's file {file} is not there"));
            }
            format!("audio at {bpm} bpm")
        }
        Backing::Track { project, track, .. } => format!("{track} from {project}"),
    };
    Ok(format!(
        "jam {:?}: {} on {} at {} bpm in {} {}, {} sections; {backing}",
        jam.name,
        jam.drums.groove.as_deref().unwrap_or("its own patterns"),
        jam.drums.kit,
        jam.tempo,
        jam.key.tonic,
        match jam.key.mode {
            cantina_project::Mode::Major => "major",
            cantina_project::Mode::Minor => "minor",
        },
        sections.len(),
    ))
}

/// The stock grooves of a genre, every section of it.
fn stock_grooves(genre: &str) -> Vec<cantina_project::Groove> {
    let mut found = Vec::new();
    for folder in entries(&crate::groove_dir()) {
        for file in entries(&folder) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            if let Ok((_, Some(g))) = groove_from_json(&text) {
                if g.genre == genre {
                    found.push(g);
                }
            }
        }
    }
    found
}

/// Where the rig with this id is: the user's, then the stock one.
fn find_rig(id: &str) -> Option<PathBuf> {
    let file = cantina_project::rig_file_name(id);
    crate::rig_dirs()
        .into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
}

/// An exercise: its rules, then what it names. Unlike a jam's, a rig or
/// groove that is not there fails it, since an exercise heard through
/// the wrong sound teaches the wrong thing; a kit that is not installed
/// is a warning, as the app plays the first kit it has.
fn check_exercise(
    text: &str,
    options: &CheckOptions,
    warnings: &mut Vec<String>,
) -> Result<String, Vec<String>> {
    use cantina_project::{Instrument, Phrase};
    let ex = cantina_project::exercise_from_json(text)?;
    let instrument = |i: Instrument| match i {
        Instrument::Guitar => "guitar",
        Instrument::Bass => "bass",
    };
    let mut problems = Vec::new();
    if let Some(id) = &ex.rig {
        match find_rig(id) {
            None => problems.push(format!(
                "the rig {id:?} is not installed, in ~/Music/Cantina/Rigs or among the stock rigs"
            )),
            Some(path) => {
                let roots =
                    crate::rig::RigRoots::with_defaults(&options.ir_roots, &options.pedal_roots);
                let read = std::fs::read_to_string(&path)
                    .map_err(|e| vec![e.to_string()])
                    .and_then(|t| cantina_project::rig_from_json(&t).map_err(|e| e.problems()));
                match read.and_then(|r| crate::rig::build(&r, &roots, 2).map(|b| (r, b))) {
                    Ok((rig, mut built)) => {
                        warnings.append(&mut built.missing);
                        if rig.instrument != ex.instrument {
                            warnings.push(format!(
                                "the rig {id:?} is for a {}; the exercise is for a {}",
                                instrument(rig.instrument),
                                instrument(ex.instrument)
                            ));
                        }
                    }
                    Err(p) => {
                        problems.push(format!("the rig {id:?} does not build: {}", p.join("; ")))
                    }
                }
            }
        }
    }
    if let Some(drums) = &ex.drums {
        let mut found = stock_grooves(&drums.groove);
        if found.is_empty() {
            problems.push(format!(
                "there is no stock groove for the genre {:?}",
                drums.groove
            ));
        } else {
            found.retain(|g| g.time_signature == ex.time_signature);
            match found
                .iter()
                .find(|g| g.section == cantina_project::Section::Verse)
                .or(found.first())
            {
                None => problems.push(format!(
                    "the {} grooves are not in {}/{}",
                    drums.groove, ex.time_signature.numerator, ex.time_signature.denominator
                )),
                Some(g) => {
                    if ex.tempo.goal < g.tempo.min || ex.tempo.start > g.tempo.max {
                        warnings.push(format!(
                            "the tempos {} to {} bpm never reach the {} to {} bpm the {} groove suits",
                            ex.tempo.start, ex.tempo.goal, g.tempo.min, g.tempo.max, drums.groove
                        ));
                    }
                }
            }
        }
        let installed = crate::kit_roots(&options.kit_roots).iter().any(|r| {
            r.join(&drums.kit)
                .join(cantina_engine::kit::KIT_FILE)
                .is_file()
        });
        if !installed {
            warnings.push(format!(
                "the kit {:?} is not installed; the app plays the first kit it has",
                drums.kit
            ));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let bars = ex.loop_ticks() / ex.bar_ticks().max(1);
    let what = match &ex.phrase {
        Phrase::Notes { notes } => format!("{} notes", notes.len()),
        Phrase::Chords { chords, .. } => format!(
            "{} {}",
            chords.len(),
            if chords.len() == 1 { "chord" } else { "chords" }
        ),
    };
    let ladder = if ex.tempo.goal > ex.tempo.start {
        format!(
            "{} to {} bpm by {}",
            ex.tempo.start, ex.tempo.goal, ex.tempo.step
        )
    } else {
        format!("{} bpm", ex.tempo.start)
    };
    let through = ex.rig.as_deref().unwrap_or("no rig");
    let over = ex.drums.as_ref().map_or("the click".to_string(), |d| {
        format!("{} on {}", d.groove, d.kit)
    });
    Ok(format!(
        "exercise {:?}: {} {}, {}; {what} over {bars} {}, {ladder}, through {through} over {over}",
        ex.name,
        ex.style,
        instrument(ex.instrument),
        ex.teaches.name(),
        if bars == 1 { "bar" } else { "bars" },
    ))
}

/// A course: its rules, then its exercises. One that is not installed,
/// or is for the other instrument, fails it; a goal outside an
/// exercise's tempo ladder is a warning.
fn check_course(text: &str, warnings: &mut Vec<String>) -> Result<String, Vec<String>> {
    let course = cantina_project::course_from_json(text)?;
    let mut problems = Vec::new();
    for (i, step) in course.steps.iter().enumerate() {
        let at = format!("step {} ({})", i + 1, step.exercise);
        let file = format!("{}{}", step.exercise, cantina_project::EXERCISE_SUFFIX);
        let Some(path) = crate::exercise_dirs()
            .into_iter()
            .map(|d| d.join(&file))
            .find(|p| p.is_file())
        else {
            problems.push(format!(
                "{at}: no exercise {file} in ~/Music/Cantina/Exercises or among the stock ones"
            ));
            continue;
        };
        let read = std::fs::read_to_string(&path)
            .map_err(|e| vec![e.to_string()])
            .and_then(|t| cantina_project::exercise_from_json(&t));
        match read {
            Err(p) => problems.push(format!("{at} does not load: {}", p.join("; "))),
            Ok(e) => {
                if e.instrument != course.instrument {
                    problems.push(format!("{at} is for another instrument than the course"));
                }
                if step.goal < e.tempo.start || step.goal > e.tempo.goal {
                    warnings.push(format!(
                        "{at}: the goal {} is outside the exercise's {} to {} bpm",
                        step.goal, e.tempo.start, e.tempo.goal
                    ));
                }
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(format!(
        "course {:?}: {} {}, {} steps",
        course.name,
        course.style,
        match course.instrument {
            cantina_project::Instrument::Guitar => "guitar",
            cantina_project::Instrument::Bass => "bass",
        },
        course.steps.len()
    ))
}

/// The entries of a folder, sorted; none when it cannot be read.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
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
        assert!(
            r.summary.contains("\"Bass Metal\" (bass, Metal): 5 pedals"),
            "{}",
            r.summary
        );
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
                .any(|w| w.contains("1 values for 6 parameters")),
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
