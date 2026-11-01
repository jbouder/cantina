use std::f32::consts::TAU;

use super::*;
use crate::{Biquad, Buffer, Gain, Processor, Renderer, Transport, Unit};

fn sine(hz: f32, n: usize, amplitude: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amplitude * (TAU * hz * i as f32 / 48_000.0).sin())
        .collect()
}

fn impulse(n: usize) -> Vec<f32> {
    let mut v = vec![0.0; n];
    v[0] = 1.0;
    v
}

fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|x| x * x).sum::<f32>() / signal.len() as f32).sqrt()
}

fn render(def: &PedalDef, input: &[f32]) -> Vec<f32> {
    let mut pedal = compile(def).unwrap_or_else(|e| panic!("{e}"));
    Renderer::default().render(&mut pedal, &[input.to_vec()])[0].clone()
}

fn problems(def: &PedalDef) -> Vec<Problem> {
    match def.validate() {
        Ok(()) => panic!("{} validated", def.name),
        Err(e) => e.problems(),
    }
}

/// `[from, to]` pairs from a path of ids.
fn chain(ids: &[&str]) -> Vec<(String, String)> {
    ids.windows(2)
        .map(|w| (w[0].to_string(), w[1].to_string()))
        .collect()
}

/// The drive from the plan, with the drive knob in decibels.
fn warm_drive() -> PedalDef {
    PedalDef {
        version: PEDAL_VERSION,
        name: "Warm Drive".into(),
        params: vec![ParamDef::new("drive", "Drive", "dB", 0.0, 30.0, 12.0)],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("pre", "biquad")
                .with("type", "highpass")
                .with("freq", 80.0),
            NodeDef::new("gain", "gain").bound("db", Binding::param("drive")),
            NodeDef::new("clip", "waveshaper")
                .with("curve", "soft")
                .with("oversample", 4.0),
            NodeDef::new("tone", "biquad")
                .with("type", "lowpass")
                .with("freq", 4500.0),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "pre", "gain", "clip", "tone", "out"]),
    }
}

const PLAN_JSON: &str = r#"{
  "version": 1,
  "name": "Warm Drive",
  "params": [{ "id": "drive", "label": "Drive", "unit": "dB", "min": 0, "max": 30, "default": 12 }],
  "nodes": [
    { "id": "in", "kind": "input" },
    { "id": "pre", "kind": "biquad", "type": "highpass", "freq": 80 },
    { "id": "gain", "kind": "gain", "db": { "param": "drive" } },
    { "id": "clip", "kind": "waveshaper", "curve": "soft", "oversample": 4 },
    { "id": "tone", "kind": "biquad", "type": "lowpass", "freq": 4500 },
    { "id": "out", "kind": "output" }
  ],
  "edges": [["in", "pre"], ["pre", "gain"], ["gain", "clip"], ["clip", "tone"], ["tone", "out"]]
}"#;

#[test]
fn the_plans_pedal_loads_validates_and_processes_audio() {
    let def = PedalDef::from_json(PLAN_JSON).unwrap();
    assert_eq!(def, warm_drive());
    let mut pedal = compile(&def).unwrap();
    assert_eq!(pedal.name(), "Warm Drive");
    assert_eq!(pedal.params().len(), 1);
    assert_eq!(pedal.params()[0].name, "Drive");
    assert_eq!(pedal.params()[0].unit, Unit::Decibels);
    assert_eq!(pedal.param(0), 12.0);
    assert_eq!(pedal.latency(), 22);
    pedal.set_param(0, 30.0);
    let input = sine(220.0, 9600, 0.5);
    let out = Renderer::default().render(&mut pedal, std::slice::from_ref(&input))[0].clone();
    assert!(out.iter().all(|s| s.is_finite()));
    let peak = out[4800..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.8 && peak < 1.2, "driven peak {peak}");
    // Clipped: the rms is closer to the peak than a sine's is.
    assert!(
        rms(&out[4800..]) / peak > 0.75,
        "not clipped: {}",
        rms(&out[4800..]) / peak
    );
    // Silence stays silent.
    let quiet = Renderer::default().render(&mut pedal, &[vec![0.0; 2048]]);
    assert!(quiet[0].iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn json_round_trips_and_an_empty_pedal_passes_audio_through() {
    let def = warm_drive();
    let text = def.to_json();
    assert!(
        text.contains("\"freq\": 80\n") || text.contains("\"freq\": 80,"),
        "{text}"
    );
    assert_eq!(PedalDef::from_json(&text).unwrap(), def);
    let empty = PedalDef::empty("Nothing");
    let input = sine(440.0, 256, 0.5);
    assert_eq!(render(&empty, &input), input);
    assert_eq!(compile(&empty).unwrap().latency(), 0);
    assert!(compile(&empty).unwrap().params().is_empty());
}

#[test]
fn not_json_and_wrong_shapes_are_reported_as_such() {
    let err = PedalDef::from_json("{").unwrap_err();
    assert!(matches!(err, PedalError::Json(_)), "{err}");
    let err = PedalDef::from_json(r#"{"name": "x"}"#).unwrap_err();
    assert!(err.to_string().contains("version"), "{err}");
}

fn has(problems: &[Problem], node: Option<&str>, text: &str) -> bool {
    problems
        .iter()
        .any(|p| p.node.as_deref() == node && p.message.contains(text))
}

#[test]
fn validation_names_the_node_behind_each_problem() {
    let mut def = warm_drive();
    def.nodes[3].kind = "wavshaper".into();
    def.edges.push(("tone".into(), "nowhere".into()));
    let p = problems(&def);
    assert!(has(&p, Some("clip"), "unknown kind \"wavshaper\""), "{p:?}");
    assert!(
        has(
            &p,
            None,
            "edge \"tone\" to \"nowhere\": no node \"nowhere\""
        ),
        "{p:?}"
    );
    // Everything is reported at once.
    assert_eq!(p.len(), 2, "{p:?}");

    let mut def = warm_drive();
    def.edges.push(("tone".into(), "pre".into()));
    let p = problems(&def);
    assert!(has(&p, Some("pre"), "has more than one input"), "{p:?}");
    assert!(
        p.iter().any(|q| q.message.contains("a cycle runs through")),
        "{p:?}"
    );
    let cycle = p.iter().find(|q| q.message.contains("cycle")).unwrap();
    assert!(
        cycle.message.contains("\"pre\"") && cycle.message.contains("\"tone\""),
        "{p:?}"
    );

    // A cycle through a signal binding counts too.
    let mut def = warm_drive();
    def.nodes[2] = NodeDef::new("gain", "gain").bound("linear", Binding::signal("tone", 0.0, 1.0));
    let p = problems(&def);
    assert!(
        p.iter().any(|q| q.message.contains("a cycle runs through")),
        "{p:?}"
    );
}

#[test]
fn structural_rules_are_checked() {
    // Two outputs, no input.
    let mut def = warm_drive();
    def.nodes[0].kind = "output".into();
    let p = problems(&def);
    assert!(has(&p, None, "there is no input node"), "{p:?}");
    assert!(has(&p, None, "there are 2 output nodes"), "{p:?}");

    // Duplicate ids.
    let mut def = warm_drive();
    def.nodes[4].id = "pre".into();
    let p = problems(&def);
    assert!(has(&p, Some("pre"), "two nodes have this id"), "{p:?}");

    // An lfo as audio, an edge into the input, a node fed by nothing,
    // a node feeding nothing.
    let def = PedalDef {
        version: 1,
        name: "Loose".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("lfo", "lfo"),
            NodeDef::new("g", "gain"),
            NodeDef::new("lonely", "biquad"),
            NodeDef::new("dead", "split"),
            NodeDef::new("out", "output"),
        ],
        edges: vec![
            ("lfo".into(), "g".into()),
            ("g".into(), "in".into()),
            ("in".into(), "dead".into()),
            ("g".into(), "out".into()),
            ("g".into(), "out".into()),
        ],
    };
    let p = problems(&def);
    assert!(has(&p, Some("lfo"), "drives fields, not audio"), "{p:?}");
    assert!(has(&p, Some("in"), "the input takes no input"), "{p:?}");
    assert!(has(&p, Some("g"), "is wired to \"out\" twice"), "{p:?}");
    assert!(has(&p, Some("g"), "has no input"), "{p:?}");
    assert!(has(&p, Some("lonely"), "has no input"), "{p:?}");
    assert!(has(&p, Some("lonely"), "feeds nothing"), "{p:?}");
    assert!(has(&p, Some("dead"), "feeds nothing"), "{p:?}");
    assert!(has(&p, Some("lfo"), "feeds nothing"), "{p:?}");

    // Version, name and parameter rules.
    let mut def = warm_drive();
    def.version = 2;
    def.name = " ".into();
    def.params
        .push(ParamDef::new("drive", "Again", "parsecs", 5.0, 1.0, 9.0));
    def.params.push(ParamDef {
        toggle: true,
        ..ParamDef::choice("both", "Both", &["a", ""], 0)
    });
    let p = problems(&def);
    assert!(
        has(
            &p,
            None,
            "version 2 is not the version this build reads (1)"
        ),
        "{p:?}"
    );
    assert!(has(&p, None, "the pedal has no name"), "{p:?}");
    assert!(
        has(&p, None, "two parameters are called \"drive\""),
        "{p:?}"
    );
    assert!(has(&p, None, "unit \"parsecs\""), "{p:?}");
    assert!(has(&p, None, "min 5 must be below max 1"), "{p:?}");
    assert!(has(&p, None, "default 9 is outside"), "{p:?}");
    assert!(has(&p, None, "is both a choice and a toggle"), "{p:?}");
    assert!(has(&p, None, "has an empty choice"), "{p:?}");
}

#[test]
fn field_rules_are_checked_with_the_field_named() {
    let mut def = warm_drive();
    def.params
        .push(ParamDef::new("wide", "Wide", "%", 0.0, 100.0, 50.0));
    def.params
        .push(ParamDef::choice("pick", "Pick", &["a", "b"], 0));
    def.nodes[1] = NodeDef::new("pre", "biquad")
        .with("type", "bandpass")
        .with("freq", 5.0)
        .with("q", "high")
        .with("gian", 3.0)
        .with("smooth", Value::Bound(Binding::param("wide")));
    def.nodes[2] = NodeDef::new("gain", "gain")
        .with("db", 1.0)
        .bound("linear", Binding::param("wide"));
    def.nodes[3] = NodeDef::new("clip", "waveshaper")
        .with("oversample", 3.0)
        .bound("curve", Binding::signal("tone", 0.0, 1.0));
    def.nodes[4] = NodeDef::new("tone", "biquad")
        .bound("freq", Binding::param("missing"))
        .bound("gain", Binding::param("wide").log())
        .bound(
            "q",
            Binding {
                param: Some("wide".into()),
                node: Some("pre".into()),
                ..Binding::default()
            },
        );
    let p = problems(&def);
    let field = |node: &str, field: &str, text: &str| {
        assert!(
            p.iter().any(|q| q.node.as_deref() == Some(node)
                && q.field.as_deref() == Some(field)
                && q.message.contains(text)),
            "no problem for {node}.{field} containing {text:?} in {p:#?}"
        );
    };
    field(
        "pre",
        "type",
        "\"bandpass\" is not one of lowpass, highpass, peak",
    );
    field("pre", "freq", "5 is out of range; freq takes 20 to 20000");
    field("pre", "q", "takes a number");
    field("pre", "gian", "no such field on a biquad");
    field("pre", "smooth", "takes a plain number");
    field("gain", "linear", "set db or linear, not both");
    field(
        "gain",
        "linear",
        "parameter \"wide\" runs 0 to 100 but linear takes 0 to 16; add a map",
    );
    field("clip", "oversample", "takes 1, 2, 4 or 8");
    field("clip", "curve", "cannot follow a signal");
    field("tone", "freq", "no parameter \"missing\"");
    field("tone", "gain", "log needs a map");
    field("tone", "q", "a binding follows a param or a node, not both");

    // Signal bindings only on modulatable fields, and not to oneself.
    let mut def = warm_drive();
    def.nodes[4] = NodeDef::new("tone", "biquad")
        .bound("q", Binding::signal("pre", 0.5, 1.0))
        .bound("freq", Binding::signal("tone", 100.0, 1000.0))
        .bound("gain", Binding::table("drive", [Value::Name("x".into())]));
    let p = problems(&def);
    assert!(has(&p, Some("tone"), "cannot follow a signal"), "{p:?}");
    assert!(
        has(&p, Some("tone"), "a node cannot follow itself"),
        "{p:?}"
    );
    assert!(
        has(&p, Some("tone"), "values must be numbers here"),
        "{p:?}"
    );

    // Choice tables take names from the options; a bare choice must fit.
    let mut def = warm_drive();
    def.params
        .push(ParamDef::choice("curve", "Curve", &["A", "B", "C"], 0));
    def.params
        .push(ParamDef::new("big", "Big", "", 0.0, 9.0, 0.0));
    def.nodes[3] = NodeDef::new("clip", "waveshaper")
        .bound(
            "curve",
            Binding::table("curve", ["soft".into(), "loud".into()]),
        )
        .bound(
            "oversample",
            Binding::table("big", [2.0.into(), 3.0.into()]),
        );
    def.nodes
        .push(NodeDef::new("c2", "waveshaper").bound("curve", Binding::param("big")));
    def.edges = chain(&["in", "pre", "gain", "clip", "c2", "tone", "out"]);
    let p = problems(&def);
    assert!(
        has(&p, Some("clip"), "value \"loud\" is not one of"),
        "{p:?}"
    );
    assert!(
        has(&p, Some("clip"), "values must each be 1, 2, 4 or 8"),
        "{p:?}"
    );
    assert!(
        has(&p, Some("c2"), "runs past the 7 options; add values"),
        "{p:?}"
    );
}

#[test]
fn a_gain_and_a_filter_match_the_stock_processors_exactly() {
    let def = PedalDef {
        version: 1,
        name: "Lowpassed".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("g", "gain").with("db", -6.0),
            NodeDef::new("f", "biquad")
                .with("type", "lowpass")
                .with("freq", 1000.0)
                .with("q", 0.5),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "g", "f", "out"]),
    };
    let input = sine(300.0, 4096, 0.5);
    let out = render(&def, &input);
    let mut expected =
        Renderer::default().render(&mut Gain::new(-6.0), std::slice::from_ref(&input))[0].clone();
    let mut filter = Biquad::identity();
    filter.set_lowpass(48_000.0, 1000.0, 0.5);
    filter.process(&mut expected);
    assert_eq!(out, expected);
}

#[test]
fn a_bound_gain_eases_to_a_new_value_without_steps() {
    let def = PedalDef {
        version: 1,
        name: "Level".into(),
        params: vec![ParamDef::new("level", "Level", "dB", -60.0, 24.0, 0.0)],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("g", "gain").bound("db", Binding::param("level")),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "g", "out"]),
    };
    let mut pedal = compile(&def).unwrap();
    pedal.prepare(48_000.0, 4800);
    pedal.set_param(0, -20.0);
    let mut buffer = vec![1.0f32; 4800];
    let mut channels = [&mut buffer[..]];
    let mut b = Buffer::new(&mut channels);
    pedal.process(&mut b);
    assert!((buffer[4799] - 0.1).abs() < 1e-3, "{}", buffer[4799]);
    let max_step = buffer
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(max_step < 0.01, "largest step {max_step}");
    assert!(buffer.windows(2).all(|w| w[1] <= w[0]));
    // Unknown ids are ignored and values clamp.
    pedal.set_param(7, 1.0);
    pedal.set_param(0, 100.0);
    assert_eq!(pedal.param(0), 24.0);
    assert_eq!(pedal.param(7), 0.0);
    // The default state blob round-trips.
    let blob = pedal.state();
    let mut other = compile(&def).unwrap();
    other.set_state(&blob).unwrap();
    assert_eq!(other.param(0), 24.0);
}

#[test]
fn maps_tables_and_log_maps_shape_a_parameter() {
    // A linear map: a 0..1 knob scales from silent to double.
    let def = PedalDef {
        version: 1,
        name: "Mapped".into(),
        params: vec![
            ParamDef::new("amount", "Amount", "", 0.0, 1.0, 0.5),
            ParamDef::choice("curve", "Curve", &["Brick", "Soft"], 0),
            ParamDef::new("spread", "Spread", "", 0.0, 1.0, 0.5),
        ],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("g", "gain")
                .bound("linear", Binding::mapped("amount", 0.0, 2.0))
                .with("smooth", 0.0),
            NodeDef::new("clip", "waveshaper")
                .bound(
                    "curve",
                    Binding::table("curve", ["hard".into(), "soft".into()]),
                )
                .with("oversample", 1.0),
            NodeDef::new("d", "delay")
                .bound("time", Binding::mapped("spread", 1.0, 16.0).log())
                .with("smooth", 0.0),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "g", "clip", "d", "out"]),
    };
    let mut pedal = compile(&def).unwrap();
    assert_eq!(pedal.latency(), 0);
    assert_eq!(pedal.params()[1].unit, Unit::Choice(&["Brick", "Soft"]));
    // amount 0.5 -> x1, hard clip leaves 0.75 alone, log map at the
    // middle of 1..16 ms is 4 ms: 192 samples.
    let out = Renderer::default().render(&mut pedal, &[vec![0.75; 400]])[0].clone();
    assert!(out[..192].iter().all(|s| *s == 0.0), "{:?}", &out[185..200]);
    assert!((out[192] - 0.75).abs() < 1e-6, "{}", out[192]);
    // amount 1 -> x2, hard clip -> 1.
    pedal.set_param(0, 1.0);
    let out = Renderer::default().render(&mut pedal, &[vec![0.75; 400]])[0].clone();
    assert_eq!(out[192], 1.0);
    // The soft curve rounds it instead.
    pedal.set_param(1, 1.0);
    let out = Renderer::default().render(&mut pedal, &[vec![0.75; 400]])[0].clone();
    assert!((out[192] - 1.5f32.tanh()).abs() < 1e-6, "{}", out[192]);
}

#[test]
fn a_delay_delays_by_its_time_and_feeds_back() {
    let def = PedalDef {
        version: 1,
        name: "Slap".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("d", "delay")
                .with("time", 10.0)
                .with("feedback", 0.5)
                .with("mix", 0.5)
                .with("max", 100.0),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "d", "out"]),
    };
    let out = render(&def, &impulse(2000));
    assert!((out[0] - 0.5).abs() < 1e-6, "dry {}", out[0]);
    assert!((out[480] - 0.5).abs() < 1e-6, "first repeat {}", out[480]);
    assert!((out[960] - 0.25).abs() < 1e-6, "second repeat {}", out[960]);
    assert!(
        (out[1440] - 0.125).abs() < 1e-6,
        "third repeat {}",
        out[1440]
    );
    let others: f32 = out
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 480 != 0)
        .map(|(_, s)| s.abs())
        .sum();
    assert!(others < 1e-5, "{others}");
}

fn tremolo(sync: bool) -> PedalDef {
    PedalDef {
        version: 1,
        name: "Trem".into(),
        params: vec![ParamDef::new("depth", "Depth", "%", 0.0, 100.0, 100.0)],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("lfo", "lfo")
                .with("rate", 10.0)
                .with("sync", sync)
                .with("note", "1/4")
                .with("unipolar", true)
                // Read at the trough: the gain is full at the start of
                // a cycle and dips by the depth half way through.
                .with("phase", 0.75)
                .bound("depth", Binding::mapped("depth", 0.0, 1.0)),
            NodeDef::new("vca", "gain").bound("linear", Binding::signal("lfo", 1.0, 0.0)),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "vca", "out"]),
    }
}

#[test]
fn an_lfo_drives_a_gain_as_a_tremolo() {
    // Full depth, unipolar sine read at its trough: the first sample is
    // full level, a half cycle later (2400 samples at 10 Hz) silent.
    let out = render(&tremolo(false), &vec![1.0; 9600]);
    assert!((out[0] - 1.0).abs() < 1e-5, "{}", out[0]);
    assert!(out[2400].abs() < 1e-4, "{}", out[2400]);
    assert!((out[4800] - 1.0).abs() < 1e-4, "{}", out[4800]);
    // Half depth dips half way.
    let mut pedal = compile(&tremolo(false)).unwrap();
    pedal.set_param(0, 50.0);
    let out = Renderer::default().render(&mut pedal, &[vec![1.0; 9600]])[0].clone();
    assert!((out[2400] - 0.5).abs() < 1e-3, "{}", out[2400]);
}

#[test]
fn a_synced_lfo_follows_the_transport() {
    let mut pedal = compile(&tremolo(true)).unwrap();
    pedal.prepare(48_000.0, 64);
    // Stopped at 120 bpm a quarter note is 2 Hz: a dip 12 000 samples in.
    let mut out = vec![1.0f32; 24_000];
    let mut channels = vec![out.clone()];
    pedal.set_transport(&Transport {
        bpm: 120.0,
        beats: 0.0,
        playing: false,
    });
    Renderer::default().run(&mut channels, |b| pedal.process(b));
    out.copy_from_slice(&channels[0]);
    assert!(out[12_000].abs() < 1e-3, "{}", out[12_000]);
    assert!((out[23_999] - 1.0).abs() < 1e-2, "{}", out[23_999]);
    // Playing, the phase is locked to the beat: a block starting half a
    // beat in is at the dip at once.
    let mut pedal = compile(&tremolo(true)).unwrap();
    pedal.prepare(48_000.0, 64);
    pedal.set_transport(&Transport {
        bpm: 120.0,
        beats: 0.5,
        playing: true,
    });
    let mut block = vec![1.0f32; 64];
    let mut channels = [&mut block[..]];
    pedal.process(&mut Buffer::new(&mut channels));
    assert!(block[0].abs() < 1e-3, "{}", block[0]);
}

#[test]
fn stereo_spread_offsets_the_right_channel() {
    let def = PedalDef {
        version: 1,
        name: "Wide".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("lfo", "lfo")
                .with("rate", 1.0)
                .with("shape", "triangle")
                .with("spread", 0.5),
            NodeDef::new("vca", "gain").bound("linear", Binding::signal("lfo", 0.0, 1.0)),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "vca", "out"]),
    };
    let mut pedal = compile(&def).unwrap();
    let out = Renderer::default().render(&mut pedal, &[vec![1.0; 48_000], vec![1.0; 48_000]]);
    for i in (0..48_000).step_by(1000) {
        assert!(
            (out[0][i] + out[1][i]).abs() < 1e-5,
            "{i}: {} {}",
            out[0][i],
            out[1][i]
        );
    }
    // A triangle peaks half way through its cycle.
    assert!((out[0][24_000] - 1.0).abs() < 1e-3, "{}", out[0][24_000]);
}

#[test]
fn waveshaper_latency_depends_on_oversampling_and_the_pedal_takes_the_longest_path() {
    let shaper = |oversample: Value| PedalDef {
        version: 1,
        name: "Clip".into(),
        params: vec![ParamDef::choice(
            "os",
            "Oversampling",
            &["Off", "2×", "4×", "8×"],
            2,
        )],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("clip", "waveshaper").with("oversample", oversample),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "clip", "out"]),
    };
    assert_eq!(compile(&shaper(1.0.into())).unwrap().latency(), 0);
    assert_eq!(compile(&shaper(2.0.into())).unwrap().latency(), 22);
    assert_eq!(compile(&shaper(8.0.into())).unwrap().latency(), 22);
    let bound = shaper(Value::Bound(Binding::table(
        "os",
        [1.0.into(), 2.0.into(), 4.0.into(), 8.0.into()],
    )));
    let mut pedal = compile(&bound).unwrap();
    assert_eq!(pedal.latency(), 22);
    // Switching the factor moves nothing in time.
    pedal.prepare(48_000.0, 64);
    for index in [0.0, 1.0, 3.0, 2.0] {
        pedal.set_param(0, index);
        let out = Renderer::default().render(&mut pedal, &[impulse(256)])[0].clone();
        let at = out.iter().position(|s| s.abs() > 0.1).unwrap();
        assert_eq!(at, 22, "factor index {index}");
    }

    // Parallel branches: the shaped one is 22 late, the dry one not;
    // the pedal reports the longer.
    let def = PedalDef {
        version: 1,
        name: "Parallel".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("tee", "split"),
            NodeDef::new("clip", "waveshaper").with("oversample", 4.0),
            NodeDef::new("dry", "gain").with("db", -6.0206),
            NodeDef::new("sum", "mix"),
            NodeDef::new("out", "output"),
        ],
        edges: vec![
            ("in".into(), "tee".into()),
            ("tee".into(), "clip".into()),
            ("tee".into(), "dry".into()),
            ("clip".into(), "sum".into()),
            ("dry".into(), "sum".into()),
            ("sum".into(), "out".into()),
        ],
    };
    let mut pedal = compile(&def).unwrap();
    assert_eq!(pedal.latency(), 22);
    let out = Renderer::default().render(&mut pedal, &[impulse(256)])[0].clone();
    assert!((out[0] - 0.5).abs() < 1e-5, "dry {}", out[0]);
    assert!(out[22].abs() > 0.3, "shaped {}", out[22]);
}

#[test]
fn mix_sums_and_split_passes() {
    let def = PedalDef {
        version: 1,
        name: "Sum".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("tee", "split"),
            NodeDef::new("a", "gain"),
            NodeDef::new("b", "gain").with("linear", 0.5),
            NodeDef::new("sum", "mix"),
            NodeDef::new("out", "output"),
        ],
        edges: vec![
            ("in".into(), "tee".into()),
            ("tee".into(), "a".into()),
            ("tee".into(), "b".into()),
            ("a".into(), "sum".into()),
            ("b".into(), "sum".into()),
            ("sum".into(), "out".into()),
        ],
    };
    let input = sine(100.0, 300, 0.4);
    let out = render(&def, &input);
    for (o, x) in out.iter().zip(&input) {
        assert!((o - x * 1.5).abs() < 1e-6);
    }
}

#[test]
fn an_allpass_is_flat_in_level_and_a_swept_filter_follows_a_signal() {
    let def = PedalDef {
        version: 1,
        name: "Shift".into(),
        params: vec![],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("ap", "allpass")
                .with("stages", 4.0)
                .with("freq", 1000.0),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "ap", "out"]),
    };
    let input = sine(1000.0, 24_000, 0.5);
    let out = render(&def, &input);
    let (a, b) = (rms(&out[12_000..]), rms(&input[12_000..]));
    assert!((a / b - 1.0).abs() < 0.01, "{a} vs {b}");
    assert!(out[12_000..] != input[12_000..], "the phase should move");

    let mut swept = def.clone();
    swept.nodes.insert(
        1,
        NodeDef::new("lfo", "lfo")
            .with("rate", 2.0)
            .with("unipolar", true),
    );
    swept.nodes[2] = NodeDef::new("ap", "allpass")
        .with("stages", 4.0)
        .with("feedback", 0.5)
        .with("mix", 0.5)
        .bound("freq", Binding::signal("lfo", 300.0, 6000.0).log());
    let phased = render(&swept, &input);
    assert!(phased.iter().all(|s| s.is_finite()));
    assert!(phased[12_000..] != out[12_000..]);

    let mut filtered = def.clone();
    filtered.nodes.insert(
        1,
        NodeDef::new("lfo", "lfo")
            .with("rate", 2.0)
            .with("unipolar", true),
    );
    filtered.nodes[2] = NodeDef::new("ap", "biquad")
        .with("type", "lowpass")
        .bound("freq", Binding::signal("lfo", 200.0, 8000.0).log());
    let wah = render(&filtered, &input);
    assert!(wah.iter().all(|s| s.is_finite()));
    // At 2 Hz the sine peaks an eighth of a second in and troughs at
    // three eighths: opened wide the tone passes, closed down it is cut.
    let open = rms(&wah[6000..6200]);
    let closed = rms(&wah[18_000..18_200]);
    assert!(open > closed * 3.0, "open {open} closed {closed}");
}

#[test]
fn blocks_larger_than_prepared_for_are_bypassed() {
    let mut pedal = compile(&warm_drive()).unwrap();
    let mut block = sine(440.0, 128, 0.5);
    let original = block.clone();
    let mut channels = [&mut block[..]];
    pedal.process(&mut Buffer::new(&mut channels));
    assert_eq!(block, original, "unprepared");
    pedal.prepare(48_000.0, 64);
    let mut channels = [&mut block[..]];
    pedal.process(&mut Buffer::new(&mut channels));
    assert_eq!(block, original, "too large");
    let mut channels = [&mut block[..64]];
    pedal.process(&mut Buffer::new(&mut channels));
    assert_ne!(block[..64], original[..64]);
}

#[test]
fn choice_lists_are_interned_once() {
    let def = PedalDef {
        version: 1,
        name: "Pick".into(),
        params: vec![ParamDef::choice("c", "C", &["One", "Two"], 1)],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("g", "gain")
                .bound("linear", Binding::table("c", [1.0.into(), 2.0.into()])),
            NodeDef::new("out", "output"),
        ],
        edges: chain(&["in", "g", "out"]),
    };
    let a = compile(&def).unwrap();
    let b = compile(&def).unwrap();
    let (Unit::Choice(x), Unit::Choice(y)) = (a.params()[0].unit, b.params()[0].unit) else {
        panic!("not choices");
    };
    assert!(std::ptr::eq(x, y));
    assert_eq!(x, &["One", "Two"]);
    assert_eq!(a.params()[0].default, 1.0);
    assert_eq!(render(&def, &[0.5])[0], 1.0);
}

#[test]
fn the_catalog_lists_every_kind() {
    let names: Vec<&str> = kinds().iter().map(|k| k.kind).collect();
    assert_eq!(
        names,
        [
            "input",
            "output",
            "gain",
            "biquad",
            "waveshaper",
            "delay",
            "lfo",
            "allpass",
            "mix",
            "split"
        ]
    );
    let lfo = kinds().iter().find(|k| k.kind == "lfo").unwrap();
    assert!(lfo.source && lfo.control);
    let FieldKind::Name { options, .. } =
        lfo.fields.iter().find(|f| f.name == "note").unwrap().kind
    else {
        panic!()
    };
    assert_eq!(options[3], "1/4");
}
