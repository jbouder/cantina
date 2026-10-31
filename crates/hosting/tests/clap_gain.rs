//! Loads the workspace's own test plugin, built as a dylib, and checks the
//! CLAP wrapper behaves like a Processor.

mod common;

use cantina_dsp::{Buffer, Processor};
use cantina_hosting::clap::{self, ClapProcessor};
use common::fixture_path;

fn param_named(p: &dyn Processor, name: &str) -> usize {
    p.params()
        .iter()
        .position(|i| i.name == name)
        .unwrap_or_else(|| panic!("no parameter {name:?}"))
}

#[test]
fn fixture_is_listed() {
    let plugins = clap::list_plugins(&fixture_path()).unwrap();
    assert_eq!(plugins.len(), 2, "the gain and the crash plugin");
    assert_eq!(plugins[0].id, cantina_clap_gain::ID);
    assert_eq!(plugins[0].name, cantina_clap_gain::NAME);
    assert_eq!(plugins[0].vendor, cantina_clap_gain::VENDOR);
    assert_eq!(plugins[1].id, cantina_clap_gain::CRASH_ID);
}

#[test]
fn gain_and_delay_come_through_with_latency() {
    let path = fixture_path();
    let mut plugin = ClapProcessor::load(&path, Some(cantina_clap_gain::ID), 1).unwrap();
    assert_eq!(plugin.name(), cantina_clap_gain::NAME);
    assert_eq!(plugin.plugin_channels(), 2);

    let gain = param_named(&plugin, "Gain");
    let delay = param_named(&plugin, "Delay");
    assert_eq!(plugin.param(delay), 7.0);
    plugin.set_param(gain, -6.0206);
    plugin.set_param(delay, 11.0);
    plugin.prepare(48_000.0, 256);
    assert_eq!(plugin.latency(), 11, "latency follows the Delay parameter");

    let mut signal = vec![0.0f32; 512];
    signal[0] = 1.0;
    for block in signal.chunks_mut(256) {
        let mut channels = [block];
        let mut buffer = Buffer::new(&mut channels);
        plugin.process(&mut buffer);
    }
    assert_eq!(plugin.process_errors(), 0);
    assert!((signal[11] - 0.5).abs() < 1e-3, "{:?}", &signal[..16]);
    assert!(signal[..11].iter().all(|s| s.abs() < 1e-6));
    assert!(signal[12..].iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn parameter_changes_while_running_and_state_round_trip() {
    let path = fixture_path();
    let mut a = ClapProcessor::load(&path, None, 2).unwrap();
    let gain = param_named(&a, "Gain");
    let delay = param_named(&a, "Delay");
    a.set_param(delay, 0.0);
    a.prepare(44_100.0, 64);
    assert_eq!(a.latency(), 0);

    // Change the gain between blocks: the event rides along with the next
    // process call.
    let mut left = vec![1.0f32; 64];
    let mut right = vec![1.0f32; 64];
    a.set_param(gain, -20.0);
    {
        let mut channels = [&mut left[..], &mut right[..]];
        let mut buffer = Buffer::new(&mut channels);
        a.process(&mut buffer);
    }
    assert!((left[63] - 0.1).abs() < 1e-4, "{}", left[63]);
    assert!((right[63] - 0.1).abs() < 1e-4, "{}", right[63]);

    let blob = a.state();
    assert_eq!(blob.len(), 8, "the fixture saves two f32s");
    let mut b = ClapProcessor::load(&path, None, 2).unwrap();
    b.set_state(&blob).unwrap();
    assert!((b.param(gain) + 20.0).abs() < 1e-5);
    assert_eq!(b.param(delay), 0.0);
}
