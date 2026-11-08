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
    converse_with(&model, &mut HostTools(host.clone()), &thread, true);

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
    converse_with(&model, &mut HostTools(host.clone()), &thread, true);
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
        converse_with(&model, &mut HostTools(host.clone()), &thread, yes);
        assert_eq!(
            host.lock().unwrap().project.tracks.len(),
            if yes { 1 } else { 2 }
        );
    }
}
