//! The assistant end to end without a network or an engine: a scripted
//! model calling the real tools on a fake host through the real loop.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::chat::script::{call, converse_with, say, Scripted};
use super::chat::{Item, Thread};
use super::tools::fake::FakeHost;
use super::tools::Host;
use super::{HostTools, Lock};

impl Lock for Arc<Mutex<FakeHost>> {
    fn with<R>(&self, f: impl FnOnce(&mut dyn Host) -> R) -> R {
        f(&mut *self.lock().unwrap())
    }
}

fn thread(text: &str) -> Mutex<Thread> {
    let mut t = Thread::default();
    t.push(Item::User { text: text.into() });
    Mutex::new(t)
}

#[test]
fn a_gate_before_the_drive_is_read_built_and_reported() {
    let host = Arc::new(Mutex::new(FakeHost::new()));
    let model = Scripted::new(vec![
        call("1", "get_project", json!({})),
        call(
            "2",
            "add_pedal",
            json!({"track": 1, "kind": "overdrive", "position": 0, "params": {"Drive": 12}}),
        ),
        say("Drive added at the front of Guitar, at 12 dB."),
    ]);
    let thread = thread("put a drive first on my guitar");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);

    let host = host.lock().unwrap();
    assert_eq!(host.project.tracks[0].chain[0].spec, "overdrive");
    assert_eq!(host.applied.len(), 1);
    let items = thread.lock().unwrap().items.clone();
    assert!(
        matches!(&items[1], Item::Call { name, ok: true, undo: None, .. } if name == "get_project")
    );
    assert!(
        matches!(&items[2], Item::Call { summary, ok: true, undo: Some(r), .. } if summary == "Added Overdrive to Guitar (Drive 12 dB)" && *r == host.project.revision)
    );
    // The model saw the project it read, and the system prompt knew the tracks.
    let seen = model.seen.lock().unwrap();
    assert!(seen[0][0]["content"]
        .as_str()
        .unwrap()
        .contains("Guitar (id 1, audio, 1 pedal)"));
    assert!(seen[1].last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("\"Gain\""));
}

#[test]
fn a_refused_call_is_fixed_by_the_model_on_the_next_step() {
    let host = Arc::new(Mutex::new(FakeHost::new()));
    let model = Scripted::new(vec![
        call("1", "set_tempo", json!({"bpm": "slow"})),
        call("2", "set_tempo", json!({"bpm": 72})),
        say("Tempo is 72."),
    ]);
    let thread = thread("slow it down to 72");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);
    assert_eq!(host.lock().unwrap().project.tempo.bpm, 72.0);
    let seen = model.seen.lock().unwrap();
    assert!(seen[1].last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("error"));
}

#[test]
fn removing_a_track_waits_for_the_player() {
    for yes in [false, true] {
        let host = Arc::new(Mutex::new(FakeHost::new()));
        let model = Scripted::new(vec![
            call("1", "remove_track", json!({"track": "Drums"})),
            say("ok"),
        ]);
        let thread = thread("remove the drums");
        converse_with(&model, &mut HostTools::new(host.clone()), &thread, yes);
        assert_eq!(
            host.lock().unwrap().project.tracks.len(),
            if yes { 1 } else { 2 }
        );
    }
}

fn with_library(tag: &str) -> Arc<Mutex<FakeHost>> {
    let mut host = FakeHost::new();
    host.library = Some(super::author::testing::TempLibrary::new(tag));
    Arc::new(Mutex::new(host))
}

#[test]
fn a_pedal_that_fails_is_fixed_saved_and_added_to_the_track() {
    use super::author::testing::example;
    let host = with_library("e2e-pedal");
    let good = example("warm-drive.pedal.json");
    let mut broken = good.clone();
    broken["nodes"][1]["kind"] = json!("flux-capacitor");
    let model = Scripted::new(vec![
        call(
            "1",
            "write_pedal",
            json!({"id": "warm-drive", "definition": broken, "add_to_track": "Guitar"}),
        ),
        call(
            "2",
            "write_pedal",
            json!({"id": "warm-drive", "definition": good, "add_to_track": "Guitar"}),
        ),
        say("Saved Warm Drive and put it on Guitar."),
    ]);
    let thread = thread("make me a warm drive");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);

    let host = host.lock().unwrap();
    let folders = host.library.as_ref().unwrap().folders.clone();
    assert!(folders.pedals.join("warm-drive.pedal.json").exists());
    assert_eq!(host.rescans, 1, "rescanned once, after the save");
    // The failed attempt went back to the model with the checker's words.
    let seen = model.seen.lock().unwrap();
    let failure = seen[1].last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(failure.contains("not saved"), "{failure}");
    let items = thread.lock().unwrap().items.clone();
    assert!(matches!(&items[1], Item::Call { ok: false, .. }));
    assert!(
        matches!(&items[2], Item::Call { ok: true, summary, undo: Some(_), .. } if summary.starts_with("Wrote the pedal") && summary.ends_with("and added it to Guitar")),
        "{:?}",
        items[2]
    );
    // The system prompt carries the skill's references.
    assert!(
        seen[0][0]["content"]
            .as_str()
            .unwrap()
            .contains("node-catalog")
            || seen[0][0]["content"].as_str().unwrap().contains("biquad")
    );
}

#[test]
fn authoring_gets_a_bounded_number_of_tries_per_message() {
    let host = with_library("e2e-tries");
    let model = Scripted::new(
        (0..5)
            .map(|i| {
                call(
                    &i.to_string(),
                    "write_pedal",
                    json!({"id": "nope", "definition": {"name": "x"}}),
                )
            })
            .chain([say("I could not make it work.")])
            .collect(),
    );
    let thread = thread("make a pedal");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);
    let items = thread.lock().unwrap().items.clone();
    let stopped = items
        .iter()
        .filter(|i| matches!(i, Item::Call { summary, .. } if summary.starts_with("Stopped after")))
        .count();
    assert_eq!(
        stopped, 2,
        "attempts four and five are refused without checking"
    );
    let folders = host
        .lock()
        .unwrap()
        .library
        .as_ref()
        .unwrap()
        .folders
        .clone();
    assert!(std::fs::read_dir(folders.pedals).unwrap().next().is_none());
}

#[test]
fn a_kit_is_built_with_a_track_and_a_pattern_written_for_it() {
    use super::author::testing::example;
    let host = with_library("e2e-kit");
    let model = Scripted::new(vec![
        call(
            "1",
            "write_kit_recipe",
            json!({"id": "boom-bap", "recipe": example("boom-bap.recipe.json"), "add_drum_track": true}),
        ),
        call(
            "2",
            "write_pattern",
            json!({"name": "Half time", "definition": example("half-time.pattern.json"), "add_to_track": "Drums 2"}),
        ),
        say("Done."),
    ]);
    let thread = thread("a boom bap kit and a half-time beat");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);
    let host = host.lock().unwrap();
    let items = thread.lock().unwrap().items.clone();
    assert!(
        matches!(&items[1], Item::Call { ok: true, .. }),
        "{:?}",
        items[1]
    );
    assert!(
        matches!(&items[2], Item::Call { ok: true, .. }),
        "{:?}",
        items[2]
    );
    assert_eq!(
        host.project.tracks.last().unwrap().kit.as_deref(),
        Some("boom-bap")
    );
    assert_eq!(host.project.patterns.len(), 1);
    let folders = host.library.as_ref().unwrap().folders.clone();
    assert!(folders.kits.join("boom-bap/kit.json").exists());
    assert!(folders.patterns.join("Half time.pattern.json").exists());
}

#[test]
fn crackle_is_diagnosed_from_the_engine_and_the_docs() {
    let host = Arc::new(Mutex::new(FakeHost::new()));
    let model = Scripted::new(vec![
        call("1", "get_engine", json!({})),
        call("2", "search_docs", json!({"query": "crackles buffer size"})),
        say("Raise the buffer to 256 frames; see the latency guide."),
    ]);
    let thread = thread("why does it crackle?");
    converse_with(&model, &mut HostTools::new(host.clone()), &thread, true);
    let seen = model.seen.lock().unwrap();
    // The prompt says to read the numbers, and the engine report and the
    // docs' addresses reached the model.
    assert!(seen[0][0]["content"]
        .as_str()
        .unwrap()
        .contains("read get_engine first"));
    assert!(seen[1].last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("running"));
    assert!(seen[2].last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("https://"));
    assert!(
        host.lock().unwrap().applied.is_empty(),
        "diagnosing changes nothing"
    );
}

#[test]
fn a_suggested_chain_is_applied_only_with_the_players_yes_as_one_step() {
    for yes in [false, true] {
        let host = Arc::new(Mutex::new(FakeHost::new()));
        let model = Scripted::new(vec![
            call(
                "1",
                "apply_chain",
                json!({"track": "Guitar", "pedals": [{"kind": "overdrive", "params": {"Drive": 30}}, {"kind": "gain"}]}),
            ),
            say("ok"),
        ]);
        let thread = thread("yes, set that up");
        converse_with(&model, &mut HostTools::new(host.clone()), &thread, yes);
        let host = host.lock().unwrap();
        let chain: Vec<&str> = host.project.tracks[0]
            .chain
            .iter()
            .map(|p| p.spec.as_str())
            .collect();
        if yes {
            assert_eq!(chain, ["overdrive", "gain"]);
            assert_eq!(host.applied.len(), 1);
        } else {
            assert_eq!(chain, ["gain"]);
            assert!(host.applied.is_empty());
        }
    }
}

#[test]
fn the_droid_talks_only_when_its_persona_is_on() {
    for on in [true, false] {
        let host = Arc::new(Mutex::new(FakeHost::new()));
        let model = Scripted::new(vec![say("Tempo locked.")]);
        let thread = thread("hi");
        converse_with(
            &model,
            &mut HostTools::new(host).with_persona(on),
            &thread,
            true,
        );
        let system = model.seen.lock().unwrap()[0][0]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(system.contains("You are Tapster"), on);
        // Plain or not, the working rules are there.
        assert!(system.contains("Call get_project before changing anything"));
    }
}
