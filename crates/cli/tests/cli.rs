//! The `cantina-cli` binary end to end: exit codes, what goes to stdout
//! and stderr, and the files it writes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cantina-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn check_exits_0_1_and_2() {
    let fuzz = root().join("pedals/fuzz.pedal.json");
    let ok = run(&["check", path(&fuzz)]);
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stderr));
    assert!(
        text(&ok.stdout).starts_with("ok: pedal \"Fuzz\""),
        "{}",
        text(&ok.stdout)
    );

    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.pedal.json");
    std::fs::write(&bad, r#"{"version":1,"name":"Bad","nodes":[],"edges":[]}"#).unwrap();
    let invalid = run(&["check", path(&bad)]);
    assert_eq!(invalid.status.code(), Some(1));
    let err = text(&invalid.stderr);
    assert!(err.starts_with("invalid pedal:"), "{err}");
    assert!(
        err.lines().nth(1).is_some_and(|l| l.contains("input")),
        "{err}"
    );

    let json = run(&["check", "--json", path(&bad)]);
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["valid"], false);
    assert_eq!(v["format"], "pedal");

    assert_eq!(run(&["check"]).status.code(), Some(2));
    assert_eq!(run(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(run(&["check", "/no/such/file.json"]).status.code(), Some(2));
    assert_eq!(run(&["--help"]).status.code(), Some(0));
}

#[test]
fn render_pedal_writes_a_wav_and_reports_levels() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("od.wav");
    let o = run(&[
        "render",
        "pedal",
        path(&root().join("pedals/overdrive.pedal.json")),
        "--di",
        "--set",
        "drive=30",
        "--json",
        "-o",
        path(&out),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["peakDb"].as_f64().unwrap() > -30.0, "{v}");
    assert!(v["dcOffset"].as_f64().unwrap().abs() < 0.01, "{v}");
    assert_eq!(v["nonFinite"], 0);
    assert!(v["latency"].as_u64().is_some());
    let wav = hound::WavReader::open(&out).unwrap();
    // The DI's four seconds and the half-second tail.
    assert_eq!(wav.len() as usize, 48_000 * 4 + 24_000);

    let tone = run(&[
        "render",
        "pedal",
        path(&root().join("pedals/eq.pedal.json")),
        "--tone",
        "440",
        "--seconds",
        "0.5",
        "-o",
        path(&out),
    ]);
    assert_eq!(tone.status.code(), Some(0), "{}", text(&tone.stderr));
    assert!(
        text(&tone.stdout).contains("peak"),
        "{}",
        text(&tone.stdout)
    );

    let unknown = run(&[
        "render",
        "pedal",
        path(&root().join("pedals/eq.pedal.json")),
        "--set",
        "wobble=1",
        "-o",
        path(&out),
    ]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("no parameter \"wobble\""));
}

#[test]
fn render_pattern_plays_on_a_kit() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = dir.path().join("beat.pattern.json");
    std::fs::write(
        &pattern,
        r#"{"name":"Beat","stepsPerBeat":4,"length":16,"swing":0,"rows":[
          {"pad":"kick","steps":[1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0]},
          {"pad":"snare","steps":[0,0,0,0,1,0,0,0,0,0,0,0,1,0,0,0]},
          {"pad":"gong","steps":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}]}"#,
    )
    .unwrap();
    let out = dir.path().join("beat.wav");
    let o = run(&[
        "render",
        "pattern",
        path(&pattern),
        "--kit",
        path(&root().join("kits/rock")),
        "--bpm",
        "120",
        "--json",
        "-o",
        path(&out),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(
        text(&o.stderr).contains("no pad \"gong\""),
        "{}",
        text(&o.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    // Two bars at 120 bpm, then a second of tail; stereo.
    assert_eq!(v["frames"], 48_000 * 4 + 48_000);
    assert_eq!(v["channels"], 2);
    assert_eq!(v["silent"], false);
}

#[test]
fn synth_kit_writes_a_kit_that_checks_and_is_the_same_every_time() {
    let dir = tempfile::tempdir().unwrap();
    let recipe = root().join("recipes/lofi.recipe.json");
    let a = dir.path().join("lofi");
    let o = run(&["synth-kit", path(&recipe), path(&a)]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(
        text(&o.stdout).contains("8 pads, 15 samples"),
        "{}",
        text(&o.stdout)
    );
    let check = run(&["check", path(&a)]);
    assert_eq!(check.status.code(), Some(0), "{}", text(&check.stderr));
    let b = dir.path().join("again");
    run(&["synth-kit", path(&recipe), path(&b)]);
    let mut files: Vec<_> = std::fs::read_dir(&a)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(files.len(), 16);
    for f in files {
        assert_eq!(
            std::fs::read(a.join(&f)).unwrap(),
            std::fs::read(b.join(&f)).unwrap(),
            "{f:?} differs between runs"
        );
    }
    // A pattern for the stock kits plays on it.
    let o = run(&["check", path(&recipe)]);
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn kinds_and_schema_print_json() {
    let kinds = run(&["kinds", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&kinds.stdout).unwrap();
    assert!(v.as_array().unwrap().iter().any(|k| k["kind"] == "lfo"));
    for format in ["pedal", "kit", "kit-recipe", "pattern", "tab", "project"] {
        let o = run(&["schema", format]);
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        let on_disk: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root().join(format!("schemas/{format}.schema.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(v, on_disk, "{format}");
    }
    assert_eq!(run(&["schema", "song"]).status.code(), Some(2));
}

#[test]
fn check_reads_jams_and_names_what_they_lack() {
    for entry in std::fs::read_dir(root().join("jams")).unwrap() {
        let file = entry.unwrap().path();
        let out = run(&["check", path(&file)]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert!(
            text(&out.stdout).starts_with("ok: jam "),
            "{}",
            text(&out.stdout)
        );
        assert!(
            !text(&out.stdout).contains("warning"),
            "{}: {}",
            file.display(),
            text(&out.stdout)
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let jam = |body: &str| {
        let file = dir.path().join("x.jam.json");
        std::fs::write(&file, body).unwrap();
        run(&["check", path(&file)])
    };
    // A genre with no stock groove, and nothing of its own: invalid.
    let out =
        jam(r#"{"version":1,"name":"X","tempo":100,"drums":{"kit":"rock","groove":"Polka"}}"#);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("no stock groove"),
        "{}",
        text(&out.stderr)
    );
    // A rig that is not there, and a chord that is not one.
    let out = jam(
        r#"{"version":1,"name":"X","tempo":100,"drums":{"kit":"rock","groove":"Rock"},"rig":"nope"}"#,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(
        text(&out.stdout).contains("\"nope\" is not installed"),
        "{}",
        text(&out.stdout)
    );
    let out = jam(
        r#"{"version":1,"name":"X","tempo":100,"key":{"tonic":"A","mode":"minor"},
            "drums":{"kit":"rock","groove":"Rock"},
            "backing":{"kind":"progression","chords":"i | Zz","instrument":"bass"}}"#,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("\"Zz\" is not a chord"),
        "{}",
        text(&out.stderr)
    );
}
