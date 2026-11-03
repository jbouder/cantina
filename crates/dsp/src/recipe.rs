//! Kit recipes: a whole drum kit described by a few numbers per pad and
//! synthesised into samples, for when there is nothing to record.
//!
//! A recipe names each pad's voice (`kick`, `snare`, `hat`, `clap`, `tom`
//! or `cowbell`) and shapes it with `pitch`, `decay`, `tone` and `noise`.
//! [`synthesize`] turns it into one sample per velocity layer plus the
//! [`Kit`] that maps them, which whoever can write files saves as a kit
//! folder. The noise comes from a generator seeded by the recipe's `seed`,
//! the pad's id and the layer, so the same recipe always gives the same
//! samples, bit for bit, and a kit made from one round-trips through git.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Lo-fi",
//!   "seed": 7,
//!   "pads": [
//!     { "id": "kick", "voice": "kick", "pitch": 52, "decay": 0.45 },
//!     { "id": "hh-closed", "voice": "hat", "decay": 0.05, "choke": "hat" },
//!     { "id": "hh-open", "voice": "hat", "decay": 0.4, "choke": "hat" }
//!   ]
//! }
//! ```
//!
//! Soft layers are the same voice played gentler: darker, a little
//! shorter and with less attack, not just quieter, because the sampler
//! already scales by velocity.

use serde::{Deserialize, Serialize};

use crate::biquad::Biquad;
use crate::kit::{Kit, Layer, Pad, MAX_LAYERS, MAX_PADS};

/// The recipe format version this crate reads.
pub const RECIPE_VERSION: u32 = 1;

/// The rate recipes are synthesised at.
pub const RECIPE_SAMPLE_RATE: f32 = 48_000.0;

/// The longest decay a pad may ask for, in seconds.
pub const MAX_DECAY: f32 = 4.0;

/// A kit described as synthesis settings rather than samples.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KitRecipe {
    /// Ignored; lets an editor find the schema.
    #[serde(default, rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The format version, [`RECIPE_VERSION`].
    pub version: u32,
    /// The kit's name, shown in the kit picker.
    pub name: String,
    /// Seeds the noise. The same recipe and seed give the same samples.
    #[serde(default)]
    pub seed: u32,
    /// The pads, in the order the sequencer shows them.
    pub pads: Vec<PadRecipe>,
}

/// What a pad is synthesised as.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Voice {
    /// A sine dropping in pitch with a click on top.
    Kick,
    /// A pitched body under a burst of bright noise.
    Snare,
    /// Six detuned square waves and noise, high-passed: closed with a
    /// short decay, open with a long one.
    Hat,
    /// A few bursts of band-limited noise and a short tail.
    Clap,
    /// A sine with a gentle drop in pitch and a little noise.
    Tom,
    /// Two square waves through a band.
    Cowbell,
}

impl Voice {
    /// The pitch, decay, tone and noise a voice starts from.
    pub fn defaults(self) -> (f32, f32, f32, f32) {
        match self {
            Voice::Kick => (55.0, 0.5, 0.5, 0.1),
            Voice::Snare => (190.0, 0.25, 0.6, 0.6),
            Voice::Hat => (400.0, 0.08, 0.7, 0.5),
            Voice::Clap => (1200.0, 0.3, 0.5, 1.0),
            Voice::Tom => (120.0, 0.4, 0.5, 0.15),
            Voice::Cowbell => (560.0, 0.3, 0.6, 0.0),
        }
    }
}

/// One pad of a recipe. Every number is optional and starts at the
/// voice's default.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PadRecipe {
    /// The pad id patterns refer to, such as `kick` or `hh-closed`. Also
    /// the stem of its sample files.
    pub id: String,
    /// Display name; the id when missing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What it is synthesised as.
    pub voice: Voice,
    /// The fundamental, in Hz: where a kick or tom settles, a snare's
    /// body, a hat's lowest partial, a clap's band, a cowbell's lower tone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    /// Time to fall by 60 dB, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay: Option<f32>,
    /// Brightness from 0 (dark) to 1 (bright): the click on a kick, the
    /// noise's colour on a snare, the cut-off of a hat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<f32>,
    /// How much noise is blended with the pitched part, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<f32>,
    /// Velocity layers from 1 to 4, split evenly across the range: a soft
    /// one at the bottom, the full hit at the top. 2 when missing.
    #[serde(default = "two", skip_serializing_if = "is_two")]
    pub layers: u8,
    /// Gain in decibels.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f32,
    /// -1 is hard left, 1 hard right.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,
    /// Pads in the same choke group cut each other, as in the kit format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke: Option<String>,
}

fn two() -> u8 {
    2
}

fn is_two(n: &u8) -> bool {
    *n == 2
}

fn is_zero(x: &f32) -> bool {
    *x == 0.0
}

/// Most velocity layers a recipe pad may ask for.
pub const MAX_RECIPE_LAYERS: u8 = 4;

/// A synthesised sample: the file name it goes in, and mono samples at
/// [`RECIPE_SAMPLE_RATE`].
#[derive(Debug, Clone, PartialEq)]
pub struct SynthSample {
    /// A plain file name, `<pad id>-<layer>.wav`.
    pub file: String,
    /// The audio.
    pub samples: Vec<f32>,
}

impl KitRecipe {
    /// Reads and checks a recipe.
    pub fn from_json(text: &str) -> Result<KitRecipe, String> {
        let recipe: KitRecipe =
            serde_json::from_str(text).map_err(|e| format!("not a kit recipe: {e}"))?;
        recipe.validate()?;
        Ok(recipe)
    }

    /// The recipe as pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a recipe serializes")
    }

    /// Checks the rules beyond the shape: the version, ids that make
    /// plain file names, and every number in its range.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != RECIPE_VERSION {
            return Err(format!(
                "version {} is not supported; this Cantina reads recipe version {RECIPE_VERSION}",
                self.version
            ));
        }
        if self.name.trim().is_empty() {
            return Err("the recipe has no name".into());
        }
        if self.pads.is_empty() || self.pads.len() > MAX_PADS {
            return Err(format!(
                "the recipe has {} pads; 1 to {MAX_PADS} are allowed",
                self.pads.len()
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for pad in &self.pads {
            let id = &pad.id;
            if id.is_empty()
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!(
                    "pad id {id:?} must be letters, digits, '-' or '_', as it names the sample files"
                ));
            }
            if !ids.insert(id.as_str()) {
                return Err(format!("pad id {id:?} is used more than once"));
            }
            let range = |field: &str, v: Option<f32>, lo: f32, hi: f32| match v {
                Some(x) if !(x.is_finite() && (lo..=hi).contains(&x)) => Err(format!(
                    "pad {id:?} has a {field} of {x}, outside {lo} to {hi}"
                )),
                _ => Ok(()),
            };
            range("pitch", pad.pitch, 20.0, 12_000.0)?;
            range("decay", pad.decay, 0.005, MAX_DECAY)?;
            range("tone", pad.tone, 0.0, 1.0)?;
            range("noise", pad.noise, 0.0, 1.0)?;
            range("gainDb", Some(pad.gain_db), -60.0, 12.0)?;
            range("pan", Some(pad.pan), -1.0, 1.0)?;
            if pad.layers == 0 || pad.layers > MAX_RECIPE_LAYERS {
                return Err(format!(
                    "pad {id:?} asks for {} layers; 1 to {MAX_RECIPE_LAYERS} are allowed",
                    pad.layers
                ));
            }
            if pad.choke.as_deref().is_some_and(|c| c.trim().is_empty()) {
                return Err(format!("pad {id:?} has an empty choke group"));
            }
        }
        debug_assert!(MAX_RECIPE_LAYERS as usize <= MAX_LAYERS);
        Ok(())
    }
}

/// Synthesises every pad's layers and the kit that maps them. The kit's
/// layer files are the samples' names.
pub fn synthesize(recipe: &KitRecipe) -> (Kit, Vec<SynthSample>) {
    let mut samples = Vec::new();
    let mut pads = Vec::new();
    for pad in &recipe.pads {
        let (pitch, decay, tone, noise) = pad.voice.defaults();
        let base = Settings {
            pitch: pad.pitch.unwrap_or(pitch),
            decay: pad.decay.unwrap_or(decay),
            tone: pad.tone.unwrap_or(tone),
            noise: pad.noise.unwrap_or(noise),
            force: 1.0,
        };
        let n = pad.layers.max(1) as usize;
        let mut layers = Vec::new();
        for layer in 0..n {
            // Layer 0 is the softest; the top layer is the full hit.
            let force = if n == 1 {
                1.0
            } else {
                0.45 + 0.55 * layer as f32 / (n - 1) as f32
            };
            let settings = Settings { force, ..base };
            let seed = mix(recipe.seed, &pad.id, layer as u32);
            let audio = render(pad.voice, &settings, seed);
            let file = if n == 1 {
                format!("{}.wav", pad.id)
            } else {
                format!("{}-{}.wav", pad.id, layer + 1)
            };
            let lo = layer as f32 / n as f32;
            let hi = (layer + 1) as f32 / n as f32;
            layers.push(Layer {
                file: file.clone(),
                velocity: [lo, hi],
                gain_db: 0.0,
            });
            samples.push(SynthSample {
                file,
                samples: audio,
            });
        }
        pads.push(Pad {
            id: pad.id.clone(),
            name: if pad.name.is_empty() {
                pad.id.clone()
            } else {
                pad.name.clone()
            },
            gain_db: pad.gain_db,
            pan: pad.pan,
            choke: pad.choke.clone(),
            layers,
        });
    }
    let kit = Kit {
        version: crate::kit::KIT_VERSION,
        name: recipe.name.clone(),
        pads,
    };
    (kit, samples)
}

#[derive(Debug, Clone, Copy)]
struct Settings {
    pitch: f32,
    decay: f32,
    tone: f32,
    noise: f32,
    /// 1 for the full hit, less for a softer layer.
    force: f32,
}

/// A small, fast, seedable noise source. Not for cryptography; only for
/// samples that must come out the same every time.
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform in -1 to 1.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// The seed for one layer of one pad: FNV-1a over the id, mixed with the
/// recipe's seed and the layer.
fn mix(seed: u32, id: &str, layer: u32) -> u32 {
    let mut h = 0x811c_9dc5u32 ^ seed.wrapping_mul(0x9E37_79B9);
    for b in id.bytes().chain(layer.to_le_bytes()) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Amplitude after `t` seconds of a decay that falls 60 dB in `decay`.
fn env(t: f32, decay: f32) -> f32 {
    (-6.9078 * t / decay.max(1e-4)).exp()
}

fn render(voice: Voice, s: &Settings, seed: u32) -> Vec<f32> {
    let rate = RECIPE_SAMPLE_RATE;
    // Softer hits ring a little shorter.
    let decay = s.decay * (0.8 + 0.2 * s.force);
    // Long enough for the slowest part to fall 60 dB, plus a little.
    let tail = match voice {
        Voice::Snare | Voice::Clap => decay * 1.2,
        _ => decay,
    };
    let frames = ((tail + 0.01) * rate) as usize;
    let mut noise = Noise::new(seed);
    let mut out = vec![0.0f32; frames];
    let tau = std::f32::consts::TAU;
    match voice {
        Voice::Kick | Voice::Tom => {
            let (sweep, sweep_time) = if voice == Voice::Kick {
                (3.0, 0.035)
            } else {
                (0.6, 0.06)
            };
            let sweep = sweep * (0.6 + 0.4 * s.force);
            let mut phase = 0.0f32;
            let mut click = Biquad::identity();
            click.set_lowpass(rate, 1500.0 + 8000.0 * s.tone * s.force, 0.7);
            let mut hiss = Biquad::identity();
            hiss.set_lowpass(rate, 2000.0 + 6000.0 * s.tone, 0.7);
            for (i, y) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let f = s.pitch * (1.0 + sweep * (-t / sweep_time).exp());
                phase = (phase + f / rate).fract();
                let body = (tau * phase).sin() * env(t, decay);
                let attack = click.tick(noise.next()) * env(t, 0.012) * s.tone * s.force;
                let air = hiss.tick(noise.next()) * env(t, decay * 0.4) * s.noise;
                *y = body + 0.6 * attack + 0.4 * air;
            }
        }
        Voice::Snare => {
            let mut hp = Biquad::identity();
            hp.set_highpass(rate, 1200.0 + 2500.0 * s.tone, 0.7);
            let mut lp = Biquad::identity();
            lp.set_lowpass(rate, 5000.0 + 9000.0 * s.tone * s.force, 0.7);
            let (mut p1, mut p2) = (0.0f32, 0.0f32);
            for (i, y) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let f = s.pitch * (1.0 + 0.3 * (-t / 0.02).exp());
                p1 = (p1 + f / rate).fract();
                p2 = (p2 + f * 1.58 / rate).fract();
                let body = ((tau * p1).sin() + 0.5 * (tau * p2).sin()) * env(t, decay * 0.5);
                let snares = lp.tick(hp.tick(noise.next())) * env(t, decay);
                *y = (1.0 - 0.5 * s.noise) * body + 1.6 * s.noise * snares;
            }
        }
        Voice::Hat | Voice::Cowbell => {
            // Partials as ratios of the pitch: the 808's six for a hat,
            // two for a cowbell.
            const HAT: [f32; 6] = [1.0, 1.4471, 1.6170, 1.9265, 2.5028, 2.6637];
            const BELL: [f32; 2] = [1.0, 1.4799];
            let ratios: &[f32] = if voice == Voice::Hat { &HAT } else { &BELL };
            let mut phases = [0.0f32; 6];
            let mut hp = Biquad::identity();
            let mut lp = Biquad::identity();
            if voice == Voice::Hat {
                hp.set_highpass(rate, 5000.0 + 3000.0 * s.tone, 0.8);
                lp.set_lowpass(rate, 9000.0 + 9000.0 * s.tone * s.force, 0.7);
            } else {
                hp.set_highpass(rate, s.pitch * 0.9, 1.2);
                lp.set_lowpass(rate, s.pitch * (2.0 + 2.0 * s.tone), 1.2);
            }
            for (i, y) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut metal = 0.0;
                for (k, r) in ratios.iter().enumerate() {
                    phases[k] = (phases[k] + s.pitch * r / rate).fract();
                    metal += if phases[k] < 0.5 { 1.0 } else { -1.0 };
                }
                metal /= ratios.len() as f32;
                let x = (1.0 - s.noise) * metal + s.noise * noise.next();
                let d = if voice == Voice::Cowbell {
                    // A bell's quick drop, then its ring.
                    0.6 * env(t, decay * 0.15) + 0.4 * env(t, decay)
                } else {
                    env(t, decay)
                };
                *y = lp.tick(hp.tick(x)) * d;
            }
        }
        Voice::Clap => {
            let mut hp = Biquad::identity();
            hp.set_highpass(rate, s.pitch * 0.7, 1.0);
            let mut lp = Biquad::identity();
            lp.set_lowpass(rate, s.pitch * (1.6 + 2.0 * s.tone), 1.0);
            // Three hands before the body, 9 ms apart.
            let bursts = [0.0f32, 0.009, 0.018];
            for (i, y) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut e = 0.0f32;
                for b in bursts {
                    if t >= b {
                        e = e.max(env(t - b, 0.03));
                    }
                }
                let body = if t >= 0.027 {
                    0.7 * env(t - 0.027, decay)
                } else {
                    0.0
                };
                let x = noise.next() * (1.0 - 0.5 * (1.0 - s.noise));
                *y = lp.tick(hp.tick(x)) * e.max(body);
            }
        }
    }
    finish(&mut out, s.force);
    out
}

/// Blocks any offset, fades in over half a millisecond and out over the
/// last five, and normalises the peak: the full hit to -1 dBFS, softer
/// layers a little below so the change of layer is not a jump.
fn finish(out: &mut [f32], force: f32) {
    let mut dc = Biquad::identity();
    dc.set_highpass(RECIPE_SAMPLE_RATE, 15.0, 0.7);
    dc.process(out);
    let n = out.len();
    // Half a millisecond in, so the first sample is silence, not a step.
    let rise = ((0.0005 * RECIPE_SAMPLE_RATE) as usize).min(n);
    for (i, s) in out.iter_mut().take(rise).enumerate() {
        *s *= i as f32 / rise as f32;
    }
    let fade = ((0.005 * RECIPE_SAMPLE_RATE) as usize).min(n);
    for i in 0..fade {
        out[n - fade + i] *= 1.0 - (i + 1) as f32 / fade as f32;
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let target = 0.891 * (0.7 + 0.3 * force);
        let k = target / peak;
        out.iter_mut().for_each(|s| *s *= k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe() -> KitRecipe {
        KitRecipe::from_json(
            r#"{ "version": 1, "name": "Test", "seed": 3, "pads": [
              { "id": "kick", "voice": "kick" },
              { "id": "snare", "voice": "snare" },
              { "id": "hh-closed", "voice": "hat", "decay": 0.05, "choke": "hat" },
              { "id": "hh-open", "voice": "hat", "decay": 0.5, "choke": "hat" },
              { "id": "clap", "voice": "clap" },
              { "id": "tom", "voice": "tom", "layers": 3 },
              { "id": "bell", "voice": "cowbell", "layers": 1 }
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn every_voice_makes_bounded_audible_layers() {
        let (kit, samples) = synthesize(&recipe());
        kit.validate().unwrap();
        assert_eq!(samples.len(), 2 * 5 + 3 + 1);
        for s in &samples {
            let peak = s.samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.5 && peak < 0.9, "{} peaks at {peak}", s.file);
            assert!(s.samples.iter().all(|x| x.is_finite()), "{}", s.file);
            assert_eq!(s.samples[0], 0.0, "{} starts at zero", s.file);
            assert!(s.samples.last().unwrap().abs() < 1e-3, "{} ends", s.file);
        }
        let tom = kit.pad("tom").unwrap();
        assert_eq!(tom.layers.len(), 3);
        assert_eq!(tom.layers[0].velocity, [0.0, 1.0 / 3.0]);
        assert_eq!(tom.layers[2].velocity[1], 1.0);
        assert_eq!(kit.pad("bell").unwrap().layers[0].file, "bell.wav");
        assert_eq!(kit.pad("hh-open").unwrap().choke.as_deref(), Some("hat"));
    }

    #[test]
    fn decay_sets_the_length_and_soft_layers_are_darker() {
        let (_, samples) = synthesize(&recipe());
        let len = |f: &str| samples.iter().find(|s| s.file == f).unwrap().samples.len();
        assert!(len("hh-open-2.wav") > 5 * len("hh-closed-2.wav"));
        // Brightness as the share of energy in sample-to-sample change.
        let bright = |f: &str| {
            let x = &samples.iter().find(|s| s.file == f).unwrap().samples;
            let d: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
            let e: f32 = x.iter().map(|v| v * v).sum();
            d / e
        };
        assert!(bright("snare-1.wav") < bright("snare-2.wav"));
    }

    #[test]
    fn the_same_recipe_gives_the_same_samples_and_the_seed_changes_them() {
        let a = synthesize(&recipe());
        assert_eq!(a, synthesize(&recipe()));
        let mut other = recipe();
        other.seed = 4;
        let b = synthesize(&other);
        let snare = |s: &[SynthSample]| s.iter().find(|x| x.file == "snare-2.wav").unwrap().clone();
        assert_ne!(snare(&a.1), snare(&b.1));
        // Different pads with the same voice do not share their noise.
        assert_ne!(mix(3, "a", 0), mix(3, "b", 0));
        assert_ne!(mix(3, "a", 0), mix(3, "a", 1));
    }

    #[test]
    fn bad_recipes_say_what_is_wrong() {
        let check = |json: &str, expected: &str| {
            let err = KitRecipe::from_json(json).unwrap_err();
            assert!(err.contains(expected), "{err}");
        };
        let pad = |p: &str| format!(r#"{{"version":1,"name":"K","pads":[{p}]}}"#);
        check(&pad(r#"{"id":"k","voice":"kick","decay":9}"#), "decay of 9");
        check(&pad(r#"{"id":"k","voice":"kick","tone":-1}"#), "tone of -1");
        check(&pad(r#"{"id":"../k","voice":"kick"}"#), "letters, digits");
        check(&pad(r#"{"id":"k","voice":"kick","layers":5}"#), "5 layers");
        check(&pad(r#"{"id":"k","voice":"gong"}"#), "unknown variant");
        check(
            &pad(r#"{"id":"k","voice":"kick","pich":50}"#),
            "unknown field",
        );
        check(
            &pad(r#"{"id":"k","voice":"kick"},{"id":"k","voice":"tom"}"#),
            "more than once",
        );
        check(r#"{"version":2,"name":"K","pads":[]}"#, "version 2");
        check(r#"{"version":1,"name":"K","pads":[]}"#, "0 pads");
    }
}
