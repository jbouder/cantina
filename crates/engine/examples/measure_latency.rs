//! Measures real round-trip latency through a loopback cable.
//!
//! ```text
//! cargo run -p cantina-engine --example measure_latency -- --in Scarlett --out Scarlett --buffer 64
//! ```
//!
//! Connect output 1 of the interface to input 1 with a cable, turn the
//! input gain to about half, and run. Prints each click's delay, the
//! median, and a row for docs/latency.md.

use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use cantina_engine::latency::ClickTrain;
use cantina_engine::{device, Stream, StreamConfig};

struct Options {
    input: Option<String>,
    output: Option<String>,
    sample_rate: f64,
    buffer_frames: u32,
    input_channel: usize,
    clicks: usize,
    period_ms: f64,
    amplitude: f32,
}

fn parse_args() -> Result<Options> {
    let mut opts = Options {
        input: None,
        output: None,
        sample_rate: 48_000.0,
        buffer_frames: 128,
        input_channel: 0,
        clicks: 10,
        period_ms: 500.0,
        amplitude: 0.5,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String> {
            args.next().with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
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
            "--clicks" | "-n" => opts.clicks = value("--clicks")?.parse()?,
            "--period" | "-p" => opts.period_ms = value("--period")?.parse()?,
            "--amplitude" | "-a" => opts.amplitude = value("--amplitude")?.parse()?,
            "--help" | "-h" => {
                println!(
                    "measure_latency [--in NAME] [--out NAME] [--rate HZ] [--buffer FRAMES] \
                     [--channel N] [--clicks N] [--period MS] [--amplitude 0..1]\n\n\
                     Connect an output to an input with a cable first. Defaults: system devices, \
                     48000 Hz, 128 frames, channel 1, 10 clicks, 500 ms apart, amplitude 0.5."
                );
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}; try --help"),
        }
    }
    if opts.clicks == 0 {
        bail!("--clicks must be at least 1");
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

fn main() -> Result<()> {
    let opts = parse_args()?;
    let input = resolve(opts.input.as_deref(), device::default_input(), "input")?;
    let output = resolve(opts.output.as_deref(), device::default_output(), "output")?;

    let period = (opts.period_ms / 1000.0 * opts.sample_rate).round() as usize;
    let lead = period;
    let total = lead + (opts.clicks + 1) * period;
    let (train, mut capture) = ClickTrain::new(lead, period, opts.amplitude, total * 2);

    let stream = Stream::open(
        StreamConfig {
            input,
            output,
            sample_rate: opts.sample_rate,
            buffer_frames: opts.buffer_frames,
            input_channel: opts.input_channel,
        },
        train,
    )?;
    let info = stream.info().clone();
    let ms = |frames: usize| frames as f64 * 1000.0 / info.sample_rate;

    let in_name = device::name(input)?;
    let out_name = device::name(output)?;
    println!("in:  {in_name} (channel {})", opts.input_channel + 1);
    println!("out: {out_name}");
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
    let (in_dev, in_safety) = device::reported_latency(input, true)?;
    let (out_dev, out_safety) = device::reported_latency(output, false)?;
    let reported = (in_dev + in_safety + out_dev + out_safety + 2 * info.buffer_frames) as usize
        + info.ring_prefill_frames as usize;
    println!(
        "CoreAudio reports {reported} frames = {:.2} ms. Playing {} clicks...",
        ms(reported),
        opts.clicks
    );

    while capture.len() < total {
        thread::sleep(Duration::from_millis(20));
        capture.drain();
        if capture.dropped() > 0 {
            break;
        }
    }
    let underruns = stream
        .stats()
        .output_underruns
        .load(std::sync::atomic::Ordering::Relaxed);
    drop(stream);

    let m = capture.analyze(opts.clicks)?;
    println!();
    println!(
        "noise floor {:.5} rms, threshold {:.4}",
        m.noise_rms, m.threshold
    );
    println!("delays (frames): {:?}", m.delays);
    println!(
        "round trip: median {} frames = {:.2} ms, min {} = {:.2} ms, max {} = {:.2} ms",
        m.median(),
        ms(m.median()),
        m.min(),
        ms(m.min()),
        m.max(),
        ms(m.max())
    );
    let delta = m.median() as i64 - reported as i64;
    println!(
        "measured minus reported: {delta:+} frames = {:+.2} ms",
        delta as f64 * 1000.0 / info.sample_rate
    );
    if underruns > 0 {
        println!("warning: {underruns} underrun frames during the run; try a larger buffer");
    }
    println!();
    println!("Row for docs/latency.md:");
    println!(
        "| {in_name} | {} | {} | {} | {:.2} | {} | {:.2} | {:+} |",
        info.sample_rate as u32,
        info.buffer_frames,
        m.median(),
        ms(m.median()),
        reported,
        ms(reported),
        delta
    );
    Ok(())
}
