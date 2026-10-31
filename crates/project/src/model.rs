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
pub const FORMAT_VERSION: u32 = 4;

/// Ticks per quarter note in pattern and pattern clip positions. The
/// same as the engine's tempo map, so a tick here is a tick there.
pub const PPQ: u32 = 960;

/// Most steps a pattern may have.
pub const MAX_PATTERN_STEPS: usize = 128;

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
id_type!(
    /// Identifies a drum pattern.
    PatternId
);
id_type!(
    /// Identifies a pattern clip on a drum track.
    PatternClipId
);

/// A pattern placed on a drum track. Positions are in ticks, so the
/// clip follows the tempo: change the tempo and it stays on its bars.
/// The pattern loops inside the clip; `offset` ticks into the pattern
/// play first, so trimming the front keeps the beats where they were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternClip {
    pub id: PatternClipId,
    pub pattern: PatternId,
    /// Timeline tick the clip starts at.
    pub start: u64,
    /// Ticks into the pattern it starts from.
    pub offset: u64,
    /// Ticks long.
    pub length: u64,
}

impl PatternClip {
    pub fn end(&self) -> u64 {
        self.start + self.length
    }
}

/// A step sequencer pattern: a grid of pads against steps, with a
/// velocity per cell. Patterns belong to the project and refer to pads
/// by the ids the kit format uses (`kick`, `snare`, `hh-closed`…), so a
/// pattern plays on any drum track whose kit has those pads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    /// Steps per quarter note: 2 for eighths, 4 for sixteenths, 8 for
    /// thirty-seconds; 3 and 6 for triplets.
    pub steps_per_beat: u8,
    /// Steps in the pattern, which loops after the last.
    pub length: u16,
    /// 0 is straight; 1 delays every second step by a third of a step,
    /// a triplet feel.
    pub swing: f32,
    /// One row per pad that has hits.
    #[serde(default)]
    pub rows: Vec<PatternRow>,
}

/// One pad's steps. `steps` holds exactly the pattern's length of
/// velocities from 0 (no hit) to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternRow {
    pub pad: String,
    pub steps: Vec<f32>,
}

impl Pattern {
    /// An empty pattern of one bar of sixteenths in 4/4.
    pub fn new(id: PatternId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            steps_per_beat: 4,
            length: 16,
            swing: 0.0,
            rows: Vec::new(),
        }
    }

    /// Ticks per step.
    pub fn step_ticks(&self) -> u64 {
        PPQ as u64 / self.steps_per_beat.max(1) as u64
    }

    /// Ticks in one pass of the pattern.
    pub fn ticks(&self) -> u64 {
        self.step_ticks() * self.length.max(1) as u64
    }

    pub fn row(&self, pad: &str) -> Option<&PatternRow> {
        self.rows.iter().find(|r| r.pad == pad)
    }

    /// The velocity at a cell; 0 for a pad with no row.
    pub fn velocity(&self, pad: &str, step: usize) -> f32 {
        self.row(pad)
            .and_then(|r| r.steps.get(step))
            .copied()
            .unwrap_or(0.0)
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err(format!("pattern {} has no name", self.id));
        }
        if !matches!(self.steps_per_beat, 1 | 2 | 3 | 4 | 6 | 8) {
            return Err(format!(
                "pattern {} has {} steps per beat; 1, 2, 3, 4, 6 or 8 are allowed",
                self.id, self.steps_per_beat
            ));
        }
        if self.length == 0 || self.length as usize > MAX_PATTERN_STEPS {
            return Err(format!(
                "pattern {} has {} steps; 1 to {MAX_PATTERN_STEPS} are allowed",
                self.id, self.length
            ));
        }
        if !self.swing.is_finite() || !(0.0..=1.0).contains(&self.swing) {
            return Err(format!(
                "pattern {} has a swing of {}, outside 0 to 1",
                self.id, self.swing
            ));
        }
        let mut pads = std::collections::HashSet::new();
        for row in &self.rows {
            if row.pad.is_empty() {
                return Err(format!("pattern {} has a row with no pad", self.id));
            }
            if !pads.insert(row.pad.as_str()) {
                return Err(format!(
                    "pattern {} has two rows for pad {:?}",
                    self.id, row.pad
                ));
            }
            if row.steps.len() != self.length as usize {
                return Err(format!(
                    "pattern {} row {:?} has {} steps, the pattern {}",
                    self.id,
                    row.pad,
                    row.steps.len(),
                    self.length
                ));
            }
            if row
                .steps
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(format!(
                    "pattern {} row {:?} has a velocity outside 0 to 1",
                    self.id, row.pad
                ));
            }
        }
        Ok(())
    }
}

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

/// A mixer strip: what a track, or the master, does to its sum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Strip {
    pub gain_db: f32,
    /// -1 is hard left, 1 hard right. Ignored on the master.
    pub pan: f32,
    pub mute: bool,
    /// Ignored on the master.
    pub solo: bool,
}

impl Default for Strip {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
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
    /// The drum kit, by id, for a drum track. An audio track has none.
    /// A drum track plays patterns through its kit instead of audio
    /// from its input and clips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kit: Option<String>,
    #[serde(default)]
    pub chain: Vec<Pedal>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    /// Patterns on the timeline, for a drum track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_clips: Vec<PatternClip>,
    #[serde(default)]
    pub strip: Strip,
}

impl Track {
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            armed: false,
            kit: None,
            chain: Vec::new(),
            clips: Vec::new(),
            pattern_clips: Vec::new(),
            strip: Strip::default(),
        }
    }

    /// A drum track playing `kit`.
    pub fn drums(id: TrackId, name: impl Into<String>, kit: impl Into<String>) -> Self {
        Self {
            kit: Some(kit.into()),
            ..Self::new(id, name)
        }
    }

    pub fn is_drums(&self) -> bool {
        self.kit.is_some()
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn pedal(&self, id: PedalId) -> Option<&Pedal> {
        self.chain.iter().find(|p| p.id == id)
    }

    pub fn pattern_clip(&self, id: PatternClipId) -> Option<&PatternClip> {
        self.pattern_clips.iter().find(|c| c.id == id)
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
    /// Drum patterns, shared by every drum track.
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    /// The master strip: gain and mute on the sum of the tracks.
    #[serde(default)]
    pub master: Strip,
    /// Edits applied since the project was created. Commands do not
    /// touch it; the shell bumps it with each one and the journal records
    /// which revision each line brought the project to, so on open the
    /// lines newer than the saved file are known.
    #[serde(default)]
    pub revision: u64,
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
            patterns: Vec::new(),
            master: Strip::default(),
            revision: 0,
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

    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    pub fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id)
    }

    /// The track holding a pattern clip, and the clip.
    pub fn pattern_clip(&self, id: PatternClipId) -> Option<(&Track, &PatternClip)> {
        self.tracks
            .iter()
            .find_map(|t| t.pattern_clip(id).map(|c| (t, c)))
    }

    /// True when a pattern is on the timeline somewhere.
    pub fn pattern_in_use(&self, id: PatternId) -> bool {
        self.tracks
            .iter()
            .any(|t| t.pattern_clips.iter().any(|c| c.pattern == id))
    }

    /// Every id in use, across tracks, clips, pedals and patterns.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.tracks
            .iter()
            .flat_map(|t| {
                std::iter::once(t.id.0)
                    .chain(t.clips.iter().map(|c| c.id.0))
                    .chain(t.chain.iter().map(|p| p.id.0))
                    .chain(t.pattern_clips.iter().map(|c| c.id.0))
            })
            .chain(self.patterns.iter().map(|p| p.id.0))
    }

    /// An id no object in the project has. Ids are not reused while an
    /// object lives, and a command that creates an object carries its id,
    /// so replaying a log recreates the same ids.
    pub fn fresh_id(&self) -> Id {
        self.ids().max().map_or(1, |max| max + 1)
    }

    /// Checks the invariants a file must satisfy: unique ids, clips with
    /// a length, finite parameter values, well-formed patterns and a
    /// usable tempo.
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
        for strip in self.tracks.iter().map(|t| &t.strip).chain([&self.master]) {
            if !strip.gain_db.is_finite() || !strip.pan.is_finite() {
                return Err("a strip has a gain or pan that is not a number".into());
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
            if track
                .kit
                .as_deref()
                .is_some_and(|k| k.is_empty() || k.contains(['/', '\\']))
            {
                return Err(format!(
                    "track {} has a kit id that is not a plain name",
                    track.id
                ));
            }
            for clip in &track.pattern_clips {
                if clip.length == 0 {
                    return Err(format!("pattern clip {} has no length", clip.id));
                }
                if self.pattern(clip.pattern).is_none() {
                    return Err(format!(
                        "pattern clip {} refers to pattern {}, which does not exist",
                        clip.id, clip.pattern
                    ));
                }
                if !track.is_drums() {
                    return Err(format!(
                        "track {} is an audio track but has pattern clips",
                        track.id
                    ));
                }
            }
        }
        for pattern in &self.patterns {
            pattern.validate()?;
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
        let mut p = project();
        assert_eq!(p.fresh_id(), 4);
        assert_eq!(Project::new("Empty", 48_000.0).fresh_id(), 1);
        p.patterns.push(Pattern::new(PatternId(7), "Beat"));
        assert_eq!(p.fresh_id(), 8, "patterns share the id space");
    }

    #[test]
    fn patterns_are_validated() {
        let mut p = project();
        let mut pattern = Pattern::new(PatternId(5), "Beat");
        assert_eq!(pattern.step_ticks(), 240);
        assert_eq!(pattern.ticks(), 3840);
        pattern.rows.push(PatternRow {
            pad: "kick".into(),
            steps: vec![0.0; 16],
        });
        pattern.rows[0].steps[4] = 0.8;
        assert_eq!(pattern.velocity("kick", 4), 0.8);
        assert_eq!(pattern.velocity("kick", 5), 0.0);
        assert_eq!(pattern.velocity("snare", 4), 0.0);
        p.patterns.push(pattern);
        assert_eq!(p.validate(), Ok(()));
        let check = |edit: fn(&mut Pattern), expected: &str| {
            let mut p = p.clone();
            edit(&mut p.patterns[0]);
            let err = p.validate().unwrap_err();
            assert!(err.contains(expected), "{err}");
        };
        check(|q| q.name.clear(), "no name");
        check(|q| q.steps_per_beat = 5, "steps per beat");
        check(|q| q.length = 0, "1 to 128");
        check(|q| q.swing = 1.5, "swing");
        check(
            |q| q.rows[0].steps.pop().map(drop).unwrap_or(()),
            "15 steps, the pattern 16",
        );
        check(|q| q.rows[0].steps[0] = 2.0, "outside 0 to 1");
        check(|q| q.rows.push(q.rows[0].clone()), "two rows");
        p.tracks.push(Track::drums(TrackId(9), "Drums", "acoustic"));
        assert!(p.tracks[1].is_drums());
        assert_eq!(p.validate(), Ok(()));
        p.tracks[1].pattern_clips.push(PatternClip {
            id: PatternClipId(10),
            pattern: PatternId(5),
            start: 0,
            offset: 0,
            length: 3840,
        });
        assert_eq!(p.validate(), Ok(()));
        assert!(p.pattern_in_use(PatternId(5)));
        assert!(!p.pattern_in_use(PatternId(99)));
        assert_eq!(p.pattern_clip(PatternClipId(10)).unwrap().0.id, TrackId(9));
        assert_eq!(p.fresh_id(), 11);
        let mut bad = p.clone();
        bad.tracks[1].pattern_clips[0].length = 0;
        assert!(bad.validate().unwrap_err().contains("no length"));
        let mut bad = p.clone();
        bad.tracks[1].pattern_clips[0].pattern = PatternId(99);
        assert!(bad.validate().unwrap_err().contains("does not exist"));
        let mut bad = p.clone();
        bad.tracks[1].kit = None;
        assert!(bad.validate().unwrap_err().contains("audio track"));
        p.tracks[1].kit = Some("../x".into());
        assert!(p.validate().unwrap_err().contains("kit id"));
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
            serde_json::from_str(r#"{"version":4,"name":"x","sampleRate":44100}"#).unwrap();
        assert_eq!(minimal.tempo, Tempo::default());
        assert!(minimal.tracks.is_empty());
        assert!(minimal.patterns.is_empty());
        assert_eq!(minimal.master, Strip::default());
        assert!(!json["tracks"][0].as_object().unwrap().contains_key("kit"));
        assert!(!json["tracks"][0]
            .as_object()
            .unwrap()
            .contains_key("patternClips"));
    }
}
