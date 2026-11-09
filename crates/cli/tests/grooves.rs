//! The stock groove library in `patterns/`: every file is a groove with
//! its metadata, the library covers enough genres, and every groove
//! plays on every stock kit in `kits/` without clipping, at the fastest
//! tempo it suits, through a new drum track's fader ([`DRUM_GAIN_DB`]).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cantina_cli::render::render_pattern;
use cantina_project::{groove_from_json, Groove, Pattern, Section, DRUM_GAIN_DB};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn grooves() -> Vec<(PathBuf, Pattern, Groove)> {
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(root().join("patterns"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        for path in files {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                name.ends_with(".pattern.json"),
                "{} is not a pattern file",
                path.display()
            );
            let text = fs::read_to_string(&path).unwrap();
            let (pattern, groove) =
                groove_from_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let groove =
                groove.unwrap_or_else(|| panic!("{} has no groove metadata", path.display()));
            assert_eq!(
                format!("{}.pattern.json", pattern.name),
                name,
                "a groove's file is named after it"
            );
            out.push((path, pattern, groove));
        }
    }
    out
}

#[test]
fn the_library_covers_fifteen_genres_with_four_grooves_each() {
    let all = grooves();
    let mut by_genre: BTreeMap<String, Vec<Section>> = BTreeMap::new();
    for (path, _, groove) in &all {
        let folder = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let slug = groove.genre.to_lowercase().replace(' ', "-");
        assert_eq!(
            folder,
            slug,
            "{} is in the folder of its genre",
            path.display()
        );
        by_genre
            .entry(groove.genre.clone())
            .or_default()
            .push(groove.section);
    }
    assert!(by_genre.len() >= 15, "{} genres", by_genre.len());
    for (genre, sections) in &by_genre {
        assert!(
            sections.len() >= 4,
            "{genre} has {} grooves",
            sections.len()
        );
        assert!(sections.contains(&Section::Verse), "{genre} has no verse");
    }
    let names: HashSet<&str> = all.iter().map(|(_, p, _)| p.name.as_str()).collect();
    assert_eq!(names.len(), all.len(), "groove names are unique");
}

#[test]
fn every_groove_plays_on_every_stock_kit_without_clipping() {
    let mut kits: Vec<PathBuf> = fs::read_dir(root().join("kits"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("kit.json").is_file())
        .collect();
    kits.sort();
    assert!(!kits.is_empty());
    let mut worst = (0.0f32, String::new());
    for (path, pattern, groove) in grooves() {
        for kit in &kits {
            let (out, _missing) = render_pattern(&pattern, kit, groove.tempo.max, 2, 1.0)
                .unwrap_or_else(|e| panic!("{} on {}: {e:#}", path.display(), kit.display()));
            let fader = 10f32.powf(DRUM_GAIN_DB / 20.0);
            let peak = out
                .iter()
                .flat_map(|c| c.iter())
                .fold(0.0f32, |m, s| m.max(s.abs()))
                * fader;
            let label = format!(
                "{} on {}",
                pattern.name,
                kit.file_name().unwrap().to_string_lossy()
            );
            assert!(peak > 0.01, "{label} is silent");
            assert!(peak < 1.0, "{label} clips: peak {peak}");
            if peak > worst.0 {
                worst = (peak, label);
            }
        }
    }
    println!("loudest: {} at peak {:.3}", worst.1, worst.0);
}

#[test]
fn a_humanized_render_is_identical_every_time_and_differs_from_the_grid() {
    let path = root().join("patterns/funk/Funk verse.pattern.json");
    let (plain, _) = groove_from_json(&fs::read_to_string(path).unwrap()).unwrap();
    let mut human = plain.clone();
    human.humanize = Some(cantina_project::Humanize {
        timing: 0.6,
        velocity: 0.6,
    });
    human.seed = 42;
    human.rows[0].marks.push(cantina_project::StepMark {
        ratchet: 3,
        flam: true,
        ..cantina_project::StepMark::plain(4)
    });
    let kit = root().join("kits/rock");
    let render = |p: &Pattern| render_pattern(p, &kit, 100.0, 8, 0.5).unwrap().0;
    let a = render(&human);
    assert_eq!(a, render(&human), "the same samples every run");
    assert_ne!(a, render(&plain), "humanize moves the hits");
}
