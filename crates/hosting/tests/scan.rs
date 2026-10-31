//! The scanner: in-process examination of an Audio Unit, a child process
//! that survives the fixture's crashing plugin, and rescans that only
//! touch what changed.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use cantina_hosting::scan::{self, Format, Found, PluginList, Progress, Scanner, Source};
use common::{fixture_bundle, tempdir};

fn scanner() -> Scanner {
    let mut scanner = Scanner::new(env!("CARGO_BIN_EXE_cantina-scan"), Vec::new());
    scanner.timeout = Duration::from_secs(60);
    scanner
}

fn au_delay() -> Source {
    scan::discover()
        .into_iter()
        .find(|s| matches!(s, Source::Au { id, .. } if id == "aufx:dely:appl"))
        .expect("AUDelay ships with macOS")
}

#[test]
fn an_audio_unit_is_examined_in_process() {
    let mut found = Vec::new();
    scan::examine(&au_delay(), &mut |f| found.push(f)).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0], Found::Examining("Apple: AUDelay".into()));
    let Found::Plugin(p) = &found[1] else {
        panic!("{found:?}");
    };
    assert_eq!(p.format, Format::Au);
    assert_eq!(p.spec, "au:aufx:dely:appl");
    assert_eq!(p.name, "AUDelay");
    assert_eq!(p.vendor, "Apple");
    assert!(p.inputs >= 1 && p.outputs >= 1, "{p:?}");
    assert!(!p.version.is_empty());
}

#[test]
fn bundles_are_discovered_recursively_with_fingerprints() {
    let dir = tempdir("scan-discover");
    let nested = dir.join("Vendor");
    let bundle = fixture_bundle(&nested);
    std::fs::create_dir_all(dir.join("Not a bundle")).unwrap();
    let found = scan::discover_clap(&[dir.clone(), PathBuf::from("/nowhere/at/all")]);
    assert_eq!(found.len(), 1);
    match &found[0] {
        Source::Clap {
            path,
            modified,
            size,
        } => {
            assert_eq!(path, &bundle);
            assert!(*modified > 0);
            assert!(*size > 1000, "the dylib counts: {size}");
        }
        other => panic!("{other:?}"),
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_child_survives_a_crashing_plugin_and_the_rest_still_scans() {
    let dir = tempdir("scan-crash");
    let bundle = fixture_bundle(&dir);
    let clap = scan::discover_clap(std::slice::from_ref(&dir)).remove(0);
    let sources = vec![clap.clone(), au_delay()];

    let mut seen = Vec::new();
    let outcome = scanner()
        .scan(
            sources.clone(),
            &PluginList::default(),
            false,
            |p: &Progress| seen.push(p.clone()),
        )
        .unwrap();
    let list = &outcome.list;
    assert_eq!(outcome.examined, 2);
    assert_eq!(outcome.reused, 0);
    assert!(
        outcome.children >= 2,
        "the crash should have cost a child: {outcome:?}"
    );

    // The gain plugin was reported before the crash and is kept.
    let gain = list
        .plugins
        .iter()
        .find(|p| p.name == cantina_clap_gain::NAME)
        .unwrap_or_else(|| panic!("no gain plugin in {:#?}", list.plugins));
    assert_eq!(gain.format, Format::Clap);
    assert_eq!(gain.vendor, cantina_clap_gain::VENDOR);
    assert_eq!(gain.inputs, 2);
    assert_eq!(gain.outputs, 2);
    assert!(!gain.has_editor);
    assert_eq!(
        gain.spec,
        format!("clap:{}#{}", bundle.display(), cantina_clap_gain::ID)
    );
    // The crash is pinned on the plugin that was being instantiated.
    assert_eq!(list.failed.len(), 1, "{:#?}", list.failed);
    let failure = &list.failed[0];
    assert_eq!(failure.name, cantina_clap_gain::CRASH_NAME);
    assert_eq!(failure.source, clap.key());
    assert!(failure.why.contains("crashed"), "{}", failure.why);
    // The Audio Unit after it was still examined, by a fresh child.
    assert!(list.plugins.iter().any(|p| p.name == "AUDelay"));
    assert_eq!(list.sources.len(), 2);
    assert!(!list.is_unscanned());
    assert!(seen.iter().any(|p| p.current == "Cantina Test"));
    assert_eq!(seen.last().map(|p| p.done), Some(2));

    // A rescan with nothing changed examines nothing and needs no child:
    // a scanner pointing at a program that does not exist still succeeds.
    let lazy = Scanner::new("/nowhere/no-such-scanner", Vec::new());
    let again = lazy.scan(sources.clone(), list, false, |_| {}).unwrap();
    assert_eq!(again.examined, 0);
    assert_eq!(again.reused, 2);
    assert_eq!(again.children, 0);
    assert_eq!(again.list.plugins, list.plugins);
    assert_eq!(again.list.failed, list.failed);
    // A full rescan examines everything.
    assert!(lazy.scan(sources.clone(), list, true, |_| {}).is_err());

    // Touching the bundle changes its fingerprint, so only it is examined.
    let exe = bundle.join("Contents/MacOS/Cantina Test");
    let mut bytes = std::fs::read(&exe).unwrap();
    bytes.push(0);
    std::fs::write(&exe, bytes).unwrap();
    let changed = scan::discover_clap(std::slice::from_ref(&dir)).remove(0);
    assert_ne!(changed, clap);
    let sources = vec![changed, au_delay()];
    let partial = scanner().scan(sources, list, false, |_| {}).unwrap();
    assert_eq!(partial.examined, 1);
    assert_eq!(partial.reused, 1);
    assert_eq!(partial.list.plugins.len(), 2);
    assert_eq!(partial.list.failed.len(), 1);

    // A source that went away is dropped.
    let gone = scanner()
        .scan(vec![au_delay()], list, false, |_| {})
        .unwrap();
    assert_eq!(gone.list.plugins.len(), 1);
    assert!(gone.list.failed.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_hung_child_is_killed_and_blamed() {
    let mut scanner = Scanner::new("/bin/sleep", vec!["30".into()]);
    scanner.timeout = Duration::from_millis(300);
    let outcome = scanner
        .scan(vec![au_delay()], &PluginList::default(), false, |_| {})
        .unwrap();
    assert!(outcome.list.plugins.is_empty());
    assert_eq!(outcome.list.failed.len(), 1);
    assert!(
        outcome.list.failed[0].why.contains("did not respond"),
        "{}",
        outcome.list.failed[0].why
    );
}

#[test]
fn a_missing_scanner_is_an_error() {
    let scanner = Scanner::new("/nowhere/no-such-scanner", Vec::new());
    assert!(scanner
        .scan(vec![au_delay()], &PluginList::default(), false, |_| {})
        .is_err());
}
