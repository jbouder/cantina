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
    pub input_uid: Option<String>,
    pub output_uid: Option<String>,
    pub sample_rate: f64,
    pub buffer_frames: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input_uid: None,
            output_uid: None,
            sample_rate: 48_000.0,
            buffer_frames: 128,
        }
    }
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            last_project: None,
            editor_positions: HashMap::new(),
            latency_warn_ms: crate::engine::DEFAULT_LATENCY_WARN_MS,
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
        fs::remove_dir_all(dir).unwrap();
    }
}
