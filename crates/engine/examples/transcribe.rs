//! Transcribes a recording with the app's model and note creation and
//! prints the notes as JSON: `[{"start", "end", "pitch", "amplitude",
//! "bend"}]`, times in seconds.
//!
//!     cargo run --release -p cantina-engine --example transcribe -- take.wav
//!
//! A path ending in `.f32` is read as raw little-endian mono at 22 050 Hz,
//! which is how the parity check feeds it exactly the audio Basic Pitch's
//! Python sees.

use std::path::Path;

use cantina_engine::{transcribe, wav};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("a WAV or .f32 file");
    let audio: Vec<f32> = if path.ends_with(".f32") {
        std::fs::read(&path)?
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    } else {
        let (info, channels) = wav::read_all(Path::new(&path))?;
        let n = channels[0].len();
        let mono: Vec<f32> = (0..n)
            .map(|i| channels.iter().map(|c| c[i]).sum::<f32>() / channels.len() as f32)
            .collect();
        cantina_dsp::resample(&mono, info.sample_rate as f64, transcribe::RATE)
    };
    let t = std::time::Instant::now();
    let model = transcribe::Transcriber::new()?;
    let maps = model.run(&audio, |_| true)?.expect("not stopped");
    let notes = transcribe::notes(&maps, transcribe::Options::default());
    eprintln!(
        "{} notes from {:.1} s of audio in {:.0} ms",
        notes.len(),
        audio.len() as f64 / transcribe::RATE,
        t.elapsed().as_secs_f64() * 1e3
    );
    let json: Vec<String> = notes
        .iter()
        .map(|n| {
            format!(
                r#"{{"start":{:.4},"end":{:.4},"pitch":{},"amplitude":{:.4},"bend":{:.3}}}"#,
                n.start, n.end, n.pitch, n.amplitude, n.bend
            )
        })
        .collect();
    println!("[{}]", json.join(","));
    Ok(())
}
