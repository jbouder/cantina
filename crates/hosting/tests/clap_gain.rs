//! Loads the workspace's own test plugin, built as a dylib, and checks the
//! CLAP wrapper behaves like a Processor.

mod common;

use cantina_dsp::{Buffer, Processor, Unit};
use cantina_hosting::clap::{self, ClapHandle, ClapProcessor};
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

#[test]
fn units_come_from_the_plugin_formatting_its_values() {
    let plugin = ClapProcessor::load(&fixture_path(), None, 2).unwrap();
    let gain = param_named(&plugin, "Gain");
    let delay = param_named(&plugin, "Delay");
    assert_eq!(plugin.params()[gain].unit, Unit::Decibels, "\"0.0 dB\"");
    assert_eq!(
        plugin.params()[delay].unit,
        Unit::Generic,
        "\"7 samples\" is no unit and the name says nothing"
    );
}

#[test]
fn the_handle_saves_state_while_active_and_hears_the_plugin_report_changes() {
    let path = fixture_path();
    let mut processor = ClapProcessor::load(&path, None, 2).unwrap();
    let gain = param_named(&processor, "Gain");
    let delay = param_named(&processor, "Delay");
    let mut handle = processor.handle().expect("a plugin has a handle");
    assert!(processor.handle().is_none(), "the handle is taken once");
    processor.set_param(delay, 3.0);
    processor.prepare(48_000.0, 64);
    assert_eq!(processor.latency(), 3);
    assert_eq!(handle.latency(), 3, "the handle reads it live");
    assert!(handle.take_changes().is_empty());

    // A change sent to the running plugin is echoed by the fixture as an
    // output event, which the handle collects on the UI thread.
    processor.set_param(gain, -12.0);
    let mut left = vec![1.0f32; 64];
    let mut right = vec![1.0f32; 64];
    {
        let mut channels = [&mut left[..], &mut right[..]];
        processor.process(&mut Buffer::new(&mut channels));
    }
    let changes = handle.take_changes();
    assert_eq!(changes, vec![(gain, -12.0)]);
    assert!(handle.take_changes().is_empty());
    let clap: &ClapHandle = handle
        .as_any()
        .downcast_ref::<ClapHandle>()
        .expect("the handle is a ClapHandle");
    assert_eq!(clap.param(gain), -12.0);
    assert!(!clap.latency_changed());

    // State through the handle while the plugin is active, into a fresh
    // instance; the processor's own state agrees.
    let blob = handle.state();
    assert_eq!(blob.len(), 8);
    assert_eq!(processor.state(), blob);
    let mut fresh = ClapProcessor::load(&path, None, 2).unwrap();
    let mut fresh_handle = fresh.handle().unwrap();
    fresh_handle.set_state(&blob).unwrap();
    assert_eq!(fresh.param(gain), 0.0, "the processor's cache is its own");
    assert_eq!(
        fresh_handle
            .as_any()
            .downcast_ref::<ClapHandle>()
            .unwrap()
            .param(gain),
        -12.0
    );
    fresh.set_state(&blob).unwrap();
    assert_eq!(fresh.param(gain), -12.0);
    assert_eq!(fresh.param(delay), 3.0);
    assert!(fresh_handle.set_state(&blob[..3]).is_err());

    // The processor can go first; the handle keeps the instance alive.
    drop(processor);
    assert_eq!(handle.state(), blob);
}
