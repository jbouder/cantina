//! Authoring in the app: the cantina-author skill's loop without a coding
//! agent. The model writes a pedal, a drum pattern or a kit recipe; this
//! checks it with the same code as `cantina-cli check`, renders it with
//! `cantina-cli render` (or synthesises the kit with `synth-kit`), and
//! only then saves it to the library folder the app watches.
//!
//! A file that fails is never saved: the problems go back to the model,
//! which gets [`MAX_TRIES`] failed attempts per message before it is told
//! to stop and explain.

use std::path::{Path, PathBuf};

use cantina_cli::render::{self, Source, Stats};
use cantina_cli::{check, CheckOptions, Format};
use cantina_dsp::PedalDef;
use serde_json::{json, Value};

use crate::library::Folders;

/// Failed authoring calls allowed in one message before the model is
/// told to stop and say what went wrong.
pub const MAX_TRIES: u32 = 3;

/// What the model is told about the formats: the skill's generated
/// references and its examples, compiled in so they stay in step with
/// the build.
pub fn reference() -> String {
    format!(
        "# Writing pedals, drum patterns and kit recipes\n\n\
         Write files with write_pedal, write_pattern and write_kit_recipe. Each is checked \
         and rendered exactly as `cantina-cli` does; one that fails is not saved, and you get \
         the problems back to fix. Judge a pedal by its render: no clipping at the defaults, \
         a level within a few dB of the input (the DI peaks at -6 dBFS, RMS about -23 dBFS), \
         DC under 0.001. A pattern's rows are by the pad ids of the kit it will play on. \
         Leave out \"$schema\". After {MAX_TRIES} failed attempts in one message, stop and \
         tell the player what went wrong.\n\n\
         {}\n\n{}\n\n{}\n\n## Examples\n\n\
         A drive:\n```json\n{}\n```\n\n\
         A chorus:\n```json\n{}\n```\n\n\
         A delay:\n```json\n{}\n```\n\n\
         A kit recipe:\n```json\n{}\n```\n\n\
         A pattern:\n```json\n{}\n```",
        include_str!("../../../.claude/skills/cantina-author/references/formats.md"),
        include_str!("../../../.claude/skills/cantina-author/references/node-catalog.md"),
        include_str!("../../../.claude/skills/cantina-author/references/design-rules.md"),
        include_str!("../../../.claude/skills/cantina-author/examples/warm-drive.pedal.json")
            .trim(),
        include_str!("../../../.claude/skills/cantina-author/examples/vibe-chorus.pedal.json")
            .trim(),
        include_str!("../../../.claude/skills/cantina-author/examples/dark-echo.pedal.json").trim(),
        include_str!("../../../.claude/skills/cantina-author/examples/boom-bap.recipe.json").trim(),
        include_str!("../../../.claude/skills/cantina-author/examples/half-time.pattern.json")
            .trim(),
    )
}

/// What is being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pedal,
    Pattern,
    KitRecipe,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Pedal => "pedal",
            Kind::Pattern => "pattern",
            Kind::KitRecipe => "kit",
        }
    }
}

/// A file that passed and was saved.
#[derive(Debug, Clone)]
pub struct Written {
    /// Where it was saved: a file, or a kit's folder.
    pub path: PathBuf,
    /// Its name, from inside the file.
    pub name: String,
    /// What `check` said of it.
    pub summary: String,
    /// The render's numbers, for a pedal or a pattern played on a kit.
    pub stats: Option<Stats>,
    /// What passed but is worth knowing.
    pub warnings: Vec<String>,
}

impl Written {
    /// The result the model reads.
    pub fn result(&self) -> Value {
        json!({
            "ok": true,
            "saved": self.path.display().to_string(),
            "check": self.summary,
            "render": self.stats.as_ref().map(|s| json!({
                "peakDb": round(s.peak_db),
                "rmsDb": round(s.rms_db),
                "dc": s.dc_offset,
                "clippedSamples": s.clipped,
                "latencySamples": s.latency,
            })),
            "warnings": self.warnings,
        })
    }
}

fn round(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

/// A file id: lower case letters, digits and dashes, as the library
/// wants for pedals and kits.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A pattern's file name: its name, without characters a file name
/// cannot hold.
fn valid_pattern_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.len() <= 80
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with('.')
}

/// Where a file of this kind and stem is saved.
pub fn destination(kind: Kind, stem: &str, folders: &Folders) -> PathBuf {
    match kind {
        Kind::Pedal => folders.pedals.join(format!("{stem}.pedal.json")),
        Kind::Pattern => folders
            .patterns
            .join(format!("{}.pattern.json", stem.trim())),
        Kind::KitRecipe => folders.kits.join(stem),
    }
}

/// A scratch folder of its own for one attempt, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cantina-author-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Checks, renders and saves `content`. On any problem nothing is saved
/// and the problems come back, worded for the model to fix.
pub fn write(
    kind: Kind,
    stem: &str,
    content: &Value,
    folders: &Folders,
    kit_dir: Option<&Path>,
    replace: bool,
) -> Result<Written, Vec<String>> {
    let one = |p: String| vec![p];
    match kind {
        Kind::Pedal | Kind::KitRecipe if !valid_id(stem) => {
            return Err(one(format!(
                "{stem:?} is not a usable id: lower case letters, digits and dashes, such as \"warm-drive\""
            )))
        }
        Kind::Pattern if !valid_pattern_name(stem) => {
            return Err(one(format!("{stem:?} cannot be a file name")))
        }
        _ => {}
    }
    if !content.is_object() {
        return Err(one(format!("the {} must be a JSON object", kind.noun())));
    }
    let dest = destination(kind, stem, folders);
    if dest.exists() && (kind == Kind::KitRecipe || !replace) {
        return Err(one(match kind {
            Kind::KitRecipe => format!("a kit called {stem:?} is already installed; choose another id"),
            _ => format!(
                "{} already exists; choose another {}, or set replace to true to overwrite it (the player is asked first)",
                dest.display(),
                if kind == Kind::Pedal { "id" } else { "name" }
            ),
        }));
    }
    let scratch =
        Scratch::new().map_err(|e| one(format!("could not make a scratch folder: {e}")))?;
    let file = scratch.0.join(match kind {
        Kind::Pedal => format!("{stem}.pedal.json"),
        Kind::Pattern => "draft.pattern.json".to_string(),
        Kind::KitRecipe => format!("{stem}.recipe.json"),
    });
    let mut content = content.clone();
    if let Some(map) = content.as_object_mut() {
        map.remove("$schema");
    }
    let text = serde_json::to_string_pretty(&content).unwrap_or_default() + "\n";
    std::fs::write(&file, &text).map_err(|e| one(format!("could not write a draft: {e}")))?;

    // The same check `cantina-cli check` runs.
    let options = CheckOptions {
        kit: if kind == Kind::Pattern {
            kit_dir.map(Path::to_path_buf)
        } else {
            None
        },
        ..CheckOptions::default()
    };
    let report = check(&file, &options).map_err(|f| f.problems)?;
    let mut warnings = report.warnings;

    match kind {
        Kind::Pedal => {
            let def = PedalDef::from_json(&text).map_err(|e| one(e.to_string()))?;
            let (rate, input) = Source::Di.signal(4.0);
            let (out, latency) =
                render::render_pedal(&def, &[], rate, &input, rate as usize, 120.0)
                    .map_err(|e| one(format!("render: {e:#}")))?;
            let stats = Stats::of(&out, rate, latency);
            judge_pedal(&stats, &mut warnings)?;
            save_file(&text, &dest)?;
            Ok(Written {
                path: dest,
                name: def.name.clone(),
                summary: report.summary,
                stats: Some(stats),
                warnings,
            })
        }
        Kind::Pattern => {
            let pattern = cantina_project::pattern_from_json(&text).map_err(one)?;
            let stats = match kit_dir {
                Some(kit) => {
                    let (out, missing) = render::render_pattern(&pattern, kit, 120.0, 2, 1.0)
                        .map_err(|e| one(format!("render: {e:#}")))?;
                    if !missing.is_empty() {
                        warnings.push(format!(
                            "the kit has no pad {}; those rows are silent",
                            missing.join(", ")
                        ));
                    }
                    let stats = Stats::of(&out, render::RATE, 0);
                    if stats.failed() {
                        return Err(one(
                            "the pattern plays nothing on the kit: no row uses one of its pads"
                                .into(),
                        ));
                    }
                    Some(stats)
                }
                None => None,
            };
            save_file(&text, &dest)?;
            Ok(Written {
                path: dest,
                name: pattern.name,
                summary: report.summary,
                stats,
                warnings,
            })
        }
        Kind::KitRecipe => {
            let recipe =
                cantina_cli::synth::read_recipe(&file).map_err(|e| one(format!("{e:#}")))?;
            let staged = scratch.0.join("kit");
            cantina_cli::synth::synth_kit(&recipe, &staged)
                .map_err(|e| one(format!("synth-kit: {e:#}")))?;
            let kit = check(
                &staged,
                &CheckOptions {
                    format: Some(Format::Kit),
                    ..CheckOptions::default()
                },
            )
            .map_err(|f| f.problems)?;
            copy_dir(&staged, &dest).map_err(|e| {
                let _ = std::fs::remove_dir_all(&dest);
                one(format!("could not save the kit: {e}"))
            })?;
            Ok(Written {
                path: dest,
                name: recipe.name.clone(),
                summary: kit.summary,
                stats: None,
                warnings,
            })
        }
    }
}

/// The design rules a pedal's render must meet before it is saved.
fn judge_pedal(stats: &Stats, warnings: &mut Vec<String>) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    if stats.non_finite > 0 {
        problems.push(format!(
            "the render has {} samples that are not numbers: a node is unstable",
            stats.non_finite
        ));
    } else if stats.silent {
        problems
            .push("the render is silent at the defaults: is the input wired to the output?".into());
    }
    if stats.clipped > 0 {
        problems.push(format!(
            "it clips at its defaults ({} samples over full scale, peak {:.1} dBFS); lower a gain or the level",
            stats.clipped, stats.peak_db
        ));
    }
    if stats.dc_offset.abs() > 0.001 {
        problems.push(format!(
            "it has a DC offset of {:.4}; put a 20 Hz high-pass after an asymmetric curve",
            stats.dc_offset
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    // The DI's RMS is about -23 dBFS.
    if (stats.rms_db + 23.0).abs() > 9.0 {
        warnings.push(format!(
            "its level at the defaults is {:.1} dBFS RMS against the input's -23; consider the level",
            stats.rms_db
        ));
    }
    Ok(())
}

fn save_file(text: &str, dest: &Path) -> Result<(), Vec<String>> {
    let fail = |e: std::io::Error| vec![format!("could not save {}: {e}", dest.display())];
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = dest.with_extension("partial");
    std::fs::write(&tmp, text).map_err(fail)?;
    std::fs::rename(&tmp, dest).map_err(fail)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod testing {
    //! Library folders in a temporary directory.

    use super::*;

    /// Folders under a fresh temporary directory, removed when dropped.
    pub struct TempLibrary {
        pub root: PathBuf,
        pub folders: Folders,
    }

    impl TempLibrary {
        pub fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("cantina-lib-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let folders = Folders {
                pedals: root.join("Pedals"),
                kits: root.join("Kits"),
                patterns: root.join("Patterns"),
                rigs: root.join("Rigs"),
            };
            for dir in folders.all() {
                std::fs::create_dir_all(dir).unwrap();
            }
            Self { root, folders }
        }
    }

    impl Drop for TempLibrary {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// An example from the skill, as JSON.
    pub fn example(file: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.claude/skills/cantina-author/examples")
            .join(file);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn the_reference_is_the_skills_and_names_every_format() {
        let text = reference();
        for needle in ["kit-recipe", "biquad", "A drive:", "\"pads\""] {
            assert!(text.contains(needle), "{needle}");
        }
    }

    #[test]
    fn a_good_pedal_is_checked_rendered_and_saved() {
        let lib = TempLibrary::new("pedal");
        let written = write(
            Kind::Pedal,
            "warm-drive",
            &example("warm-drive.pedal.json"),
            &lib.folders,
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            written.path,
            lib.folders.pedals.join("warm-drive.pedal.json")
        );
        assert!(written.path.exists());
        let stats = written.stats.unwrap();
        assert_eq!(stats.clipped, 0);
        assert!(stats.peak_db < 0.0);
        // The saved file has no $schema and checks as the CLI would.
        let saved = std::fs::read_to_string(&written.path).unwrap();
        assert!(!saved.contains("$schema"));
        assert!(check(&written.path, &CheckOptions::default()).is_ok());
        // It is not overwritten without asking.
        let again = write(
            Kind::Pedal,
            "warm-drive",
            &example("warm-drive.pedal.json"),
            &lib.folders,
            None,
            false,
        );
        assert!(again.unwrap_err()[0].contains("already exists"));
        assert!(write(
            Kind::Pedal,
            "warm-drive",
            &example("warm-drive.pedal.json"),
            &lib.folders,
            None,
            true
        )
        .is_ok());
    }

    #[test]
    fn a_failing_file_is_never_saved_and_says_why() {
        let lib = TempLibrary::new("bad");
        let mut broken = example("warm-drive.pedal.json");
        broken["nodes"][1]["kind"] = json!("flux-capacitor");
        let problems =
            write(Kind::Pedal, "broken", &broken, &lib.folders, None, false).unwrap_err();
        assert!(!problems.is_empty());
        assert!(!lib.folders.pedals.join("broken.pedal.json").exists());
        assert!(std::fs::read_dir(&lib.folders.pedals)
            .unwrap()
            .next()
            .is_none());

        let bad_id = write(
            Kind::Pedal,
            "Warm Drive",
            &example("warm-drive.pedal.json"),
            &lib.folders,
            None,
            false,
        );
        assert!(bad_id.unwrap_err()[0].contains("not a usable id"));
        let not_object = write(
            Kind::Pattern,
            "Beat",
            &json!("x..."),
            &lib.folders,
            None,
            false,
        );
        assert!(not_object.is_err());
    }

    #[test]
    fn a_loud_pedal_is_refused_for_clipping() {
        let lib = TempLibrary::new("loud");
        let mut loud = example("warm-drive.pedal.json");
        // Turn every gain and level default far up.
        for param in loud["params"].as_array_mut().unwrap() {
            if let Some(max) = param["max"].as_f64() {
                param["default"] = json!(max);
            }
        }
        match write(Kind::Pedal, "loud", &loud, &lib.folders, None, false) {
            Err(problems) => assert!(problems.iter().any(|p| p.contains("clips")), "{problems:?}"),
            Ok(w) => assert_eq!(
                w.stats.unwrap().clipped,
                0,
                "saved only if it does not clip"
            ),
        }
    }

    #[test]
    fn a_pattern_and_a_kit_recipe_are_saved_and_the_kit_plays_the_pattern() {
        let lib = TempLibrary::new("kit");
        let kit = write(
            Kind::KitRecipe,
            "boom-bap",
            &example("boom-bap.recipe.json"),
            &lib.folders,
            None,
            false,
        )
        .unwrap();
        assert!(kit.path.join("kit.json").exists());
        let again = write(
            Kind::KitRecipe,
            "boom-bap",
            &example("boom-bap.recipe.json"),
            &lib.folders,
            None,
            true,
        );
        assert!(
            again.unwrap_err()[0].contains("already installed"),
            "a kit is never replaced"
        );

        let pattern = write(
            Kind::Pattern,
            "Half time",
            &example("half-time.pattern.json"),
            &lib.folders,
            Some(&kit.path),
            false,
        )
        .unwrap();
        assert_eq!(
            pattern.path,
            lib.folders.patterns.join("Half time.pattern.json")
        );
        let stats = pattern.stats.unwrap();
        assert!(!stats.silent);
        // Without a kit it is checked but not played.
        let unplayed = write(
            Kind::Pattern,
            "Other",
            &example("half-time.pattern.json"),
            &lib.folders,
            None,
            false,
        )
        .unwrap();
        assert!(unplayed.stats.is_none());
    }
}
