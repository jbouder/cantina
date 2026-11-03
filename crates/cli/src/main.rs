//! The `cantina-cli` command. See the crate docs and `--help`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cantina_cli::render::{self, Source, Stats};
use cantina_cli::{check, synth, CheckOptions, Format, EXIT_INVALID, EXIT_OK, EXIT_USAGE};
use clap::{Args, Parser, Subcommand};

/// Check, render and list what Cantina reads, without the app.
///
/// Exit status: 0 when the file is valid or the render worked, 1 when it
/// is not (the problems go to stderr, the first first), 2 for a mistake
/// on the command line.
#[derive(Parser)]
#[command(name = "cantina-cli", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a pedal, kit, kit recipe, pattern or project against its
    /// schema and the rules the app enforces beyond it.
    Check {
        /// A file, or a kit or project folder.
        path: PathBuf,
        /// Read it as this format instead of working it out from the name.
        #[arg(long = "as", value_enum)]
        format: Option<Format>,
        /// For a pattern: a kit folder whose pads every row must name.
        #[arg(long)]
        kit: Option<PathBuf>,
        /// For a project: more folders of kits to find its drum tracks' kits in.
        #[arg(long = "kits")]
        kit_roots: Vec<PathBuf>,
        /// Print the result as JSON on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Render a pedal or a pattern offline to a WAV and print its levels.
    #[command(subcommand)]
    Render(RenderCommand),
    /// Print the pedal node catalog: every kind, its fields, ranges and rules.
    Kinds {
        /// As JSON, the catalog the designer reads.
        #[arg(long)]
        json: bool,
    },
    /// Print a format's JSON Schema.
    Schema {
        /// The format.
        #[arg(value_enum)]
        format: Format,
    },
    /// Synthesise a kit folder (samples and kit.json) from a kit recipe.
    SynthKit {
        /// The recipe, a `*.recipe.json`.
        recipe: PathBuf,
        /// The kit folder to write; its name is the kit's id.
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum RenderCommand {
    /// Run a pedal over a guitar DI, a tone or a WAV.
    Pedal {
        /// The `*.pedal.json`.
        file: PathBuf,
        #[command(flatten)]
        source: SourceArgs,
        /// Set a parameter by id, label or index, as name=value. Repeatable.
        #[arg(long = "set", value_parser = parse_setting)]
        settings: Vec<(String, f32)>,
        /// Seconds of silence after the input, for tails to ring out.
        #[arg(long, default_value_t = 0.5)]
        tail: f32,
        /// Seconds of tone, with --tone.
        #[arg(long, default_value_t = 2.0)]
        seconds: f32,
        /// Tempo the transport rolls at, for synced modulation and delay.
        #[arg(long, default_value_t = 120.0)]
        bpm: f64,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Play a pattern through a kit, on the app's sequencer and sampler.
    Pattern {
        /// The `*.pattern.json`.
        file: PathBuf,
        /// The kit folder to play it on.
        #[arg(long)]
        kit: PathBuf,
        /// Tempo, in 4/4.
        #[arg(long, default_value_t = 120.0)]
        bpm: f64,
        /// Times through the pattern.
        #[arg(long, default_value_t = 2)]
        passes: u32,
        /// Seconds after the last pass for hits to ring out.
        #[arg(long, default_value_t = 1.0)]
        tail: f64,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Args)]
#[group(multiple = false)]
struct SourceArgs {
    /// Feed it this WAV.
    #[arg(long)]
    input: Option<PathBuf>,
    /// Feed it a sine at this frequency, at -12 dBFS.
    #[arg(long)]
    tone: Option<f32>,
    /// Feed it the guitar DI fixture (the default).
    #[arg(long)]
    di: bool,
}

#[derive(Args)]
struct OutputArgs {
    /// The WAV to write; `<file stem>.wav` in the current folder when missing.
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Print the levels as JSON on stdout.
    #[arg(long)]
    json: bool,
}

fn parse_setting(s: &str) -> Result<(String, f32), String> {
    let (name, value) = s
        .split_once('=')
        .ok_or_else(|| format!("{s:?} is not name=value"))?;
    let value: f32 = value
        .trim()
        .parse()
        .map_err(|_| format!("{value:?} is not a number"))?;
    Ok((name.trim().to_string(), value))
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let usage = e.use_stderr();
            let _ = e.print();
            return ExitCode::from(if usage { EXIT_USAGE } else { EXIT_OK } as u8);
        }
    };
    let code = match run(cli.command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            EXIT_INVALID
        }
    };
    ExitCode::from(code as u8)
}

fn run(command: Command) -> anyhow::Result<i32> {
    match command {
        Command::Check {
            path,
            format,
            kit,
            kit_roots,
            json,
        } => {
            if !path.exists() {
                eprintln!("error: {} does not exist", path.display());
                return Ok(EXIT_USAGE);
            }
            let options = CheckOptions {
                format,
                kit,
                kit_roots,
            };
            let result = check(&path, &options);
            if json {
                let value = match &result {
                    Ok(r) => serde_json::json!({
                        "valid": true,
                        "format": r.format.name(),
                        "file": r.file,
                        "summary": r.summary,
                        "warnings": r.warnings,
                    }),
                    Err(f) => serde_json::json!({
                        "valid": false,
                        "format": f.format.map(Format::name),
                        "problems": f.problems,
                    }),
                };
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            match result {
                Ok(r) => {
                    if !json {
                        println!("ok: {}", r.summary);
                        for w in &r.warnings {
                            println!("warning: {w}");
                        }
                    }
                    Ok(EXIT_OK)
                }
                Err(f) => {
                    let what = f.format.map_or("file", Format::name);
                    eprintln!("invalid {what}: {}", path.display());
                    for p in &f.problems {
                        eprintln!("  {p}");
                    }
                    Ok(EXIT_INVALID)
                }
            }
        }
        Command::Render(RenderCommand::Pedal {
            file,
            source,
            settings,
            tail,
            seconds,
            bpm,
            output,
        }) => {
            let text = std::fs::read_to_string(&file)?;
            let def = match cantina_dsp::PedalDef::from_json(&text) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("invalid pedal: {}", file.display());
                    for p in e.problems() {
                        eprintln!("  {p}");
                    }
                    return Ok(EXIT_INVALID);
                }
            };
            let source = if let Some(path) = source.input {
                let (rate, channels) = render::read_wav(&path)?;
                Source::Audio(rate, channels)
            } else if let Some(hz) = source.tone {
                Source::Tone(hz)
            } else {
                Source::Di
            };
            let (rate, input) = source.signal(seconds);
            let tail = (tail.max(0.0) as f64 * rate) as usize;
            let (out, latency) = render::render_pedal(&def, &settings, rate, &input, tail, bpm)?;
            finish(&file, &output, rate, &out, latency)
        }
        Command::Render(RenderCommand::Pattern {
            file,
            kit,
            bpm,
            passes,
            tail,
            output,
        }) => {
            let text = std::fs::read_to_string(&file)?;
            let pattern = match cantina_project::pattern_from_json(&text) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("invalid pattern: {}\n  {e}", file.display());
                    return Ok(EXIT_INVALID);
                }
            };
            if !(bpm.is_finite() && bpm > 0.0) {
                eprintln!("error: --bpm must be above 0");
                return Ok(EXIT_USAGE);
            }
            let (out, missing) = render::render_pattern(&pattern, &kit, bpm, passes, tail)?;
            for pad in &missing {
                eprintln!("warning: the kit has no pad {pad:?}; its row is silent");
            }
            finish(&file, &output, render::RATE, &out, 0)
        }
        Command::Kinds { json } => {
            if json {
                println!("{}", cantina_cli::kinds_json());
            } else {
                print!("{}", cantina_cli::kinds_table());
            }
            Ok(EXIT_OK)
        }
        Command::Schema { format } => {
            println!("{}", serde_json::to_string_pretty(&format.schema())?);
            Ok(EXIT_OK)
        }
        Command::SynthKit { recipe, out } => {
            let recipe = match synth::read_recipe(&recipe) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("invalid kit recipe: {e:#}");
                    return Ok(EXIT_INVALID);
                }
            };
            let kit = synth::synth_kit(&recipe, &out)?;
            let samples: usize = kit.pads.iter().map(|p| p.layers.len()).sum();
            match check(&out, &CheckOptions::default()) {
                Ok(_) => {
                    println!(
                        "wrote {}: kit {:?}, {} pads, {samples} samples",
                        out.display(),
                        kit.name,
                        kit.pads.len()
                    );
                    Ok(EXIT_OK)
                }
                Err(f) => {
                    eprintln!("the kit written to {} does not check:", out.display());
                    for p in f.problems {
                        eprintln!("  {p}");
                    }
                    Ok(EXIT_INVALID)
                }
            }
        }
    }
}

/// Writes the render and prints its numbers; fails on silence or NaN.
fn finish(
    file: &Path,
    output: &OutputArgs,
    rate: f64,
    channels: &[Vec<f32>],
    latency: usize,
) -> anyhow::Result<i32> {
    let out = output.out.clone().unwrap_or_else(|| {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("render");
        let stem = name.split('.').next().unwrap_or("render");
        PathBuf::from(format!("{stem}.wav"))
    });
    render::write_wav(&out, rate, channels)?;
    let stats = Stats::of(channels, rate, latency);
    if output.json {
        let mut v = serde_json::to_value(&stats)?;
        v["file"] = serde_json::json!(out);
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("wrote {}\n{}", out.display(), stats.text());
    }
    Ok(if stats.failed() {
        EXIT_INVALID
    } else {
        EXIT_OK
    })
}
