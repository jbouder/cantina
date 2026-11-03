//! The stock kit recipe in `recipes/` synthesised, written as a kit
//! folder, loaded the way the app loads kits, and played on the sampler:
//! every pad sounds, the hats choke, and the pads patterns share are
//! there, so a pattern written for a stock kit plays on it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cantina_dsp::{synthesize, Buffer, KitRecipe, Processor, Sampler};
use cantina_engine::{kit, wav};

fn recipe() -> KitRecipe {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/lofi.recipe.json");
    KitRecipe::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write_kit(recipe: &KitRecipe) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cantina-recipe-{}", std::process::id()));
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
fn the_stock_recipe_makes_a_kit_the_sampler_plays() {
    let dir = write_kit(&recipe());
    let (kit, loaded) = kit::load(&dir).unwrap();
    assert_eq!(kit.name, "Lo-fi (synth)");
    for pad in [
        "kick",
        "snare",
        "hh-closed",
        "hh-open",
        "tom-hi",
        "tom-lo",
        "clap",
    ] {
        assert!(loaded.pad_index(pad).is_some(), "no {pad} pad");
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
            assert!(peak > 0.01, "{} at {velocity} is silent", pad.id);
            assert!(peak <= 1.0, "{} at {velocity} clips", pad.id);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
