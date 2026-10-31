//! Editor windows need the main thread, so this test is its own `main`
//! (`harness = false`). It opens AUDelay's editor, which is Apple's
//! generic view since AUDelay ships no view of its own, checks the window
//! follows the view and remembers where it is, and checks a CLAP plugin
//! without a GUI is refused cleanly.

mod common;

use std::process::ExitCode;

use cantina_dsp::Processor;
use cantina_hosting::au::{self, AudioUnitHandle, AudioUnitProcessor};
use cantina_hosting::clap::{ClapHandle, ClapProcessor};
use cantina_hosting::editor::Editor;
use common::fixture_path;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

fn main() -> ExitCode {
    let mtm = MainThreadMarker::new().expect("this test runs on the main thread");
    // AppKit wants an application object before any window exists.
    let _app = NSApplication::sharedApplication(mtm);

    // An Audio Unit: AUDelay through the generic view.
    let info = au::find("aufx:dely:appl").unwrap();
    let mut processor = AudioUnitProcessor::new(info, 2).unwrap();
    processor.prepare(48_000.0, 256);
    let mut handle = processor.handle().unwrap();
    assert!(handle.has_editor(), "every Audio Unit has an editor");
    let au = handle
        .as_any()
        .downcast_mut::<AudioUnitHandle>()
        .expect("an AudioUnitHandle");
    let mut editor = Editor::open_au(mtm, au, None, Some((200.0, 300.0)), "AUDelay · Guitar")
        .expect("the generic view opens");
    assert!(editor.is_open());
    let (w, h) = editor.size();
    assert!(
        w >= 100.0 && h >= 50.0,
        "the generic view laid out: {w}x{h}"
    );
    assert!(editor.tick(mtm), "still open after a tick");
    let (x, y) = editor.origin();
    assert!(
        (x - 200.0).abs() < 1.0 && (y - 300.0).abs() < 1.0,
        "placed where asked: {x},{y}"
    );
    editor.close(mtm);
    // The processor can go while the handle lives on, and vice versa;
    // the editor held its own share of the instance.
    drop(processor);
    assert!(!handle.state().is_empty());

    // A CLAP plugin without a GUI: no window, a clear refusal.
    let mut clap = ClapProcessor::load(&fixture_path(), None, 2).unwrap();
    let mut clap_handle = clap.handle().unwrap();
    assert!(!clap_handle.has_editor());
    let clap_handle = clap_handle
        .as_any()
        .downcast_mut::<ClapHandle>()
        .expect("a ClapHandle");
    let refused = Editor::open_clap(mtm, clap_handle, None, None, "Cantina Test Gain")
        .err()
        .expect("the fixture has no gui extension");
    assert!(refused.to_string().contains("no editor"), "{refused:#}");

    println!("editor: ok");
    ExitCode::SUCCESS
}
