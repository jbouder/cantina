//! What the shell remembers between launches.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Which devices to open and how. Devices are remembered by UID, which
/// survives replugging; `None` follows the system default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// Run output only: no input device is opened and every track's
    /// input is silence, for playing along through an amp in the room.
    pub no_input: bool,
    pub input_uid: Option<String>,
    pub output_uid: Option<String>,
    pub sample_rate: f64,
    pub buffer_frames: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            no_input: false,
            input_uid: None,
            output_uid: None,
            sample_rate: 48_000.0,
            buffer_frames: 128,
        }
    }
}

/// The metronome: whether it clicks while the transport plays, which
/// sound, and how loud. Mirrors `Metronome` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetronomeSettings {
    pub on: bool,
    /// One of the engine's click sounds, by name.
    pub sound: String,
    pub level_db: f32,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            on: false,
            sound: cantina_engine::CLICK_SOUNDS[0].to_string(),
            level_db: -6.0,
        }
    }
}

impl MetronomeSettings {
    /// The sound's index among the engine's sounds; the first when the
    /// name is not one of them.
    pub fn sound_index(&self) -> usize {
        cantina_engine::CLICK_SOUNDS
            .iter()
            .position(|s| *s == self.sound)
            .unwrap_or(0)
    }
}

/// The assistant's connection: off until the player turns it on. The API
/// key is not kept here but in the login Keychain (`assistant::keychain`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssistantSettings {
    /// Nothing is shown and nothing is sent anywhere until this is on.
    pub enabled: bool,
    /// The base URL of an OpenAI-compatible API, such as
    /// `https://api.openai.com/v1`; `/chat/completions` and `/models` are
    /// appended to it.
    pub endpoint: String,
    /// The model to ask, by the id the endpoint lists it under.
    pub model: String,
    /// Talks as Tapster, the house droid; off gives plain replies.
    pub persona: bool,
    /// Beeps and chirps in the window on a reply and on an error.
    pub sounds: bool,
}

impl Default for AssistantSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            model: String::new(),
            persona: true,
            sounds: false,
        }
    }
}

/// The pickups an instrument has: how hot it is into the interface
/// before anyone measures it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pickups {
    /// Vintage-output single coils: quieter than the rigs expect.
    SingleCoil,
    /// Passive humbuckers: the level the rigs are leveled for.
    Humbucker,
    /// Active pickups with a preamp: hotter.
    Active,
}

/// The peak, in dBFS, the stock rigs are leveled for: a passive
/// humbucker played hard into a DI, as the CLI's DI fixtures are.
pub const RIG_REFERENCE_PEAK_DB: f32 = -6.0;

/// The most a profile trims a rig's input either way.
pub const MAX_PROFILE_TRIM_DB: f32 = 18.0;

/// What the player plays through, so a rig's input gain suits it: the
/// instrument, its pickups and tuning, and the peak a calibration
/// measured playing hard. Mirrors `InstrumentProfile` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InstrumentProfile {
    pub instrument: cantina_project::Instrument,
    pub pickups: Pickups,
    /// The open strings from the lowest, as MIDI notes.
    pub tuning: Vec<u8>,
    /// The input's peak, in dBFS, over a few seconds of playing hard;
    /// none until the player calibrates.
    pub calibrated_peak_db: Option<f32>,
}

impl Default for InstrumentProfile {
    fn default() -> Self {
        Self {
            instrument: cantina_project::Instrument::Guitar,
            pickups: Pickups::Humbucker,
            tuning: vec![40, 45, 50, 55, 59, 64],
            calibrated_peak_db: None,
        }
    }
}

impl InstrumentProfile {
    /// Decibels a rig's input gain is trimmed by: what brings the
    /// calibrated peak to the level the rigs are leveled for or, before a
    /// calibration, a nominal amount for the pickups. To half a decibel,
    /// within ±18 dB.
    pub fn trim_db(&self) -> f32 {
        let trim = match self.calibrated_peak_db {
            Some(peak) if peak.is_finite() => RIG_REFERENCE_PEAK_DB - peak,
            _ => match self.pickups {
                Pickups::SingleCoil => 4.0,
                Pickups::Humbucker => 0.0,
                Pickups::Active => -6.0,
            },
        };
        ((trim * 2.0).round() / 2.0).clamp(-MAX_PROFILE_TRIM_DB, MAX_PROFILE_TRIM_DB)
    }
}

/// How long a jam's capture holds unless asked otherwise.
pub const DEFAULT_CAPTURE_MINUTES: f32 = 5.0;
/// The lengths the capture may be set to, in minutes.
pub const CAPTURE_MINUTES: [f32; 4] = [1.0, 2.0, 5.0, 10.0];

/// The exercise practiced last and how: opened again next time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeLast {
    /// The exercise's file.
    pub path: String,
    /// The tempo it was left at.
    pub tempo: f64,
    pub looping: bool,
    pub groove: bool,
    /// The rig the input played through in place of the exercise's.
    pub rig: Option<String>,
    /// Passes before the tempo climbs a rung; 0 holds it.
    #[serde(default = "four")]
    pub climb: u32,
}

fn four() -> u32 {
    4
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub audio: AudioSettings,
    /// Name of the project to open on launch: the one open at quit.
    pub last_project: Option<String>,
    /// Where each plugin's editor window was last, by spec, as the
    /// bottom-left corner in screen points.
    pub editor_positions: HashMap<String, (f64, f64)>,
    /// Latency compensation above this many milliseconds is flagged.
    pub latency_warn_ms: f32,
    pub metronome: MetronomeSettings,
    /// Projects opened lately, newest first, for File ▸ Open Recent.
    pub recent_projects: Vec<String>,
    /// Record counts in a bar of four beats before the take starts.
    pub count_in: bool,
    /// The assistant's endpoint and model, and whether it is on.
    pub assistant: AssistantSettings,
    /// What the player plays through; rigs trim their input from it.
    pub instrument: InstrumentProfile,
    /// The last jam, opened again next time.
    pub jam: Option<cantina_project::Jam>,
    /// Minutes of a jam's input kept for Keep that.
    pub capture_minutes: f32,
    /// The exercise practiced last, opened again next time.
    pub practice: Option<PracticeLast>,
    /// Skip the start dialog and open the last project at launch.
    pub open_last_at_launch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            last_project: None,
            editor_positions: HashMap::new(),
            latency_warn_ms: crate::engine::DEFAULT_LATENCY_WARN_MS,
            metronome: MetronomeSettings::default(),
            recent_projects: Vec::new(),
            count_in: true,
            assistant: AssistantSettings::default(),
            instrument: InstrumentProfile::default(),
            jam: None,
            capture_minutes: DEFAULT_CAPTURE_MINUTES,
            practice: None,
            open_last_at_launch: false,
        }
    }
}

impl Settings {
    /// Reads the file, or the defaults when it does not exist yet. A file
    /// that does not parse is reported, not silently replaced.
    pub fn load(path: &Path) -> Result<Settings> {
        match fs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_trims_to_the_rigs_reference_peak() {
        let mut p = InstrumentProfile::default();
        assert_eq!(p.trim_db(), 0.0, "a humbucker is what the rigs expect");
        p.pickups = Pickups::SingleCoil;
        assert_eq!(p.trim_db(), 4.0);
        p.pickups = Pickups::Active;
        assert_eq!(p.trim_db(), -6.0);
        // A measurement wins over the pickups, to half a decibel.
        p.calibrated_peak_db = Some(-1.2);
        assert_eq!(p.trim_db(), -5.0);
        p.calibrated_peak_db = Some(-14.3);
        assert_eq!(p.trim_db(), 8.5);
        p.calibrated_peak_db = Some(-80.0);
        assert_eq!(p.trim_db(), MAX_PROFILE_TRIM_DB, "held to range");
        // An old settings file has no profile: the default.
        let loaded: Settings = serde_json::from_str(r#"{"countIn":false}"#).unwrap();
        assert_eq!(loaded.instrument, InstrumentProfile::default());
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""pickups":"active""#), "{json}");
        assert!(json.contains(r#""calibratedPeakDb":-80.0"#), "{json}");
    }

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let dir = std::env::temp_dir().join(format!("cantina-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        let mut s = Settings::default();
        s.audio.input_uid = Some("AppleUSBAudioEngine:1".into());
        s.audio.buffer_frames = 64;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        fs::write(&path, r#"{"audio":{"sampleRate":96000}}"#).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.audio.sample_rate, 96_000.0);
        assert_eq!(loaded.audio.buffer_frames, 128);
        assert!(!loaded.audio.no_input);
        assert_eq!(loaded.metronome, MetronomeSettings::default());
        assert!(
            !loaded.open_last_at_launch,
            "the start dialog shows by default"
        );
        fs::write(&path, r#"{"metronome":{"on":true,"sound":"Cowbell"}}"#).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert!(loaded.metronome.on);
        assert_eq!(loaded.metronome.sound_index(), 2);
        assert_eq!(loaded.metronome.level_db, -6.0);
        let unknown = MetronomeSettings {
            sound: "Theremin".into(),
            ..MetronomeSettings::default()
        };
        assert_eq!(unknown.sound_index(), 0);
        // The assistant is off until turned on, and no key is ever written.
        assert!(!loaded.assistant.enabled);
        s.assistant = AssistantSettings {
            enabled: true,
            endpoint: "http://localhost:11434/v1".into(),
            model: "llama3.2".into(),
            ..AssistantSettings::default()
        };
        // The droid talks by default; its sounds are off until asked for.
        assert!(s.assistant.persona && !s.assistant.sounds);
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().assistant, s.assistant);
        assert!(!fs::read_to_string(&path).unwrap().contains("key"));
        fs::remove_dir_all(dir).unwrap();
    }
}
