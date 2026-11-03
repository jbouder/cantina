//! The stock kit recipes in `recipes/` synthesised, written as a kit
//! folder, loaded the way the app loads kits, and played on the sampler:
//! every pad sounds, the hats choke, and the pads patterns share are
//! there, so a pattern written for a stock kit plays on it. The metal
//! recipe also ships synthesised, as `kits/metal`; that folder must be
//! what the recipe makes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cantina_dsp::{synthesize, Buffer, KitRecipe, Processor, Sampler};
use cantina_engine::{kit, wav};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn recipe(name: &str) -> KitRecipe {
    let path = root().join(format!("recipes/{name}.recipe.json"));
    KitRecipe::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write_kit(recipe: &KitRecipe, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cantina-recipe-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (kit, samples) = synthesize(recipe);
    for s in &samples {
        wav::write(
            &dir.join(&s.file),
            48_000.0,
            std::slice::from_ref(&s.samples),
        )
        .unwrap();
    }
    std::fs::write(dir.join(kit::KIT_FILE), kit.to_json()).unwrap();
    dir
}

#[test]
fn the_stock_recipes_make_kits_the_sampler_plays() {
    for (name, title, extra) in [
        ("lofi", "Lo-fi (synth)", "clap"),
        ("metal", "Metal (synth)", "china"),
    ] {
        sampler_plays(name, title, extra);
    }
}

fn sampler_plays(name: &str, title: &str, extra: &str) {
    let dir = write_kit(&recipe(name), name);
    let (kit, loaded) = kit::load(&dir).unwrap();
    assert_eq!(kit.name, title);
    for pad in [
        "kick",
        "snare",
        "hh-closed",
        "hh-open",
        "tom-hi",
        "tom-lo",
        extra,
    ] {
        assert!(loaded.pad_index(pad).is_some(), "{name}: no {pad} pad");
    }
    assert!(loaded.choke_groups().contains(&"hat".to_string()));
    let loaded = Arc::new(loaded);
    for (index, pad) in loaded.pads().iter().enumerate() {
        for velocity in [0.3, 1.0] {
            let mut sampler = Sampler::new(Arc::clone(&loaded));
            sampler.prepare(48_000.0, 512);
            assert!(sampler.trigger(index, velocity, 0));
            let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            let mut lanes: [&mut [f32]; 2] = [&mut l, &mut r];
            sampler.process(&mut Buffer::new(&mut lanes));
            let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.01, "{name}: {} at {velocity} is silent", pad.id);
            assert!(peak <= 1.0, "{name}: {} at {velocity} clips", pad.id);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// `kits/metal` is `recipes/metal.recipe.json` synthesised: the same
/// `kit.json`, and every sample the same to within the 16-bit rounding
/// `synth-kit` writes. Change the recipe, then run
/// `cantina-cli synth-kit recipes/metal.recipe.json kits/metal`.
#[test]
fn the_metal_kit_is_its_recipe_synthesised() {
    let (want, samples) = synthesize(&recipe("metal"));
    let (have, loaded) = kit::load(&root().join("kits/metal")).unwrap();
    assert_eq!(
        have, want,
        "kits/metal/kit.json is not what the recipe makes"
    );
    let mut index = 0;
    for pad in loaded.pads() {
        for layer in &pad.layers {
            let s = &samples[index];
            index += 1;
            let file = &layer.sample.channels[0];
            assert_eq!(
                file.len(),
                s.samples.len(),
                "{} has the wrong length",
                s.file
            );
            let worst = file
                .iter()
                .zip(&s.samples)
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(
                worst <= 2.0 / 32_767.0,
                "{} differs from the recipe by {worst}; regenerate kits/metal",
                s.file
            );
        }
    }
    assert_eq!(index, samples.len());
}
