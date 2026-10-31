//! The project as data: what `project.json` holds.
//!
//! Everything here is plain `serde` data with no behaviour beyond lookups
//! and validation. Edits go through [`Command`](crate::Command) so that
//! every change has an inverse, which is what undo, the journal and crash
//! recovery are built on.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The format version this crate writes. The loader upgrades older files;
/// see `file.rs` for the migrations.
pub const FORMAT_VERSION: u32 = 1;

/// Every object in a project has an id that is unique across tracks,
/// clips and pedals and is never reused while the object exists.
pub type Id = u64;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Id);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a track.
    TrackId
);
id_type!(
    /// Identifies a clip.
    ClipId
);
id_type!(
    /// Identifies a pedal in a track's chain.
    PedalId
);

/// Tempo and time signature. One of each for the whole timeline for now;
/// the engine's tempo map can hold changes, and when the UI can edit
/// them this grows into a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tempo {
    pub bpm: f64,
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
        }
    }
}

/// A region of a file in `audio/` placed on the timeline. The file is
/// never modified; trimming a clip changes `offset` and `length`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    /// Path relative to the project's `audio/` directory.
    pub file: String,
    /// Timeline sample the clip starts at.
    pub start: u64,
    /// Frame into the file the clip starts from.
    pub offset: u64,
    /// Frames to play.
    pub length: u64,
}

impl Clip {
    pub fn end(&self) -> u64 {
        self.start + self.length
    }
}

/// A pedal in a chain: the spec the engine builds it from (a stock name,
/// `au:…` or `clap:…`) and the value of each parameter, in the order the
/// processor lists them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pedal {
    pub id: PedalId,
    pub spec: String,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    /// Armed tracks monitor their input and record when the transport
    /// records.
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub chain: Vec<Pedal>,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            armed: false,
            chain: Vec::new(),
            clips: Vec::new(),
        }
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn pedal(&self, id: PedalId) -> Option<&Pedal> {
        self.chain.iter().find(|p| p.id == id)
    }
}

/// A whole project. Saved as `project.json` inside a `Name.cantina/`
/// directory next to `audio/`, `plugins/` and `pedals/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub version: u32,
    pub name: String,
    /// The rate every file in `audio/` is at, and the rate the engine
    /// must run at to play them.
    pub sample_rate: f64,
    #[serde(default)]
    pub tempo: Tempo,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

impl Project {
    /// An empty project at the current format version.
    pub fn new(name: impl Into<String>, sample_rate: f64) -> Self {
        Self {
            version: FORMAT_VERSION,
            name: name.into(),
            sample_rate,
            tempo: Tempo::default(),
            tracks: Vec::new(),
        }
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// The track holding a clip, and the clip.
    pub fn clip(&self, id: ClipId) -> Option<(&Track, &Clip)> {
        self.tracks.iter().find_map(|t| t.clip(id).map(|c| (t, c)))
    }

    /// The track holding a pedal, and the pedal.
    pub fn pedal(&self, id: PedalId) -> Option<(&Track, &Pedal)> {
        self.tracks.iter().find_map(|t| t.pedal(id).map(|p| (t, p)))
    }

    /// Every id in use, across tracks, clips and pedals.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.tracks.iter().flat_map(|t| {
            std::iter::once(t.id.0)
                .chain(t.clips.iter().map(|c| c.id.0))
                .chain(t.chain.iter().map(|p| p.id.0))
        })
    }

    /// An id no object in the project has. Ids are not reused while an
    /// object lives, and a command that creates an object carries its id,
    /// so replaying a log recreates the same ids.
    pub fn fresh_id(&self) -> Id {
        self.ids().max().map_or(1, |max| max + 1)
    }

    /// Checks the invariants a file must satisfy: unique ids, clips with
    /// a length, finite parameter values and a usable tempo.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.sample_rate.is_finite() && self.sample_rate > 0.0) {
            return Err(format!("sample rate {} is not usable", self.sample_rate));
        }
        if !(self.tempo.bpm.is_finite() && self.tempo.bpm > 0.0) {
            return Err(format!("tempo {} bpm is not usable", self.tempo.bpm));
        }
        if self.tempo.numerator == 0 || self.tempo.denominator == 0 {
            return Err("the time signature has a zero in it".into());
        }
        let mut seen = std::collections::HashSet::new();
        for id in self.ids() {
            if !seen.insert(id) {
                return Err(format!("id {id} is used more than once"));
            }
        }
        for track in &self.tracks {
            for clip in &track.clips {
                if clip.length == 0 {
                    return Err(format!("clip {} ({}) has no length", clip.id, clip.name));
                }
                if clip.file.is_empty() || clip.file.contains("..") || clip.file.starts_with('/') {
                    return Err(format!(
                        "clip {} refers to {:?}, which is not a file in audio/",
                        clip.id, clip.file
                    ));
                }
            }
            for pedal in &track.chain {
                if pedal.values.iter().any(|v| !v.is_finite()) {
                    return Err(format!(
                        "pedal {} has a value that is not a number",
                        pedal.id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        let mut p = Project::new("Song", 48_000.0);
        let mut t = Track::new(TrackId(1), "Guitar");
        t.chain.push(Pedal {
            id: PedalId(2),
            spec: "gain".into(),
            values: vec![0.0],
        });
        t.clips.push(Clip {
            id: ClipId(3),
            name: "take 1".into(),
            file: "take-1.wav".into(),
            start: 0,
            offset: 0,
            length: 480,
        });
        p.tracks.push(t);
        p
    }

    #[test]
    fn fresh_ids_skip_every_object() {
        let p = project();
        assert_eq!(p.fresh_id(), 4);
        assert_eq!(Project::new("Empty", 48_000.0).fresh_id(), 1);
    }

    #[test]
    fn lookups_cross_tracks() {
        let p = project();
        assert_eq!(p.clip(ClipId(3)).unwrap().0.id, TrackId(1));
        assert_eq!(p.pedal(PedalId(2)).unwrap().1.spec, "gain");
        assert!(p.clip(ClipId(99)).is_none());
    }

    #[test]
    fn validation_catches_duplicates_and_bad_clips() {
        let mut p = project();
        assert_eq!(p.validate(), Ok(()));
        p.tracks[0].clips[0].id = ClipId(1);
        assert!(p.validate().unwrap_err().contains("more than once"));
        p.tracks[0].clips[0].id = ClipId(3);
        p.tracks[0].clips[0].length = 0;
        assert!(p.validate().unwrap_err().contains("no length"));
        p.tracks[0].clips[0].length = 1;
        p.tracks[0].clips[0].file = "../etc/passwd".into();
        assert!(p.validate().unwrap_err().contains("audio/"));
        p.tracks[0].clips[0].file = "ok.wav".into();
        p.tempo.bpm = 0.0;
        assert!(p.validate().unwrap_err().contains("tempo"));
    }

    #[test]
    fn serializes_in_camel_case_with_defaults() {
        let json = serde_json::to_value(project()).unwrap();
        assert_eq!(json["sampleRate"], 48_000.0);
        assert_eq!(json["tracks"][0]["clips"][0]["start"], 0);
        let minimal: Project =
            serde_json::from_str(r#"{"version":1,"name":"x","sampleRate":44100}"#).unwrap();
        assert_eq!(minimal.tempo, Tempo::default());
        assert!(minimal.tracks.is_empty());
    }
}
