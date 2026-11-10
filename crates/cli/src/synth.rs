//! `synth-kit`: a kit recipe made into a kit folder, its samples as
//! 16-bit WAV and its `kit.json`, ready for `check` and for the app.
//!
//! The output depends only on the recipe, so running it again writes the
//! same bytes and a synthesized kit can live in git like any other.

use std::path::Path;

use anyhow::{bail, Context, Result};
use cantina_dsp::recipe::RECIPE_SAMPLE_RATE;
use cantina_dsp::{synthesize, Kit, KitRecipe};

/// Writes the kit `recipe` describes into `out`, creating it. Files the
/// kit does not use are left alone; the ones it does are overwritten.
pub fn synth_kit(recipe: &KitRecipe, out: &Path) -> Result<Kit> {
    let (kit, samples) = synthesize(recipe);
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    for sample in &samples {
        let path = out.join(&sample.file);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RECIPE_SAMPLE_RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec)
            .with_context(|| format!("creating {}", path.display()))?;
        for s in &sample.samples {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)?;
        }
        w.finalize()?;
    }
    let mut json = kit.to_json();
    json.push('\n');
    let path = out.join(cantina_engine::kit::KIT_FILE);
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(kit)
}

/// Reads a recipe file.
pub fn read_recipe(path: &Path) -> Result<KitRecipe> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    match KitRecipe::from_json(&text) {
        Ok(r) => Ok(r),
        Err(e) => bail!("{}: {e}", path.display()),
    }
}
