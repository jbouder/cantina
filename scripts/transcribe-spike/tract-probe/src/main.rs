//! Loads Basic Pitch's `nmp.onnx` with tract, runs it on windows of raw
//! little-endian f32 audio (22 050 Hz mono, 43 844 samples each) and
//! writes the three outputs (notes, onsets, contours) as raw f32 so the
//! evaluation script can compare them with ONNX Runtime.
//!
//!     cargo run --release -- nmp.onnx windows.f32 out.f32
use std::time::Instant;
use tract_onnx::prelude::*;

const WINDOW: usize = 43_844;

fn main() -> TractResult<()> {
    let args: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let model = tract_onnx::onnx()
        .model_for_path(&args[1])?
        .with_input_fact(0, f32::fact([1, WINDOW, 1]).into())?
        .into_optimized()?
        .into_runnable()?;
    eprintln!("load+optimise: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let bytes = std::fs::read(&args[2])?;
    let audio: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let mut out = Vec::new();
    let t = Instant::now();
    let windows = audio.len() / WINDOW;
    for w in audio.chunks_exact(WINDOW) {
        let input = tract_ndarray::Array3::from_shape_vec((1, WINDOW, 1), w.to_vec())?;
        let result = model.run(tvec!(Tensor::from(input).into()))?;
        // ONNX Runtime orders outputs as declared: :2 notes(88), :1 onsets(88), :0 contours(264).
        for r in result.iter() {
            out.extend_from_slice(r.try_as_plain_ram()?.as_slice::<f32>()?);
        }
    }
    let ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("{windows} windows in {ms:.1} ms ({:.1} ms per window of 1.99 s)", ms / windows as f64);
    let raw: Vec<u8> = out.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&args[3], raw)?;
    Ok(())
}
