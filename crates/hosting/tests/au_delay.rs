//! Loads Apple's AUDelay, which ships with every Mac, and checks it behaves
//! like a Processor: parameters, latency, and audio that comes out delayed.

use cantina_dsp::{Buffer, Processor};
use cantina_hosting::au::{self, AudioUnitProcessor};

fn param_named(p: &dyn Processor, needle: &str) -> usize {
    p.params()
        .iter()
        .position(|i| i.name.to_lowercase().contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "no parameter containing {needle:?} in {:?}",
                p.params()
                    .iter()
                    .map(|i| i.name.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn apple_effects_are_listed() {
    let effects = au::list_effects();
    assert!(
        effects
            .iter()
            .any(|c| c.is_apple() && c.name.contains("AUDelay")),
        "{effects:#?}"
    );
    let found = au::find("aufx:dely:appl").unwrap();
    assert!(found.name.contains("AUDelay"));
    assert!(au::find("no such plugin anywhere").is_err());
}

#[test]
fn au_delay_delays_an_impulse() {
    let sample_rate = 48_000.0;
    let info = au::find("aufx:dely:appl").unwrap();
    let mut delay = AudioUnitProcessor::new(info, 1).unwrap();
    assert_eq!(delay.name(), "AUDelay");
    assert!(!delay.params().is_empty());

    let wet = param_named(&delay, "mix");
    let time = param_named(&delay, "delay time");
    let feedback = param_named(&delay, "feedback");
    let cutoff = param_named(&delay, "cutoff");
    delay.set_param(wet, 100.0);
    delay.set_param(time, 0.1);
    delay.set_param(feedback, 0.0);
    delay.set_param(cutoff, delay.params()[cutoff].max);
    assert!((delay.param(time) - 0.1).abs() < 1e-6);

    delay.prepare(sample_rate, 512);
    assert!(delay.latency() < sample_rate as usize);
    assert_eq!(delay.render_errors(), 0);

    let total = 12_000;
    let mut signal = vec![0.0f32; total];
    signal[0] = 1.0;
    for block in signal.chunks_mut(512) {
        let mut channels = [block];
        let mut buffer = Buffer::new(&mut channels);
        delay.process(&mut buffer);
    }
    assert_eq!(delay.render_errors(), 0);

    let (peak_at, peak) = signal
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |best, (i, &x)| {
            if x.abs() > best.1 {
                (i, x.abs())
            } else {
                best
            }
        });
    let expected = (0.1 * sample_rate) as usize;
    assert!(
        peak_at.abs_diff(expected) <= 16,
        "peak at {peak_at}, expected about {expected}"
    );
    assert!(peak > 0.1, "peak {peak}");
    let early: f32 = signal[64..expected - 64].iter().map(|x| x.abs()).sum();
    assert!(early < 0.01, "energy before the delay time: {early}");
}

#[test]
fn stereo_configuration_and_state_round_trip() {
    let info = au::find("aufx:dely:appl").unwrap();
    let mut a = AudioUnitProcessor::new(info.clone(), 2).unwrap();
    a.prepare(44_100.0, 256);
    assert_eq!(a.render_errors(), 0);
    let time = param_named(&a, "delay time");
    a.set_param(time, 0.25);
    let blob = a.state();

    let mut b = AudioUnitProcessor::new(info, 2).unwrap();
    b.set_state(&blob).unwrap();
    assert!((b.param(time) - 0.25).abs() < 1e-6);
}
