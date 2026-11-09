//! Rigs built into processors: what `check` and `render rig` do with a
//! `*.rig.json`, and what the app does when it applies one.
//!
//! A rig's pedals are built the way the app builds a chain's: a stock
//! name with [`cantina_dsp::stock`], `cab:<id>` as a cabinet with the
//! impulse loaded from the impulse folders, `pedal:<id>` compiled from a
//! designed pedal in the pedal folders, a plugin through the engine's
//! hosting. Each pedal's parameter names are resolved against what it
//! turns out to have. A pedal that cannot be built because something is
//! not installed (a plugin, a designed pedal, an impulse) is left out and
//! named, so the rest of the rig still plays; a name or value that is
//! wrong is a problem with the file.

use std::path::{Path, PathBuf};

use cantina_dsp::{
    compile, db_to_linear, stock, stock_definition, Buffer, Cabinet, ParamInfo, PedalDef,
    Processor, Transport, Unit, MAX_CHANNELS,
};
use cantina_project::{resolve_values, ParamSlot, Rig, RigPedal};

use crate::render::BLOCK;

/// Where a rig's pedals are looked for beyond the stock ones.
#[derive(Debug, Clone, Default)]
pub struct RigRoots {
    /// Folders of impulse responses, for `cab:<id>`.
    pub irs: Vec<PathBuf>,
    /// Folders of designed pedals, for `pedal:<id>`.
    pub pedals: Vec<PathBuf>,
}

impl RigRoots {
    /// The folders given, then the user's library, then this repository's
    /// stock impulses when the binary was built from it.
    pub fn with_defaults(irs: &[PathBuf], pedals: &[PathBuf]) -> Self {
        let mut out = Self {
            irs: irs.to_vec(),
            pedals: pedals.to_vec(),
        };
        if let Some(home) = std::env::var_os("HOME") {
            let library = PathBuf::from(home).join("Music/Cantina");
            out.irs.push(library.join("IRs"));
            out.pedals.push(library.join("Pedals"));
        }
        out.irs
            .push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../irs"));
        out
    }
}

/// A rig's pedal, built and set.
pub struct BuiltPedal {
    /// Its index in the rig's chain.
    pub index: usize,
    /// The processor, with the rig's values set.
    pub processor: Box<dyn Processor>,
    /// The values set, one per parameter.
    pub values: Vec<f32>,
}

/// A rig, built.
pub struct BuiltRig {
    /// The pedals that could be built, in order.
    pub pedals: Vec<BuiltPedal>,
    /// The pedals left out because something is not installed, one line
    /// each saying what.
    pub missing: Vec<String>,
}

/// The parameters a processor has, as rig names resolve against: the
/// pedal format's ids from `def` when it is defined in it.
pub fn slots(processor: &dyn Processor, def: Option<&PedalDef>) -> Vec<ParamSlot> {
    slots_for(processor.params(), def)
}

/// [`slots`] from a parameter list.
pub fn slots_for(params: &[ParamInfo], def: Option<&PedalDef>) -> Vec<ParamSlot> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| ParamSlot {
            id: def.and_then(|d| d.params.get(i)).map(|d| d.id.clone()),
            name: p.name.to_string(),
            min: p.min,
            max: p.max,
            default: p.default,
            options: match p.unit {
                Unit::Choice(names) => names.iter().map(|n| n.to_string()).collect(),
                _ => Vec::new(),
            },
            switch: matches!(p.unit, Unit::Toggle),
        })
        .collect()
}

/// Why a pedal was not built.
pub enum Unbuilt {
    /// Something it needs is not installed; the rest of the rig plays.
    Missing(String),
    /// The rig names it wrongly.
    Wrong(String),
}

/// Builds one pedal of a rig, without its values: the processor and the
/// definition its parameter ids come from.
pub fn build_pedal(
    pedal: &RigPedal,
    roots: &RigRoots,
    channels: usize,
) -> Result<(Box<dyn Processor>, Option<PedalDef>), Unbuilt> {
    let spec = pedal.pedal.as_str();
    if let Some(id) = spec.strip_prefix("cab:") {
        let entry = cantina_engine::ir::discover(&roots.irs)
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| {
                Unbuilt::Missing(format!("the impulse response {id:?} is not installed"))
            })?;
        let impulse = cantina_engine::ir::load(&entry.path)
            .map_err(|e| Unbuilt::Missing(format!("{e:#}")))?;
        let mut cab = Cabinet::default();
        cab.set_impulse(&impulse.samples, impulse.sample_rate);
        return Ok((Box::new(cab), None));
    }
    if let Some(id) = spec.strip_prefix("pedal:") {
        let path = roots
            .pedals
            .iter()
            .map(|r| r.join(format!("{id}.pedal.json")))
            .find(|p| p.is_file())
            .ok_or_else(|| {
                Unbuilt::Missing(format!("the designed pedal {id:?} is not installed"))
            })?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Unbuilt::Missing(format!("{}: {e}", path.display())))?;
        let def = PedalDef::from_json(&text)
            .map_err(|e| Unbuilt::Wrong(format!("{}: {e}", path.display())))?;
        let pedal =
            compile(&def).map_err(|e| Unbuilt::Wrong(format!("{}: {e}", path.display())))?;
        return Ok((Box::new(pedal), Some(def)));
    }
    if pedal.is_plugin() {
        let mut p = cantina_engine::processors::create(spec, channels).map_err(|e| {
            Unbuilt::Missing(format!(
                "the plugin {spec} is not installed or would not load: {e:#}"
            ))
        })?;
        if let Some(state) = pedal.state_bytes() {
            p.set_state(&state).map_err(|e| {
                Unbuilt::Missing(format!("the plugin {spec} refused its state: {e}"))
            })?;
        }
        return Ok((p, None));
    }
    let processor = stock(spec).ok_or_else(|| {
        Unbuilt::Wrong(format!(
            "{spec:?} is not a stock pedal ({}), cab:<impulse>, pedal:<id>, au: or clap:",
            cantina_dsp::STOCK_NAMES.join(", ")
        ))
    })?;
    Ok((processor, stock_definition(spec).cloned()))
}

/// Builds every pedal of `rig` and sets its values. Fails with every
/// problem in the file; what is not installed is in `missing`.
pub fn build(rig: &Rig, roots: &RigRoots, channels: usize) -> Result<BuiltRig, Vec<String>> {
    let mut pedals = Vec::new();
    let mut missing = Vec::new();
    let mut problems = Vec::new();
    for (index, pedal) in rig.chain.iter().enumerate() {
        let at = format!("chain {index} ({})", pedal.pedal);
        match build_pedal(pedal, roots, channels) {
            Ok((mut processor, def)) => {
                let slots = slots(processor.as_ref(), def.as_ref());
                // A plugin with a state and no params keeps what the state set.
                if pedal.params.is_empty() && pedal.state.is_some() {
                    let values = (0..slots.len()).map(|i| processor.param(i)).collect();
                    pedals.push(BuiltPedal {
                        index,
                        processor,
                        values,
                    });
                    continue;
                }
                match resolve_values(pedal, &slots) {
                    Ok(values) => {
                        for (i, v) in values.iter().enumerate() {
                            processor.set_param(i, *v);
                        }
                        pedals.push(BuiltPedal {
                            index,
                            processor,
                            values,
                        });
                    }
                    Err(p) => problems.extend(p.into_iter().map(|p| format!("{at}: {p}"))),
                }
            }
            Err(Unbuilt::Missing(why)) => missing.push(format!("{at}: {why}; it is left out")),
            Err(Unbuilt::Wrong(why)) => problems.push(format!("{at}: {why}")),
        }
    }
    if problems.is_empty() {
        Ok(BuiltRig { pedals, missing })
    } else {
        Err(problems)
    }
}

/// Runs `input` through the rig's input gain and its pedals in order,
/// bypassed ones skipped, plus `tail` frames of silence, with the
/// transport rolling at `bpm`. Returns the output and the chain's latency.
/// The instrument profile is the app's, not the file's, so it plays no
/// part here.
pub fn render(
    rig: &Rig,
    built: &mut BuiltRig,
    sample_rate: f64,
    input: &[Vec<f32>],
    tail: usize,
    bpm: f64,
) -> (Vec<Vec<f32>>, usize) {
    let gain = db_to_linear(rig.input_gain_db);
    let mut channels: Vec<Vec<f32>> = input
        .iter()
        .take(MAX_CHANNELS)
        .map(|c| {
            let mut c: Vec<f32> = c.iter().map(|s| s * gain).collect();
            c.resize(c.len() + tail, 0.0);
            c
        })
        .collect();
    let active: Vec<&mut BuiltPedal> = built
        .pedals
        .iter_mut()
        .filter(|p| !rig.chain[p.index].bypassed)
        .collect();
    let mut latency = 0;
    let mut active = active;
    for p in active.iter_mut() {
        p.processor.prepare(sample_rate, BLOCK);
        latency += p.processor.latency();
    }
    let frames = channels.first().map_or(0, Vec::len);
    let mut start = 0;
    while start < frames {
        let end = (start + BLOCK).min(frames);
        let transport = Transport {
            bpm,
            beats: start as f64 / sample_rate * bpm / 60.0,
            playing: true,
        };
        for p in active.iter_mut() {
            p.processor.set_transport(&transport);
            let mut lanes: Vec<&mut [f32]> =
                channels.iter_mut().map(|c| &mut c[start..end]).collect();
            p.processor.process(&mut Buffer::new(&mut lanes));
        }
        start = end;
    }
    (channels, latency)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Source, Stats};
    use cantina_project::{rig_from_json, Instrument, RIG_SUFFIX};

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn roots() -> RigRoots {
        RigRoots {
            irs: vec![repo().join("irs")],
            pedals: vec![repo().join("pedals")],
        }
    }

    fn rig(json: &str) -> Rig {
        rig_from_json(json).unwrap()
    }

    #[test]
    fn a_rig_builds_with_its_values_set() {
        let r = rig(
            r#"{ "version": 1, "name": "T", "instrument": "guitar", "chain": [
            { "pedal": "overdrive", "params": { "drive": 30, "curve": "hard" } },
            { "pedal": "reverb", "params": { "pre-delay": 20 } },
            { "pedal": "cab:4x12-modern", "params": { "level": -3 } } ] }"#,
        );
        let built = build(&r, &roots(), 1).unwrap();
        assert!(built.missing.is_empty());
        assert_eq!(built.pedals.len(), 3);
        assert_eq!(built.pedals[0].values[0], 30.0);
        assert_eq!(built.pedals[0].processor.param(0), 30.0);
        assert_eq!(built.pedals[1].values[3], 20.0);
        assert_eq!(built.pedals[2].values[0], -3.0);
    }

    #[test]
    fn what_is_not_installed_is_left_out_and_named() {
        let r = rig(
            r#"{ "version": 1, "name": "T", "instrument": "guitar", "chain": [
            { "pedal": "clap:no-such-plugin", "state": "AAAA" },
            { "pedal": "pedal:no-such-design" },
            { "pedal": "cab:no-such-impulse" },
            { "pedal": "gain", "params": { "gain": -6 } } ] }"#,
        );
        let built = build(&r, &roots(), 1).unwrap();
        assert_eq!(built.pedals.len(), 1);
        assert_eq!(built.pedals[0].index, 3);
        assert_eq!(built.missing.len(), 3, "{:#?}", built.missing);
        assert!(built.missing[0].contains("no-such-plugin"));
    }

    #[test]
    fn wrong_names_are_problems() {
        let r = rig(
            r#"{ "version": 1, "name": "T", "instrument": "guitar", "chain": [
            { "pedal": "flux capacitor" },
            { "pedal": "amp", "params": { "volume": 3 } } ] }"#,
        );
        let problems = build(&r, &roots(), 1).err().unwrap();
        assert_eq!(problems.len(), 2, "{problems:#?}");
        assert!(problems[1].starts_with("chain 1 (amp): amp has no parameter \"volume\""));
    }

    #[test]
    fn input_gain_and_bypass_shape_the_render() {
        let r = rig(
            r#"{ "version": 1, "name": "T", "instrument": "guitar", "inputGainDb": -6,
            "chain": [ { "pedal": "gain", "params": { "gain": 20 }, "bypassed": true } ] }"#,
        );
        let mut built = build(&r, &roots(), 1).unwrap();
        let (rate, input) = Source::Di.signal(1.0);
        let (out, latency) = render(&r, &mut built, rate, &input, 0, 120.0);
        assert_eq!(latency, 0);
        let a = Stats::of(&input, rate, 0).peak_db;
        let b = Stats::of(&out, rate, 0).peak_db;
        assert!((a - 6.0 - b).abs() < 0.01, "{a} {b}");
    }

    /// The stock rigs, rendered through the DI of their instrument: none
    /// clips, none is silent, and their levels sit within a few dB of
    /// each other so switching rigs does not jump.
    #[test]
    fn stock_rigs_render_level_and_clean_with_golden_numbers() {
        let mut rigs: Vec<PathBuf> = std::fs::read_dir(repo().join("rigs"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(RIG_SUFFIX))
            .collect();
        rigs.sort();
        let mut levels = Vec::new();
        for path in &rigs {
            let r = rig_from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
            let mut built =
                build(&r, &roots(), 1).unwrap_or_else(|p| panic!("{}: {p:#?}", path.display()));
            assert!(
                built.missing.is_empty(),
                "{}: {:?}",
                path.display(),
                built.missing
            );
            let source = match r.instrument {
                Instrument::Guitar => Source::Di,
                Instrument::Bass => Source::BassDi,
            };
            let (rate, input) = source.signal(4.0);
            let (out, latency) = render(&r, &mut built, rate, &input, 24_000, 120.0);
            let stats = Stats::of(&out, rate, latency);
            assert!(!stats.failed(), "{}: {}", path.display(), stats.text());
            assert_eq!(stats.clipped, 0, "{}: {}", path.display(), stats.text());
            assert!(
                stats.peak_db <= -0.5,
                "{}: {}",
                path.display(),
                stats.text()
            );
            assert!(
                stats.dc_offset.abs() < 1e-3,
                "{}: {}",
                path.display(),
                stats.text()
            );
            levels.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                stats,
            ));
        }
        let rms: Vec<f32> = levels.iter().map(|(_, s)| s.rms_db).collect();
        let lo = rms.iter().cloned().fold(f32::MAX, f32::min);
        let hi = rms.iter().cloned().fold(f32::MIN, f32::max);
        let table: Vec<String> = levels
            .iter()
            .map(|(n, s)| format!("{n:<28} peak {:>6.1} rms {:>6.1}", s.peak_db, s.rms_db))
            .collect();
        assert!(
            hi - lo <= 6.0,
            "levels spread {:.1} dB:\n{}",
            hi - lo,
            table.join("\n")
        );

        // The golden numbers: a change to a stock pedal or a rig that moves
        // a rig's level shows here. Rewrite with
        // UPDATE_GOLDEN=1 cargo test -p cantina-cli rigs.
        let golden: serde_json::Map<String, serde_json::Value> = levels
            .iter()
            .map(|(n, s)| {
                let round = |x: f32| (f64::from(x) * 10.0).round() / 10.0;
                (
                    n.trim_end_matches(RIG_SUFFIX).to_string(),
                    serde_json::json!({
                        "peakDb": round(s.peak_db),
                        "rmsDb": round(s.rms_db),
                        "latency": s.latency,
                    }),
                )
            })
            .collect();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rig-levels.json");
        let text = serde_json::to_string_pretty(&golden).unwrap() + "\n";
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
                .unwrap_or_default();
        for (name, now) in &golden {
            let was = &on_disk[name];
            for k in ["peakDb", "rmsDb"] {
                let (a, b) = (
                    was[k].as_f64().unwrap_or(f64::NAN),
                    now[k].as_f64().unwrap(),
                );
                assert!(
                    (a - b).abs() <= 0.15,
                    "{name} {k} was {a}, now {b}; if on purpose, UPDATE_GOLDEN=1 cargo test -p cantina-cli rigs"
                );
            }
            assert_eq!(was["latency"], now["latency"], "{name} latency");
        }
        assert_eq!(
            on_disk.as_object().map_or(0, |o| o.len()),
            golden.len(),
            "fixtures/rig-levels.json lists other rigs; UPDATE_GOLDEN=1 cargo test -p cantina-cli rigs"
        );
    }
}
