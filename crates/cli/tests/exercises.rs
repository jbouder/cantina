//! Every stock exercise checks without a warning and plays: its phrase on
//! the tab voice, through its rig, neither silent nor clipped.

use std::path::{Path, PathBuf};

use cantina_cli::render::{render_phrase, Stats, RATE};
use cantina_cli::rig::RigRoots;
use cantina_cli::{check, CheckOptions, Format};
use cantina_project::{exercise_from_json, rig_file_name, rig_from_json, Teaches};

fn stock() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../exercises");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".exercise.json"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_stock_exercise_checks_and_plays_through_its_rig() {
    let files = stock();
    assert!(files.len() >= 30, "{} stock exercises", files.len());
    let roots = RigRoots::with_defaults(&[], &[]);
    let mut taught = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy();
        let report = check(path, &CheckOptions::default())
            .unwrap_or_else(|f| panic!("{name}: {:?}", f.problems));
        assert_eq!(report.format, Format::Exercise);
        assert!(report.warnings.is_empty(), "{name}: {:?}", report.warnings);

        let exercise = exercise_from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(
            exercise.tempo.goal > exercise.tempo.start,
            "{name}: a stock exercise climbs"
        );
        assert!(
            exercise.drums.is_some(),
            "{name}: a stock exercise has a groove"
        );
        let sentences = exercise.instructions.matches(". ").count() + 1;
        assert!(
            (2..=5).contains(&sentences),
            "{name}: {sentences} sentences of instructions"
        );
        taught.push(exercise.teaches);
        // A loop that ends in silence says so with `bars`.
        assert!(
            exercise.bars.is_some()
                || exercise.loop_ticks() - exercise.phrase_end() < exercise.beat_ticks(),
            "{name}: the loop ends with {} ticks of nothing",
            exercise.loop_ticks() - exercise.phrase_end()
        );

        let id = exercise
            .rig
            .as_deref()
            .expect("a stock exercise names a rig");
        let rig_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../rigs")
            .join(rig_file_name(id));
        let rig = rig_from_json(&std::fs::read_to_string(rig_path).unwrap()).unwrap();
        assert_eq!(rig.instrument, exercise.instrument, "{name}");
        let mut built = cantina_cli::rig::build(&rig, &roots, 1).unwrap();
        let phrase = render_phrase(&exercise, exercise.tempo.start, 1, 1.0);
        let (out, latency) =
            cantina_cli::rig::render(&rig, &mut built, RATE, &[phrase], 0, exercise.tempo.start);
        let stats = Stats::of(&out, RATE, latency);
        assert!(!stats.failed(), "{name}: {}", stats.text());
        assert_eq!(stats.clipped, 0, "{name}: {}", stats.text());
        assert!(stats.peak_db < -0.5, "{name}: {}", stats.text());
        assert!(stats.rms_db > -40.0, "{name}: {}", stats.text());
        assert!(stats.dc_offset.abs() < 1e-3, "{name}: {}", stats.text());
    }
    for teaches in [
        Teaches::Chord,
        Teaches::Change,
        Teaches::Scale,
        Teaches::Technique,
        Teaches::Riff,
        Teaches::Rhythm,
    ] {
        assert!(taught.contains(&teaches), "nothing teaches {teaches:?}");
    }
}
