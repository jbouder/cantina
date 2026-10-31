//! Guitar in, guitar out, through a chain of stock processors.
//!
//! ```text
//! cargo run -p cantina-engine --example passthrough -- --list
//! cargo run -p cantina-engine --example passthrough -- --in Scarlett --out Scarlett --buffer 64
//! cargo run -p cantina-engine --example passthrough -- --chain gain,overdrive,gain
//! cargo run -p cantina-engine --example passthrough -- --list-au
//! cargo run -p cantina-engine --example passthrough -- --chain gain,au:AUDelay
//! cargo run -p cantina-engine --example passthrough -- --list-clap
//! cargo run -p cantina-engine --example passthrough -- --chain clap:Surge\ XT
//! ```
//!
//! While running, type `list` to see the chain and its parameters,
//! `<processor>.<param> <value>` to change one (for example `2.drive 30`
//! or `2.1 30`), `s` for counters, `q` to quit. Processors and parameters
//! are numbered from 1 as `list` shows them. With `--click BPM` a
//! metronome is mixed in while the transport plays, and `--play FILE`
//! streams a WAV file from the start of the timeline: `play`, `stop`,
//! `loop BARS` and `pos` drive the transport.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_dsp::{Unit, STOCK_NAMES};
use cantina_engine::{
    device, processors, Click, Clip, Controller, Endpoint, GraphConfig, LoopRange, NodeId, Player,
    PlayerHandle, Runtime, Stream, StreamConfig, TempoMap, TimeSignature, MAX_BLOCK_FRAMES,
};
use cantina_hosting::{au, clap};

struct Options {
    list: bool,
    list_au: bool,
    list_clap: bool,
    input: Option<String>,
    output: Option<String>,
    sample_rate: f64,
    buffer_frames: u32,
    input_channel: usize,
    chain: Vec<String>,
    gain_db: f32,
    click_bpm: Option<f64>,
    play: Option<String>,
}

fn parse_args() -> Result<Options> {
    let mut opts = Options {
        list: false,
        list_au: false,
        list_clap: false,
        input: None,
        output: None,
        sample_rate: 48_000.0,
        buffer_frames: 128,
        input_channel: 0,
        chain: vec!["gain".into(), "overdrive".into()],
        gain_db: 0.0,
        click_bpm: None,
        play: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String> {
            args.next().with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--list" | "-l" => opts.list = true,
            "--list-au" => opts.list_au = true,
            "--list-clap" => opts.list_clap = true,
            "--in" | "-i" => opts.input = Some(value("--in")?),
            "--out" | "-o" => opts.output = Some(value("--out")?),
            "--rate" | "-r" => opts.sample_rate = value("--rate")?.parse()?,
            "--buffer" | "-b" => opts.buffer_frames = value("--buffer")?.parse()?,
            "--channel" | "-c" => {
                let n: usize = value("--channel")?.parse()?;
                if n == 0 {
                    bail!("--channel is 1-based");
                }
                opts.input_channel = n - 1;
            }
            "--chain" => {
                opts.chain = value("--chain")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "--gain" | "-g" => opts.gain_db = value("--gain")?.parse()?,
            "--click" => opts.click_bpm = Some(value("--click")?.parse()?),
            "--play" => opts.play = Some(value("--play")?),
            "--help" | "-h" => {
                println!(
                    "passthrough [--list] [--list-au] [--list-clap] [--in NAME] [--out NAME] \
                     [--rate HZ] [--buffer FRAMES] [--channel N] [--chain a,b,c] [--gain DB] \
                     [--click BPM] [--play FILE]\n\n\
                     Names match a prefix, case-insensitively. Channel is 1-based. --gain sets \
                     every gain processor's initial level. Chain items are stock processors ({}), \
                     au:<name or aufx:dely:appl>, or clap:<bundle name or path>[#plugin id].\n\
                     --click adds a metronome at BPM in 4/4 that plays while the transport \
                     runs. --play streams a mono WAV at the stream rate from the start of the \
                     timeline through the chain.\n\
                     Defaults: system devices, 48000 Hz, 128 frames, channel 1, chain \
                     gain,overdrive, 0 dB.",
                    STOCK_NAMES.join(", ")
                );
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}; try --help"),
        }
    }
    Ok(opts)
}

fn resolve(
    name: Option<&str>,
    default: Option<device::DeviceId>,
    what: &str,
) -> Result<device::DeviceId> {
    match name {
        Some(n) => {
            device::find_by_name(n)?.with_context(|| format!("no {what} device matching {n:?}"))
        }
        None => default.with_context(|| format!("no default {what} device")),
    }
}

fn print_devices() -> Result<()> {
    let default_in = device::default_input();
    let default_out = device::default_output();
    println!(
        "{:<40} {:>3} {:>3} {:>8} {:>6}",
        "device", "in", "out", "rate", "frames"
    );
    for d in device::list()? {
        let mark = match (Some(d.id) == default_in, Some(d.id) == default_out) {
            (true, true) => " (default in, out)",
            (true, false) => " (default in)",
            (false, true) => " (default out)",
            _ => "",
        };
        println!(
            "{:<40} {:>3} {:>3} {:>8} {:>6}{mark}",
            d.name, d.input_channels, d.output_channels, d.sample_rate, d.buffer_frames
        );
    }
    Ok(())
}

fn print_audio_units() {
    let effects = au::list_effects();
    println!("{:<16} name", "identifier");
    for c in &effects {
        println!("{:<16} {}", c.id.to_string(), c.name);
    }
    println!(
        "{} effect Audio Units. Use au:<name> or au:<identifier> in --chain.",
        effects.len()
    );
}

fn print_clap_plugins() {
    let bundles = clap::installed_bundles();
    for bundle in &bundles {
        println!("{}", bundle.display());
        match clap::list_plugins(bundle) {
            Ok(plugins) => {
                for p in plugins {
                    println!("    {:<40} {} ({})", p.id, p.name, p.vendor);
                }
            }
            Err(e) => println!("    could not load: {e:#}"),
        }
    }
    println!(
        "{} CLAP bundles in {}. Use clap:<bundle name>[#plugin id] in --chain.",
        bundles.len(),
        clap::default_search_paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" and ")
    );
}

/// Adds the processors to the controller's graph in series and returns
/// their node ids in chain order.
fn build_chain(controller: &mut Controller, names: &[String], gain_db: f32) -> Result<Vec<NodeId>> {
    let graph = controller.graph_mut();
    let mut nodes = Vec::with_capacity(names.len());
    for name in names {
        let mut processor = processors::create(name, 1)?;
        if name.eq_ignore_ascii_case("gain") {
            processor.set_param(0, gain_db);
        }
        nodes.push(graph.add_processor(processor));
    }
    graph.set_chain(&nodes)?;
    controller.commit()?;
    Ok(nodes)
}

/// Name, unit, minimum and maximum of one parameter.
type ParamRow = (String, Unit, f32, f32);

/// The chain's layout for the command loop, taken from the controller's
/// view of the graph.
struct Layout {
    processors: Vec<(NodeId, String, Vec<ParamRow>)>,
}

impl Layout {
    fn of(controller: &Controller, nodes: &[NodeId]) -> Self {
        Self {
            processors: nodes
                .iter()
                .map(|id| {
                    let info = controller.graph().node(*id).expect("node was just added");
                    (
                        *id,
                        info.name.clone(),
                        info.params
                            .iter()
                            .map(|i| (i.name.to_string(), i.unit, i.min, i.max))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    fn print(&self, controller: &Controller) {
        for (i, (id, name, params)) in self.processors.iter().enumerate() {
            println!("{}. {name}", i + 1);
            let values = controller.graph().node(*id).map(|n| n.values.as_slice());
            for (j, (pname, unit, min, max)) in params.iter().enumerate() {
                println!(
                    "   {}.{} {pname:<8} {:>8.2} {}  [{min} .. {max}]",
                    i + 1,
                    j + 1,
                    values.and_then(|v| v.get(j)).copied().unwrap_or(0.0),
                    unit_label(*unit)
                );
            }
        }
    }

    /// Parses `<processor>.<param>` where each part is a 1-based index or,
    /// for the parameter, a name.
    fn resolve(&self, target: &str) -> Result<(usize, usize)> {
        let (p, q) = target
            .split_once('.')
            .context("use <processor>.<param>, for example 2.drive")?;
        let processor = p.parse::<usize>().context("processor must be a number")?;
        if processor == 0 || processor > self.processors.len() {
            bail!("no processor {processor}; type list");
        }
        let params = &self.processors[processor - 1].2;
        let param = match q.parse::<usize>() {
            Ok(n) if n >= 1 && n <= params.len() => n - 1,
            Ok(n) => bail!("no parameter {n} on processor {processor}"),
            Err(_) => params
                .iter()
                .position(|(name, ..)| name.eq_ignore_ascii_case(q))
                .with_context(|| format!("no parameter {q:?} on processor {processor}"))?,
        };
        Ok((processor - 1, param))
    }
}

fn unit_label(unit: Unit) -> &'static str {
    match unit {
        Unit::Generic => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Seconds => "s",
        Unit::Percent => "%",
    }
}

fn main() -> Result<()> {
    let opts = parse_args()?;
    if opts.list {
        return print_devices();
    }
    if opts.list_au {
        print_audio_units();
        return Ok(());
    }
    if opts.list_clap {
        print_clap_plugins();
        return Ok(());
    }

    let input = resolve(opts.input.as_deref(), device::default_input(), "input")?;
    let output = resolve(opts.output.as_deref(), device::default_output(), "output")?;

    let (runtime, mut controller) = Runtime::new(GraphConfig {
        sample_rate: opts.sample_rate,
        max_block_frames: MAX_BLOCK_FRAMES,
        channels: 1,
    })?;
    let nodes = build_chain(&mut controller, &opts.chain, opts.gain_db)?;
    let mut player_handle = None;
    if let Some(file) = &opts.play {
        let (player, mut handle) = Player::new(opts.sample_rate, 1);
        let info = cantina_engine::wav::info(Path::new(file))?;
        handle.set_clips(vec![Clip {
            path: PathBuf::from(file),
            start: 0,
            offset: 0,
            length: info.frames,
        }])?;
        let player = controller.graph_mut().add(Box::new(player));
        // The file joins the input at the head of the chain.
        let first = nodes
            .first()
            .map_or(Endpoint::Output, |n| Endpoint::Node(*n));
        controller.graph_mut().connect(player, first)?;
        controller.commit()?;
        println!(
            "playing {file}: {} frames, {} channels, {} Hz",
            info.frames, info.channels, info.sample_rate
        );
        player_handle = Some(handle);
    }
    if let Some(bpm) = opts.click_bpm {
        let click = controller.graph_mut().add(Box::new(Click::default()));
        controller.graph_mut().connect(click, Endpoint::Output)?;
        controller.commit()?;
        controller
            .transport_mut()
            .set_tempo_map(TempoMap::new(bpm, TimeSignature::COMMON));
    }
    let layout = Layout::of(&controller, &nodes);

    let stream = Stream::open(
        StreamConfig {
            input,
            output,
            sample_rate: opts.sample_rate,
            buffer_frames: opts.buffer_frames,
            input_channel: opts.input_channel,
        },
        runtime,
    )?;
    let info = stream.info();

    println!(
        "in:  {} (channel {})",
        device::name(input)?,
        opts.input_channel + 1
    );
    println!("out: {}", device::name(output)?);
    println!(
        "{} Hz, {} frames, {}",
        info.sample_rate,
        info.buffer_frames,
        if info.duplex {
            "one duplex unit"
        } else {
            "two units, one block held back"
        }
    );

    let ms = |frames: u32| frames as f64 * 1000.0 / info.sample_rate;
    let (in_dev, in_safety) = device::reported_latency(input, true)?;
    let (out_dev, out_safety) = device::reported_latency(output, false)?;
    let total = in_dev
        + in_safety
        + out_dev
        + out_safety
        + 2 * info.buffer_frames
        + info.ring_prefill_frames;
    println!(
        "CoreAudio-reported round trip: {total} frames = {:.2} ms \
         (input {in_dev}+{in_safety}, output {out_dev}+{out_safety}, buffers {}, held back {})",
        ms(total),
        2 * info.buffer_frames,
        info.ring_prefill_frames
    );
    println!("Run measure_latency for the real number.");
    println!();
    layout.print(&controller);
    println!("Type <processor>.<param> <value>, list, s, play, stop, loop BARS, pos, or q.");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (None, _) => {}
            (Some("q") | Some("quit"), _) => break,
            (Some("s") | Some("stats"), _) => {
                print_stats(&stream, &controller, player_handle.as_ref())
            }
            (Some("play"), _) => {
                controller.transport_mut().play();
            }
            (Some("stop"), _) => {
                controller.transport_mut().stop();
            }
            (Some("pos"), _) => {
                let t = controller.transport();
                println!(
                    "{} at sample {} ({})",
                    if t.is_playing() { "playing" } else { "stopped" },
                    t.position(),
                    t.musical_position(opts.sample_rate)
                );
            }
            (Some("loop"), Some(bars)) => match bars.parse::<u32>() {
                Ok(0) => {
                    controller.transport_mut().set_loop(None);
                }
                Ok(bars) => {
                    let end = controller.transport().tempo_map().position_to_samples(
                        cantina_engine::MusicalPosition::new(bars, 0, 0),
                        opts.sample_rate,
                    );
                    controller.transport_mut().locate(0);
                    controller
                        .transport_mut()
                        .set_loop(Some(LoopRange { start: 0, end }));
                }
                Err(_) => println!("loop takes a number of bars, 0 to clear"),
            },
            (Some("list") | Some("l"), _) => layout.print(&controller),
            (Some(target), Some(value)) => match (layout.resolve(target), value.parse::<f32>()) {
                (Ok((p, q)), Ok(v)) => {
                    let id = layout.processors[p].0;
                    if controller.set_param(id, q, v) {
                        let v = controller.graph().node(id).map_or(v, |n| n.values[q]);
                        println!("{}.{} = {v}", p + 1, q + 1);
                    } else {
                        println!("the audio thread is not draining changes");
                    }
                }
                (Err(e), _) => println!("{e}"),
                (_, Err(_)) => println!("value must be a number"),
            },
            (Some(_), None) => println!("type <processor>.<param> <value>, list, s, or q"),
        }
        io::stdout().flush()?;
    }
    print_stats(&stream, &controller, player_handle.as_ref());
    stream.close();
    Ok(())
}

fn print_stats(stream: &Stream, controller: &Controller, player: Option<&PlayerHandle>) {
    use std::sync::atomic::Ordering::Relaxed;
    let s = stream.stats();
    println!(
        "callbacks in {} out {}, overrun frames {}, underrun frames {}, largest block {}, oversized {}",
        s.input_callbacks.load(Relaxed),
        s.output_callbacks.load(Relaxed),
        s.input_overruns.load(Relaxed),
        s.output_underruns.load(Relaxed),
        s.max_block_frames.load(Relaxed),
        s.oversized_blocks.load(Relaxed),
    );
    let r = controller.stats();
    println!(
        "graph blocks {}, swaps {}, busy nodes {}, dropped params {}",
        r.blocks.load(Relaxed),
        r.swaps.load(Relaxed),
        r.busy_nodes.load(Relaxed),
        r.params_dropped.load(Relaxed),
    );
    if let Some(p) = player {
        let p = p.stats();
        println!(
            "player chunks {}, ready frames {}, underrun frames {}, seeks {}, stale chunks {}, read errors {}",
            p.chunks.load(Relaxed),
            p.ready_frames.load(Relaxed),
            p.underrun_frames.load(Relaxed),
            p.seeks.load(Relaxed),
            p.stale_chunks.load(Relaxed),
            p.read_errors.load(Relaxed),
        );
    }
}
