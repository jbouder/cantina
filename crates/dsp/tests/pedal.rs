//! A compiled pedal allocates nothing while it runs. Every buffer is
//! reserved in `prepare`; `process`, `set_param` and `set_transport`
//! only walk them. This test counts allocations through a wrapping
//! global allocator while a pedal using every kind of node, with bound
//! parameters and signals, processes a few hundred blocks.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use cantina_dsp::pedal::{Binding, NodeDef, ParamDef, PedalDef, PEDAL_VERSION};
use cantina_dsp::{compile, Buffer, Processor, Transport};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn everything() -> PedalDef {
    PedalDef {
        version: PEDAL_VERSION,
        name: "Everything".into(),
        params: vec![
            ParamDef::new("drive", "Drive", "dB", 0.0, 40.0, 20.0),
            ParamDef::new("tone", "Tone", "Hz", 500.0, 8000.0, 3000.0),
            ParamDef::choice("curve", "Curve", &["Soft", "Hard"], 0),
            ParamDef::choice("os", "Oversampling", &["Off", "2×", "4×", "8×"], 2),
            ParamDef::new("rate", "Rate", "Hz", 0.1, 10.0, 1.0),
            ParamDef::toggle("sync", "Sync", false),
            ParamDef::new("depth", "Depth", "%", 0.0, 100.0, 50.0),
            ParamDef::new("mix", "Mix", "%", 0.0, 100.0, 50.0),
        ],
        nodes: vec![
            NodeDef::new("in", "input"),
            NodeDef::new("tee", "split"),
            NodeDef::new("pre", "biquad")
                .with("type", "highpass")
                .with("freq", 80.0),
            NodeDef::new("gain", "gain")
                .bound("db", Binding::param("drive"))
                .with("smooth", 0.01),
            NodeDef::new("clip", "waveshaper")
                .bound(
                    "curve",
                    Binding::table("curve", ["soft".into(), "hard".into()]),
                )
                .bound(
                    "oversample",
                    Binding::table("os", [1.0.into(), 2.0.into(), 4.0.into(), 8.0.into()]),
                ),
            NodeDef::new("tone", "biquad")
                .with("type", "lowpass")
                .bound("freq", Binding::param("tone"))
                .with("smooth", 0.01),
            NodeDef::new("lfo", "lfo")
                .bound("rate", Binding::param("rate"))
                .bound("sync", Binding::param("sync"))
                .with("note", "1/8")
                .with("unipolar", true)
                .with("spread", 0.25)
                .bound("depth", Binding::mapped("depth", 0.0, 1.0)),
            NodeDef::new("chorus", "delay")
                .bound("time", Binding::signal("lfo", 12.0, 20.0))
                .with("feedback", 0.2)
                .bound("mix", Binding::mapped("mix", 0.0, 1.0))
                .with("max", 20.0),
            NodeDef::new("phase", "allpass")
                .with("stages", 4.0)
                .bound("freq", Binding::signal("lfo", 300.0, 6000.0).log())
                .with("feedback", 0.3)
                .with("mix", 0.5),
            NodeDef::new("wah", "biquad")
                .with("type", "lowpass")
                .bound("freq", Binding::signal("lfo", 200.0, 4000.0).log()),
            NodeDef::new("dry", "gain").bound("linear", Binding::mapped("mix", 1.0, 0.0)),
            NodeDef::new("sum", "mix"),
            NodeDef::new("out", "output"),
        ],
        edges: vec![
            ("in".into(), "tee".into()),
            ("tee".into(), "pre".into()),
            ("pre".into(), "gain".into()),
            ("gain".into(), "clip".into()),
            ("clip".into(), "tone".into()),
            ("tone".into(), "chorus".into()),
            ("chorus".into(), "phase".into()),
            ("phase".into(), "wah".into()),
            ("wah".into(), "sum".into()),
            ("tee".into(), "dry".into()),
            ("dry".into(), "sum".into()),
            ("sum".into(), "out".into()),
        ],
    }
}

#[test]
fn processing_allocates_nothing() {
    let mut pedal = compile(&everything()).unwrap();
    pedal.prepare(48_000.0, 128);
    let mut left: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
    let mut right = left.clone();
    let transport = Transport {
        bpm: 100.0,
        beats: 3.5,
        playing: true,
    };

    // A few blocks first, in case anything is lazy.
    for _ in 0..4 {
        let mut channels = [&mut left[..], &mut right[..]];
        pedal.set_transport(&transport);
        pedal.process(&mut Buffer::new(&mut channels));
    }

    let before = ALLOCATIONS.load(Ordering::SeqCst);
    for block in 0..400 {
        // Every kind of change the audio thread sees: knob moves,
        // including the ones that switch a curve or a factor, the
        // transport moving, and blocks of every size up to the maximum.
        let params = pedal.params().len();
        let id = block % params;
        let p = &pedal.params()[id];
        let value = p.min + (p.max - p.min) * ((block / params) % 5) as f32 / 4.0;
        pedal.set_param(id, value);
        pedal.set_transport(&Transport {
            beats: transport.beats + block as f64 * 0.01,
            ..transport
        });
        let frames = 1 + (block * 37) % 128;
        let mut channels = [&mut left[..frames], &mut right[..frames]];
        pedal.process(&mut Buffer::new(&mut channels));
        let _ = pedal.param(id);
    }
    let after = ALLOCATIONS.load(Ordering::SeqCst);
    assert_eq!(
        after - before,
        0,
        "process allocated {} times",
        after - before
    );
    assert!(left.iter().chain(&right).all(|s| s.is_finite()));
}
