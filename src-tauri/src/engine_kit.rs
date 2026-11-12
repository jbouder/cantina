//! The drum designer's kit workbench: a copy of the part's kit to change
//! (each pad's level, pan, tune, decay and choke group, its samples, or
//! for a kit made from a recipe the recipe itself), heard as it changes
//! and saved as a kit of the user's. Nothing is written until it is
//! saved, and a kit folder is never changed in place.

use std::collections::BTreeMap;

use cantina_dsp::recipe::RECIPE_SAMPLE_RATE;
use cantina_dsp::{synthesize, Kit, KitRecipe, Sample};

use super::*;

/// Where an edited kit's samples come from.
#[derive(Clone)]
pub(super) enum SampleSource {
    /// A file on disk: in a kit folder, or one chosen to swap in.
    File(PathBuf),
    /// Audio made in memory, by a recipe.
    Made(Arc<Sample>),
}

/// The kit being changed in the workbench.
#[derive(Clone)]
pub(super) struct KitEdit {
    /// The kit it started from, by id.
    from: String,
    /// The kit as it is now.
    kit: Kit,
    /// What it was when it started, to say whether it changed.
    base: Kit,
    /// Each file the kit names, and where its audio comes from.
    sources: BTreeMap<String, SampleSource>,
    /// The recipe it was made from, when it was, as it is now.
    recipe: Option<KitRecipe>,
    /// The id it is heard under: a new one each change, so the designer's
    /// drum track takes the new kit.
    pub(super) heard: String,
}

/// The kit in the workbench, for the status. Mirrors `KitEditStatus` in
/// the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitEditStatus {
    /// The kit it started from, by id.
    pub from: String,
    pub kit: Kit,
    /// The recipe it is made from, when it is.
    pub recipe: Option<KitRecipe>,
    /// It differs from the kit it started from.
    pub changed: bool,
}

/// Where a pad's new sample comes from. Mirrors `PadSource` in the UI.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "from", rename_all = "camelCase")]
pub enum PadSource {
    /// A WAV file anywhere.
    File { path: String },
    /// Another kit's pad, every layer of it.
    Kit { kit: String, pad: String },
}

/// A pad of any installed kit, for swapping from. Mirrors `KitPadInfo`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitPadInfo {
    pub id: String,
    pub name: String,
}

/// The prefix of the ids an edited kit is heard under.
const EDIT_IDS: &str = "design:kit:";

/// A file name for a sample, free in `taken`: `stem.wav`, `stem-2.wav`…
fn free_name(stem: &str, taken: &BTreeMap<String, SampleSource>) -> String {
    let stem = crate::pedals::slug(stem);
    let first = format!("{stem}.wav");
    if !taken.contains_key(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{stem}-{n}.wav"))
        .find(|f| !taken.contains_key(f))
        .expect("some number is free")
}

impl KitEdit {
    /// The audio for each file, read or made, as the sampler takes it.
    fn load(&self) -> Result<LoadedKit> {
        let mut cache: HashMap<String, Arc<Sample>> = HashMap::new();
        let loaded = LoadedKit::build(&self.kit, |file| {
            if let Some(s) = cache.get(file) {
                return Ok(Arc::clone(s));
            }
            let sample = match self.sources.get(file) {
                Some(SampleSource::Made(s)) => Arc::clone(s),
                Some(SampleSource::File(path)) => {
                    let (info, channels) =
                        cantina_engine::wav::read_all(path).map_err(|e| format!("{e:#}"))?;
                    Arc::new(Sample {
                        sample_rate: f64::from(info.sample_rate),
                        channels,
                    })
                }
                None => return Err("is not in the kit".into()),
            };
            cache.insert(file.to_string(), Arc::clone(&sample));
            Ok(sample)
        })
        .map_err(|e| anyhow!("{e}"))?;
        Ok(loaded)
    }

    /// The recipe made into samples: the kit's pads become the recipe's,
    /// keeping each pad's tune and decay where the pad is still there.
    fn synthesize(&mut self, recipe: KitRecipe) -> Result<()> {
        recipe.validate().map_err(|e| anyhow!("{e}"))?;
        let (mut kit, samples) = synthesize(&recipe);
        for pad in &mut kit.pads {
            if let Some(was) = self.kit.pad(&pad.id) {
                pad.tune = was.tune;
                pad.decay = was.decay;
            }
        }
        kit.name = self.kit.name.clone();
        self.sources = samples
            .into_iter()
            .map(|s| {
                (
                    s.file,
                    SampleSource::Made(Arc::new(Sample {
                        sample_rate: f64::from(RECIPE_SAMPLE_RATE),
                        channels: vec![s.samples],
                    })),
                )
            })
            .collect();
        self.kit = kit;
        self.recipe = Some(recipe);
        Ok(())
    }
}

impl KitEdit {
    /// Writes the kit as a folder at `dir`, named `name`: its `kit.json`,
    /// every sample, and the recipe it was made from.
    fn write(&mut self, dir: &Path, name: &str) -> Result<()> {
        self.kit.name = name.to_string();
        self.kit.version = cantina_dsp::kit::KIT_VERSION;
        self.kit.validate().map_err(|e| anyhow!("{e}"))?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        for (file, source) in &self.sources {
            let to = dir.join(file);
            match source {
                SampleSource::File(from) => {
                    std::fs::copy(from, &to)
                        .with_context(|| format!("copying {}", from.display()))?;
                }
                SampleSource::Made(s) => {
                    cantina_engine::wav::write(&to, s.sample_rate, &s.channels)?;
                }
            }
        }
        let mut json = self.kit.to_json();
        json.push('\n');
        std::fs::write(dir.join(kit::KIT_FILE), json)?;
        if let Some(recipe) = &self.recipe {
            let mut recipe = recipe.clone();
            recipe.name = name.to_string();
            std::fs::write(dir.join("recipe.json"), recipe.to_json())?;
        }
        Ok(())
    }
}

impl Engine {
    /// The recipe a kit was made from: `recipe.json` in its folder, or a
    /// stock recipe of the same id.
    fn recipe_of(&self, id: &str, dir: &Path) -> Option<KitRecipe> {
        std::iter::once(dir.join("recipe.json"))
            .chain(
                self.recipe_roots
                    .iter()
                    .map(|r| r.join(format!("{id}.recipe.json"))),
            )
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .find_map(|t| KitRecipe::from_json(&t).ok())
    }

    /// The kit in the workbench, for the status.
    pub(super) fn kit_edit_status(&self) -> Option<KitEditStatus> {
        let edit = self.drum_design.as_ref()?.kit_edit.as_ref()?;
        Some(KitEditStatus {
            from: edit.from.clone(),
            kit: edit.kit.clone(),
            recipe: edit.recipe.clone(),
            changed: edit.kit != edit.base,
        })
    }

    /// Hears `edit` on the designer's drum track under a fresh id.
    fn kit_edit_install(&mut self, mut edit: KitEdit) -> Result<EngineStatus> {
        let loaded = edit.load()?;
        self.kit_serial += 1;
        edit.heard = format!("{EDIT_IDS}{}", self.kit_serial);
        self.kits.forget_prefix(EDIT_IDS);
        self.kits.put(&edit.heard, Arc::new(loaded));
        let design = self
            .drum_design
            .as_mut()
            .context("open the drum designer first")?;
        design.kit_edit = Some(edit);
        self.sync()?;
        Ok(self.publish())
    }

    /// The workbench's kit, starting it from the part's kit when it is
    /// not open yet.
    fn kit_edit_or_start(&self) -> Result<KitEdit> {
        let design = self
            .drum_design
            .as_ref()
            .context("open the drum designer first")?;
        if let Some(edit) = &design.kit_edit {
            return Ok(edit.clone());
        }
        let id = design.part.kit.clone();
        let entry = self
            .kits
            .entry(&id)
            .with_context(|| format!("the kit {id} is not installed"))?;
        let dir = entry.path.clone();
        let text = std::fs::read_to_string(dir.join(kit::KIT_FILE))
            .with_context(|| format!("reading {}", dir.display()))?;
        let kit = Kit::from_json(&text).map_err(|e| anyhow!("{e}"))?;
        let sources = kit
            .pads
            .iter()
            .flat_map(|p| &p.layers)
            .map(|l| (l.file.clone(), SampleSource::File(dir.join(&l.file))))
            .collect();
        Ok(KitEdit {
            from: id.clone(),
            recipe: self.recipe_of(&id, &dir),
            base: kit.clone(),
            kit,
            sources,
            heard: String::new(),
        })
    }

    /// Opens the workbench on the part's kit: a copy to change, heard as
    /// it changes.
    pub fn drum_design_kit_open(&mut self) -> Result<EngineStatus> {
        let edit = self.kit_edit_or_start()?;
        self.kit_edit_install(edit)
    }

    /// Sets each pad's level, pan, tune, decay and choke group, and the
    /// kit's name. The pads, their order and their samples stay.
    pub fn drum_design_kit_set(&mut self, kit: Kit) -> Result<EngineStatus> {
        let mut edit = self.kit_edit_or_start()?;
        kit.validate().map_err(|e| anyhow!("{e}"))?;
        if kit.pads.len() != edit.kit.pads.len()
            || kit
                .pads
                .iter()
                .zip(&edit.kit.pads)
                .any(|(a, b)| a.id != b.id || a.layers != b.layers)
        {
            bail!("the workbench changes pads' settings, not which pads or samples a kit has");
        }
        edit.kit = kit;
        self.kit_edit_install(edit)
    }

    /// Gives `pad` new samples: a WAV file, or another kit's pad.
    pub fn drum_design_kit_swap(&mut self, pad: &str, source: PadSource) -> Result<EngineStatus> {
        let mut edit = self.kit_edit_or_start()?;
        let index = edit
            .kit
            .pads
            .iter()
            .position(|p| p.id == pad)
            .with_context(|| format!("the kit has no pad {pad}"))?;
        let layers = match source {
            PadSource::File { path } => {
                let path = PathBuf::from(path);
                cantina_engine::wav::read_all(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let file = free_name(pad, &edit.sources);
                edit.sources.insert(file.clone(), SampleSource::File(path));
                vec![cantina_dsp::kit::Layer {
                    file,
                    velocity: [0.0, 1.0],
                    gain_db: 0.0,
                }]
            }
            PadSource::Kit {
                kit: other,
                pad: from,
            } => {
                let dir = self
                    .kits
                    .entry(&other)
                    .with_context(|| format!("the kit {other} is not installed"))?
                    .path
                    .clone();
                let text = std::fs::read_to_string(dir.join(kit::KIT_FILE))
                    .with_context(|| format!("reading {}", dir.display()))?;
                let source = Kit::from_json(&text).map_err(|e| anyhow!("{e}"))?;
                let source_pad = source
                    .pad(&from)
                    .with_context(|| format!("the kit {other} has no pad {from}"))?;
                let mut layers = Vec::new();
                for (i, l) in source_pad.layers.iter().enumerate() {
                    let file = free_name(&format!("{pad}-{other}-{}", i + 1), &edit.sources);
                    edit.sources
                        .insert(file.clone(), SampleSource::File(dir.join(&l.file)));
                    layers.push(cantina_dsp::kit::Layer { file, ..l.clone() });
                }
                layers
            }
        };
        edit.kit.pads[index].layers = layers;
        // Samples no layer names any more are dropped.
        let used: std::collections::HashSet<String> = edit
            .kit
            .pads
            .iter()
            .flat_map(|p| p.layers.iter().map(|l| l.file.clone()))
            .collect();
        edit.sources.retain(|f, _| used.contains(f));
        // A swapped pad is no longer the recipe's.
        edit.recipe = None;
        self.kit_edit_install(edit)
    }

    /// Makes the kit again from `recipe`.
    pub fn drum_design_kit_recipe(&mut self, recipe: KitRecipe) -> Result<EngineStatus> {
        let mut edit = self.kit_edit_or_start()?;
        edit.synthesize(recipe)?;
        self.kit_edit_install(edit)
    }

    /// Plays `pad` of the kit the designer hears, at `velocity`.
    pub fn drum_design_audition(&mut self, pad: &str, velocity: f32) -> Result<()> {
        if self.drum_design.is_none() {
            bail!("open the drum designer first");
        }
        self.pad_hit(drums_engine::DESIGN_TRACK.0, pad, velocity)
    }

    /// Puts the part's kit back as it was: the workbench closes.
    pub fn drum_design_kit_reset(&mut self) -> Result<EngineStatus> {
        if let Some(design) = &mut self.drum_design {
            design.kit_edit = None;
        }
        self.kits.forget_prefix(EDIT_IDS);
        self.sync()?;
        Ok(self.publish())
    }

    /// Saves the workbench's kit as a kit of the user's in
    /// `~/Music/Cantina/Kits/<id>`: its `kit.json`, every sample, and the
    /// recipe it was made from. The part plays it from then on.
    pub fn drum_design_kit_save(&mut self, name: &str, replace: bool) -> Result<EngineStatus> {
        let mut edit = self.kit_edit_or_start()?;
        let name = name.trim();
        if name.is_empty() {
            bail!("a kit needs a name");
        }
        let id = crate::pedals::slug(name);
        let dir = self.folders.kits.join(&id);
        if dir.exists() && !replace {
            bail!("you already have a kit called {name}");
        }
        if self
            .kits
            .entry(&id)
            .is_some_and(|e| !e.path.starts_with(&self.folders.kits))
        {
            bail!("{name} is the name of a stock kit; pick another");
        }
        // Written beside, then put in place, so a failure leaves the old
        // folder as it was.
        let tmp = self.folders.kits.join(format!(".{id}.saving"));
        let _ = std::fs::remove_dir_all(&tmp);
        if let Err(e) = edit.write(&tmp, name) {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("replacing {}", dir.display()))?;
        }
        std::fs::rename(&tmp, &dir).with_context(|| format!("writing {}", dir.display()))?;
        let mut problems = Vec::new();
        self.kits.rescan(&mut problems);
        self.kits.forget(&id);
        self.kits.forget_prefix(EDIT_IDS);
        if let Some(design) = &mut self.drum_design {
            design.kit_edit = None;
            design.part.kit = id;
        }
        self.library_status = self.folders.scan();
        self.sync()?;
        Ok(self.publish())
    }

    /// The pads of an installed kit, for swapping from.
    pub fn kit_pads(&self, id: &str) -> Result<Vec<KitPadInfo>> {
        let entry = self
            .kits
            .entry(id)
            .with_context(|| format!("the kit {id} is not installed"))?;
        let text = std::fs::read_to_string(entry.path.join(kit::KIT_FILE))?;
        let kit = Kit::from_json(&text).map_err(|e| anyhow!("{e}"))?;
        Ok(kit
            .pads
            .into_iter()
            .map(|p| KitPadInfo {
                id: p.id,
                name: p.name,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe() -> KitRecipe {
        KitRecipe::from_json(
            r#"{ "version": 1, "name": "Test", "seed": 3, "pads": [
                { "id": "kick", "voice": "kick" },
                { "id": "hh-closed", "voice": "hat", "choke": "hat" }
            ] }"#,
        )
        .unwrap()
    }

    fn edit() -> KitEdit {
        let mut e = KitEdit {
            from: "test".into(),
            kit: Kit {
                version: 2,
                name: "Test".into(),
                pads: Vec::new(),
            },
            base: Kit {
                version: 2,
                name: "Test".into(),
                pads: Vec::new(),
            },
            sources: BTreeMap::new(),
            recipe: None,
            heard: String::new(),
        };
        e.synthesize(recipe()).unwrap();
        e
    }

    #[test]
    fn a_recipe_makes_a_kit_that_loads_and_keeps_tune_and_decay() {
        let mut e = edit();
        assert_eq!(e.kit.pads.len(), 2);
        assert!(e
            .kit
            .pads
            .iter()
            .flat_map(|p| &p.layers)
            .all(|l| matches!(e.sources.get(&l.file), Some(SampleSource::Made(_)))));
        let loaded = e.load().unwrap();
        assert_eq!(loaded.pads().len(), 2);
        e.kit.pads[0].tune = -3.0;
        e.kit.pads[0].decay = 0.5;
        let mut r = recipe();
        r.pads[0].pitch = Some(70.0);
        e.synthesize(r.clone()).unwrap();
        assert_eq!((e.kit.pads[0].tune, e.kit.pads[0].decay), (-3.0, 0.5));
        assert_eq!(e.recipe, Some(r));
    }

    #[test]
    fn a_saved_kit_is_a_folder_that_loads_with_its_recipe() {
        let dir = std::env::temp_dir().join(format!("cantina-kit-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut e = edit();
        e.kit.pads[0].tune = 2.0;
        e.kit.pads[1].decay = 0.4;
        e.write(&dir, "My metal").unwrap();
        let (kit, loaded) = cantina_engine::kit::load(&dir).unwrap();
        assert_eq!(kit.name, "My metal");
        assert_eq!(kit.version, cantina_dsp::kit::KIT_VERSION);
        assert_eq!((kit.pads[0].tune, kit.pads[1].decay), (2.0, 0.4));
        assert_eq!(loaded.pads().len(), 2);
        let recipe = std::fs::read_to_string(dir.join("recipe.json")).unwrap();
        assert_eq!(KitRecipe::from_json(&recipe).unwrap().name, "My metal");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sample_names_never_clash() {
        let e = edit();
        let taken: Vec<_> = e.sources.keys().cloned().collect();
        let name = free_name("kick-1", &e.sources);
        assert!(!taken.contains(&name), "{name} in {taken:?}");
        assert!(name.ends_with(".wav"));
    }
}
