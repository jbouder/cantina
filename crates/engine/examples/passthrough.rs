//! Guitar in, guitar out, with a gain in between.
//!
//! ```text
//! cargo run -p cantina-engine --example passthrough -- --list
//! cargo run -p cantina-engine --example passthrough -- --in Scarlett --out Scarlett --buffer 64
//! ```
//!
//! While running, type a number and press return to set the gain in dB,
//! `s` to print counters, `q` to quit.

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use cantina_dsp::Gain;
use cantina_engine::{device, BlockProcessor, Stream, StreamConfig};

/// Copies input to output through a smoothed gain. The gain in dB arrives
/// from the main thread as the bits of an `f32` in an atomic.
struct GainProcessor {
    gain: Gain,
    shared_db_bits: Arc<AtomicU32>,
    last_bits: u32,
}

impl GainProcessor {
    fn new(sample_rate: f32, shared_db_bits: Arc<AtomicU32>) -> Self {
        let bits = shared_db_bits.load(Ordering::Relaxed);
        let mut gain = Gain::new(sample_rate);
        gain.set_db(f32::from_bits(bits));
        Self {
            gain,
            shared_db_bits,
            last_bits: bits,
        }
    }
}

impl BlockProcessor for GainProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let bits = self.shared_db_bits.load(Ordering::Relaxed);
        if bits != self.last_bits {
            self.last_bits = bits;
            self.gain.set_db(f32::from_bits(bits));
        }
        output.copy_from_slice(input);
        self.gain.process(output);
    }
}

struct Options {
    list: bool,
    input: Option<String>,
    output: Option<String>,
    sample_rate: f64,
    buffer_frames: u32,
    input_channel: usize,
    gain_db: f32,
}

fn parse_args() -> Result<Options> {
    let mut opts = Options {
        list: false,
        input: None,
        output: None,
        sample_rate: 48_000.0,
        buffer_frames: 128,
        input_channel: 0,
        gain_db: 0.0,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String> {
            args.next().with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--list" | "-l" => opts.list = true,
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
            "--gain" | "-g" => opts.gain_db = value("--gain")?.parse()?,
            "--help" | "-h" => {
                println!(
                    "passthrough [--list] [--in NAME] [--out NAME] [--rate HZ] [--buffer FRAMES] \
                     [--channel N] [--gain DB]\n\nNames match a prefix, case-insensitively. \
                     Channel is 1-based. Defaults: system devices, 48000 Hz, 128 frames, channel 1, 0 dB."
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

fn main() -> Result<()> {
    let opts = parse_args()?;
    if opts.list {
        return print_devices();
    }

    let input = resolve(opts.input.as_deref(), device::default_input(), "input")?;
    let output = resolve(opts.output.as_deref(), device::default_output(), "output")?;

    let gain_db_bits = Arc::new(AtomicU32::new(opts.gain_db.to_bits()));
    let stream = Stream::open(
        StreamConfig {
            input,
            output,
            sample_rate: opts.sample_rate,
            buffer_frames: opts.buffer_frames,
            input_channel: opts.input_channel,
        },
        GainProcessor::new(opts.sample_rate as f32, Arc::clone(&gain_db_bits)),
    )?;
    let info = stream.info();
    let gain_db = |bits: &AtomicU32| f32::from_bits(bits.load(Ordering::Relaxed));

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
    println!("Measured round trip is M0 issue 3; this number is what the driver claims.");
    println!(
        "gain {:.1} dB. Type a number for a new gain, s for stats, q to quit.",
        gain_db(&gain_db_bits)
    );

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        match line.trim() {
            "q" | "quit" => break,
            "s" | "stats" => print_stats(&stream),
            "" => {}
            text => match text.parse::<f32>() {
                Ok(db) => {
                    gain_db_bits.store(db.to_bits(), Ordering::Relaxed);
                    println!("gain {db:.1} dB");
                }
                Err(_) => println!("type a gain in dB, s, or q"),
            },
        }
        io::stdout().flush()?;
    }
    print_stats(&stream);
    stream.close();
    Ok(())
}

fn print_stats(stream: &Stream) {
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
}
