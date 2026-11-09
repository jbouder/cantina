//! The engine's side of jam mode: opening and closing a jam, its setup,
//! and the transport's play, stop, sections and tempo. The schedule
//! itself is [`crate::jam`].

use std::collections::BTreeMap;

use cantina_project::{
    Backing, Bar, Instrument, Jam, Meter, Rhythm, Section, DRUM_GAIN_DB, JAM_TEMPO,
};

use super::*;
use crate::jam::{
    self, JamMark, Schedule, BACKING_TRACK, DRUM_TRACK, FIRST_BACKING_PEDAL, FIRST_PEDAL,
    INPUT_TRACK, JAM_IDS,
};

/// A jam that is open: the engine hears it in place of the project.
pub(super) struct JamSession {
    setup: Jam,
    /// Each section's pattern: the jam's own, else the groove's.
    patterns: BTreeMap<Section, Pattern>,
    schedule: Schedule,
    /// The rig's pedals, ids from `first_pedal`.
    chain: Vec<Pedal>,
    /// Each rig gets ids no rig before it had, so a pedal of the same kind
    /// in the next rig is built anew with its own settings.
    first_pedal: u64,
    /// The rig's pedals that are not installed, and a rig that is not
    /// found at all.
    missing: Vec<String>,
    /// Where the project's playhead was, put back on close.
    project_position: u64,
    backing: JamBacking,
    /// What Keep that wrote last, until it goes into a project.
    kept: Option<KeptTake>,
}

/// A take Keep that wrote, and what the jam was doing under it.
struct KeptTake {
    /// The dry input and the signal through the rig, in `Jams/Takes`.
    dry: PathBuf,
    wet: PathBuf,
    frames: u64,
    sample_rate: f64,
    /// The jam's timeline sample of its first frame, when the jam rolled
    /// under it.
    start: Option<i64>,
    map: TempoMap,
    segments: Vec<(u64, u64, Section)>,
    patterns: BTreeMap<Section, Pattern>,
    setup: Jam,
    chain: Vec<Pedal>,
}

/// Where Keep that's take goes. Mirrors `KeepInto` in the UI.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "into", rename_all = "camelCase")]
pub enum KeepInto {
    /// The open project.
    Open,
    /// A new project of this name, which is opened.
    New { name: String },
    /// Nowhere: it stays in `Jams/Takes`.
    Takes,
}

/// The last take kept, as the UI shows it. Mirrors `JamKept` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptStatus {
    pub seconds: f64,
    pub dry: String,
    pub processed: String,
}

/// What the backing plays, built from the setup's.
#[derive(Clone)]
struct JamBacking {
    play: BackingPlay,
    /// Its rig's pedals, or the project track's chain, ids from
    /// `first_pedal`.
    chain: Vec<Pedal>,
    first_pedal: u64,
    /// What was left out: pedals not installed, files not found.
    missing: Vec<String>,
}

#[derive(Clone)]
enum BackingPlay {
    None,
    /// A progression's bars, for the voice.
    Voice {
        bars: Vec<Bar>,
        instrument: Instrument,
        rhythm: Rhythm,
    },
    /// Audio clips, repeated to cover a long jam, at this tempo.
    Clips {
        clips: Vec<Clip>,
        bpm: f64,
    },
}

impl JamBacking {
    fn none(first_pedal: u64) -> Self {
        Self {
            play: BackingPlay::None,
            chain: Vec::new(),
            first_pedal,
            missing: Vec::new(),
        }
    }

    /// The tempo the jam takes from audio.
    fn bpm(&self) -> Option<f64> {
        match &self.play {
            BackingPlay::Clips { bpm, .. } => Some(*bpm),
            _ => None,
        }
    }
}

/// A backing setup with its level left out: changing only the level
/// keeps what the backing is built from.
fn source_of(backing: &Backing) -> Backing {
    let mut b = backing.clone();
    match &mut b {
        Backing::None => {}
        Backing::Progression { level_db, .. }
        | Backing::Audio { level_db, .. }
        | Backing::Track { level_db, .. } => *level_db = 0.0,
    }
    b
}

/// How long an audio backing's repeats cover, and the most clips they
/// may take.
const AUDIO_HORIZON_SECONDS: f64 = 6.0 * 3600.0;
const MAX_BACKING_CLIPS: usize = 2048;

/// Clips placed again every `period` samples from the start of the jam,
/// those reaching past a period cut at it, for as many periods as the
/// horizon and the limit on clips allow.
fn repeated(clips: &[Clip], period: f64, sample_rate: f64) -> Vec<Clip> {
    let inside: Vec<&Clip> = clips
        .iter()
        .filter(|c| (c.start as f64) < period && c.length > 0)
        .collect();
    if inside.is_empty() || period < 1.0 {
        return Vec::new();
    }
    let passes = ((AUDIO_HORIZON_SECONDS * sample_rate / period).ceil() as usize)
        .min(MAX_BACKING_CLIPS / inside.len())
        .max(1);
    let mut out = Vec::with_capacity(passes * inside.len());
    for k in 0..passes {
        let base = (k as f64 * period).round() as u64;
        let next = ((k + 1) as f64 * period).round() as u64;
        for c in &inside {
            let start = base + c.start;
            out.push(Clip {
                id: ClipId(JAM_IDS + out.len() as u64),
                start,
                length: c.length.min(next.saturating_sub(start)),
                ..(*c).clone()
            });
        }
    }
    out
}

/// Samples in a whole number of bars nearest `frames`, at least one.
fn whole_bars(frames: u64, bpm: f64, meter: Meter, sample_rate: f64) -> f64 {
    let beats = f64::from(meter.numerator) * 4.0 / f64::from(meter.denominator.max(1));
    let bar = 60.0 / bpm * beats * sample_rate;
    (frames as f64 / bar).round().max(1.0) * bar
}

/// The open jam as the UI sees it. Mirrors `JamStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamStatus {
    /// What the jam is: groove, tempo, key, rig. Remembered for next time.
    pub setup: Jam,
    /// The sections the drums have a pattern for, in order.
    pub sections: Vec<Section>,
    /// Each section's length in bars.
    pub bars: BTreeMap<Section, u64>,
    /// Where the section or tempo changes, and is going to.
    pub marks: Vec<JamMark>,
    /// The sample an ending stops the jam on, when one is coming.
    pub end: Option<u64>,
    /// Ticks in a bar.
    pub bar_ticks: u64,
    /// The tempo last asked for.
    pub tempo: f64,
    /// Rig pedals left out because they are not installed.
    pub missing: Vec<String>,
    /// What the backing left out: pedals, files.
    pub backing_missing: Vec<String>,
    /// The tempo is the backing's, which the jam follows.
    pub tempo_locked: bool,
    /// The stock chord progressions, by genre.
    pub presets: Vec<PresetInfo>,
    /// Minutes of the input the jam keeps for Keep that.
    pub capture_minutes: f32,
    /// The last take kept, waiting to go into a project.
    pub kept: Option<KeptStatus>,
}

/// A project's audio track a jam can play. Mirrors `JamTrack` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamTrackInfo {
    pub name: String,
    /// The project's tempo, which the jam takes.
    pub bpm: f64,
    /// From the start to the end of its last clip.
    pub seconds: f64,
}

/// A stock chord progression. Mirrors `JamPreset` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub genre: &'static str,
    pub name: &'static str,
    pub chords: &'static str,
}

/// How long Keep that waits for the writer thread.
const KEEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Ids set aside for each rig's pedals.
const RIG_IDS: u64 = 256;

/// Room left before a bar line for a change to reach the audio thread:
/// two buffers and 20 ms besides.
fn margin(sample_rate: f64, buffer_frames: u32) -> u64 {
    2 * u64::from(buffer_frames) + (sample_rate * 0.02) as u64
}

impl JamSession {
    fn sections(&self) -> Vec<Section> {
        jam::SECTIONS
            .into_iter()
            .filter(|s| self.patterns.contains_key(s))
            .collect()
    }

    /// Ticks the section's pattern lasts.
    fn length(&self, section: Section) -> u64 {
        self.patterns.get(&section).map_or(0, |p| p.ticks())
    }

    /// A schedule from the top at the jam's tempo, starting with `first`
    /// or the jam's own first section.
    fn restart(&mut self, first: Option<Section>) {
        let sections = self.sections();
        let lengths: BTreeMap<Section, u64> =
            sections.iter().map(|s| (*s, self.length(*s))).collect();
        let first = first
            .filter(|s| sections.contains(s))
            .unwrap_or_else(|| jam::first(&sections));
        self.schedule = Schedule::new(
            self.setup.bar_ticks(),
            first,
            self.setup.tempo,
            |s| lengths.get(&s).copied().unwrap_or(0),
            jam::home(&sections),
        );
    }
}

impl Engine {
    /// The jam the engine would open now: the last one, or Rock on the
    /// first kit through the classic rock rig.
    fn jam_setup(&self) -> Jam {
        if let Some(jam) = &self.settings.jam {
            return jam.clone();
        }
        let kit = self
            .kits
            .entries()
            .first()
            .map_or_else(|| "studio".to_string(), |k| k.id.clone());
        let mut jam = Jam::new("Jam", 100.0, kit);
        let genre = self
            .grooves
            .iter()
            .find(|g| g.groove.genre == "Rock")
            .or(self.grooves.first())
            .map(|g| g.groove.genre.clone());
        jam.drums.groove = genre;
        if self.rig_path("classic-rock").is_some() {
            jam.rig = Some("classic-rock".into());
        }
        jam
    }

    /// The rig file a jam names: the user's first, then the stock one.
    fn rig_path(&self, id: &str) -> Option<String> {
        let wanted = cantina_project::rig_file_name(id);
        self.library_status
            .rigs
            .iter()
            .chain(&self.stock_rigs)
            .find(|r| {
                Path::new(&r.path)
                    .file_name()
                    .is_some_and(|n| n == wanted.as_str())
            })
            .map(|r| r.path.clone())
    }

    /// A rig's pedals with ids from `first`, and what was left out. Their
    /// plugins' states replace those of any jam pedals in `range`.
    fn jam_rig(
        &mut self,
        rig: Option<&str>,
        first: u64,
        range: std::ops::Range<u64>,
    ) -> Result<(Vec<Pedal>, Vec<String>)> {
        let (chain, states, missing) = match rig {
            None => (Vec::new(), Vec::new(), Vec::new()),
            Some(id) => match self.rig_path(id) {
                None => (
                    Vec::new(),
                    Vec::new(),
                    vec![format!("the rig {id:?} is not installed")],
                ),
                Some(path) => {
                    let rig = crate::rigs::read(Path::new(&path))?;
                    self.rig_chain(&rig, first)?
                }
            },
        };
        self.plugin_states.retain(|id, _| !range.contains(&id.0));
        for (id, spec, state) in states {
            self.plugin_states.insert(id, (spec, state));
        }
        Ok((chain, missing))
    }

    /// The ids a rig takes after `previous`, from `first` again when they
    /// would reach `end`.
    fn next_ids(previous: Option<u64>, first: u64, end: u64) -> u64 {
        match previous {
            Some(p) if p + 2 * RIG_IDS < end => p + RIG_IDS,
            _ => first,
        }
    }

    /// Builds the backing a setup asks for. Audio is checked and, at
    /// another sample rate, converted to a copy in the cache.
    fn jam_backing(&mut self, setup: &Jam, previous: Option<&JamBacking>) -> Result<JamBacking> {
        let first = Self::next_ids(
            previous.map(|b| b.first_pedal),
            FIRST_BACKING_PEDAL,
            AUDITION_IDS,
        );
        let range = FIRST_BACKING_PEDAL..AUDITION_IDS;
        let sample_rate = self.project.sample_rate;
        match &setup.backing {
            Backing::None => Ok(JamBacking::none(first)),
            Backing::Progression {
                chords,
                instrument,
                rhythm,
                rig,
                ..
            } => {
                let tonic = cantina_project::pitch_class(&setup.key.tonic).unwrap_or(0);
                let bars = cantina_project::parse_progression(chords, tonic)
                    .map_err(|p| anyhow!("{}", p.join("; ")))?;
                let (chain, missing) = self.jam_rig(rig.as_deref(), first, range)?;
                Ok(JamBacking {
                    play: BackingPlay::Voice {
                        bars,
                        instrument: *instrument,
                        rhythm: *rhythm,
                    },
                    chain,
                    first_pedal: first,
                    missing,
                })
            }
            Backing::Audio { file, bpm, .. } => {
                let (path, frames) = self.jam_audio(Path::new(file))?;
                let period = whole_bars(frames, *bpm, setup.time_signature, sample_rate);
                let clip = Clip {
                    id: ClipId(0),
                    name: "Backing".into(),
                    file: path.display().to_string(),
                    start: 0,
                    offset: 0,
                    length: frames,
                    tab: None,
                };
                self.plugin_states.retain(|id, _| !range.contains(&id.0));
                Ok(JamBacking {
                    play: BackingPlay::Clips {
                        clips: repeated(&[clip], period, sample_rate),
                        bpm: *bpm,
                    },
                    chain: Vec::new(),
                    first_pedal: first,
                    missing: Vec::new(),
                })
            }
            Backing::Track { project, track, .. } => {
                let (source, dir) = if *project == self.project.name {
                    (self.project.clone(), self.project_dir.clone())
                } else {
                    self.projects.open(project)?
                };
                let t = source
                    .tracks
                    .iter()
                    .find(|t| &t.name == track && !t.is_drums())
                    .with_context(|| format!("{project} has no audio track {track:?}"))?;
                let audio_dir = cantina_project::audio_dir(&dir);
                let mut clips = Vec::new();
                let mut missing = Vec::new();
                for c in &t.clips {
                    match self.jam_audio(&audio_dir.join(&c.file)) {
                        Ok((path, _)) => {
                            // A copy at another rate is as long, in time.
                            let ratio = sample_rate / source.sample_rate;
                            let at = |f: u64| (f as f64 * ratio).round() as u64;
                            clips.push(Clip {
                                file: path.display().to_string(),
                                start: at(c.start),
                                offset: at(c.offset),
                                length: at(c.length),
                                tab: None,
                                ..c.clone()
                            });
                        }
                        Err(e) => missing.push(format!("{}: {e:#}", c.name)),
                    }
                }
                let end = clips.iter().map(|c| c.start + c.length).max().unwrap_or(0);
                if end == 0 {
                    bail!("{track} in {project} has no audio to play");
                }
                let bpm = source.tempo.bpm;
                let period = whole_bars(end, bpm, setup.time_signature, sample_rate);
                // The track's chain, with ids of the jam's; designed pedals
                // only when this Cantina has them, plugins with their state.
                let states = if dir == self.project_dir {
                    self.plugin_states.clone()
                } else {
                    read_plugin_states(&dir)
                };
                self.plugin_states.retain(|id, _| !range.contains(&id.0));
                let mut chain = Vec::new();
                for (i, p) in t.chain.iter().enumerate() {
                    if let Some(id) = p.spec.strip_prefix("pedal:") {
                        if !self.designs.contains_key(id) && self.library.entry(id).is_none() {
                            missing
                                .push(format!("the designed pedal {id:?} is not in your library"));
                            continue;
                        }
                    }
                    let id = PedalId(first + i as u64);
                    if let Some(state) = states.get(&p.id) {
                        self.plugin_states.insert(id, state.clone());
                    }
                    chain.push(Pedal { id, ..p.clone() });
                }
                Ok(JamBacking {
                    play: BackingPlay::Clips {
                        clips: repeated(&clips, period, sample_rate),
                        bpm,
                    },
                    chain,
                    first_pedal: first,
                    missing,
                })
            }
        }
    }

    /// A WAV file at the engine's sample rate, and its length: the file
    /// itself, or a copy converted once into the cache.
    fn jam_audio(&self, path: &Path) -> Result<(PathBuf, u64)> {
        let info = wav::info(path)
            .with_context(|| format!("{} is not a WAV file Cantina can read", path.display()))?;
        let rate = self.project.sample_rate;
        if f64::from(info.sample_rate) == rate && usize::from(info.channels) <= CHANNELS {
            return Ok((path.to_path_buf(), info.frames));
        }
        let cache = self
            .app
            .path()
            .app_cache_dir()
            .context("there is no cache folder")?
            .join("jam-audio");
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&path, &mut h);
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        std::hash::Hash::hash(&modified, &mut h);
        let name = format!("{:016x}-{}.wav", std::hash::Hasher::finish(&h), rate as u32);
        let copy = cache.join(name);
        if let Ok(have) = wav::info(&copy) {
            return Ok((copy, have.frames));
        }
        let (info, channels) = wav::read_all(path)?;
        let mut channels = fit_audio(channels, f64::from(info.sample_rate), rate);
        channels.truncate(CHANNELS);
        std::fs::create_dir_all(&cache).with_context(|| format!("creating {}", cache.display()))?;
        wav::write(&copy, rate, &channels)?;
        let frames = channels.first().map_or(0, Vec::len) as u64;
        Ok((copy, frames))
    }

    /// Builds what a setup plays: each section's pattern, and the rig's
    /// chain. A groove sets the meter. Refuses a setup that does not
    /// validate or gives the drums nothing to play. The rig of `previous`
    /// is kept as it is when the setup names the same one.
    fn jam_build(
        &mut self,
        mut setup: Jam,
        project_position: u64,
        previous: Option<&JamSession>,
    ) -> Result<JamSession> {
        setup.validate().map_err(|p| anyhow!("{}", p.join("; ")))?;
        if self.kits.entry(&setup.drums.kit).is_none() {
            match self.kits.entries().first() {
                Some(k) => setup.drums.kit = k.id.clone(),
                None => bail!("no drum kit is installed"),
            }
        }
        let mut patterns = BTreeMap::new();
        if let Some(genre) = &setup.drums.groove {
            let grooves: Vec<_> = self
                .grooves
                .iter()
                .filter(|g| &g.groove.genre == genre)
                .collect();
            if grooves.is_empty() && setup.drums.patterns.is_empty() {
                bail!("there is no {genre} groove");
            }
            if let Some(g) = grooves
                .iter()
                .find(|g| g.groove.section == Section::Verse)
                .or(grooves.first())
            {
                setup.time_signature = g.groove.time_signature;
            }
            for g in grooves {
                let text = std::fs::read_to_string(&g.path)
                    .with_context(|| format!("reading {}", g.path))?;
                let (pattern, _) = cantina_project::groove_from_json(&text)
                    .map_err(|e| anyhow!("{}: {e}", g.path))?;
                patterns.insert(g.groove.section, pattern);
            }
        }
        for (section, pattern) in &setup.drums.patterns {
            patterns.insert(*section, pattern.clone());
        }
        let bar = setup.bar_ticks();
        patterns.retain(|_, p| p.ticks() > 0 && p.ticks() % bar == 0);
        if patterns.is_empty() {
            bail!(
                "the drums have nothing to play in {}/{}",
                setup.time_signature.numerator,
                setup.time_signature.denominator
            );
        }
        let (chain, missing, first_pedal) = match previous {
            Some(p) if p.setup.rig == setup.rig => {
                (p.chain.clone(), p.missing.clone(), p.first_pedal)
            }
            _ => {
                let first = Self::next_ids(
                    previous.map(|p| p.first_pedal),
                    FIRST_PEDAL,
                    FIRST_BACKING_PEDAL,
                );
                let (chain, missing) = self.jam_rig(
                    setup.rig.as_deref(),
                    first,
                    FIRST_PEDAL..FIRST_BACKING_PEDAL,
                )?;
                (chain, missing, first)
            }
        };
        let same_backing = previous.filter(|p| {
            source_of(&p.setup.backing) == source_of(&setup.backing)
                && p.setup.key == setup.key
                && p.setup.time_signature == setup.time_signature
        });
        let backing = match same_backing {
            Some(p) => p.backing.clone(),
            None => self.jam_backing(&setup, previous.map(|p| &p.backing))?,
        };
        // Audio plays at its own tempo, and the jam at it.
        if let Some(bpm) = backing.bpm() {
            setup.tempo = (bpm.clamp(JAM_TEMPO.0, JAM_TEMPO.1) * 10.0).round() / 10.0;
        }
        let mut session = JamSession {
            kept: None,
            first_pedal,
            backing,
            schedule: Schedule::new(bar, Section::Verse, setup.tempo, |_| bar, Section::Verse),
            setup,
            patterns,
            chain,
            missing,
            project_position,
        };
        session.restart(None);
        Ok(session)
    }

    /// The project the engine hears while a jam is open: the input
    /// through the rig, and the drums.
    pub(super) fn jam_project(&self) -> Option<Project> {
        let jam = self.jam.as_ref()?;
        let mut project = Project::new("Jam", self.project.sample_rate);
        let Meter {
            numerator,
            denominator,
        } = jam.setup.time_signature;
        project.tempo = Tempo {
            bpm: jam.schedule.tempo_at(0),
            numerator,
            denominator,
        };
        let mut input = Track::new(INPUT_TRACK, "Input");
        input.armed = true;
        input.chain = jam.chain.clone();
        let mut drums = Track::drums(DRUM_TRACK, "Drums", jam.setup.drums.kit.clone());
        drums.strip.gain_db = DRUM_GAIN_DB + jam.setup.drums.level_db;
        project.tracks = vec![input, drums];
        let level = match &jam.setup.backing {
            Backing::None => None,
            Backing::Progression { level_db, .. }
            | Backing::Audio { level_db, .. }
            | Backing::Track { level_db, .. } => Some(*level_db),
        };
        if let Some(level_db) = level {
            let mut backing = Track::new(BACKING_TRACK, "Backing");
            backing.chain = jam.backing.chain.clone();
            backing.strip.gain_db = level_db;
            if let BackingPlay::Clips { clips, .. } = &jam.backing.play {
                backing.clips = clips.clone();
            }
            project.tracks.push(backing);
        }
        Some(project)
    }

    /// The tempo map a jam plays to.
    pub(super) fn jam_tempo_map(&self) -> Option<TempoMap> {
        let jam = self.jam.as_ref()?;
        Some(jam.schedule.tempo_map(jam.setup.time_signature))
    }

    /// What the jam's drums play, by track, in place of pattern clips.
    pub(super) fn jam_scores(&mut self) -> HashMap<TrackId, Score> {
        let Some(jam) = &self.jam else {
            return HashMap::new();
        };
        let kit = self.kits.get(&jam.setup.drums.kit);
        let data = jam
            .patterns
            .iter()
            .map(|(s, p)| (*s, pattern_data(p, &kit, None)))
            .collect();
        HashMap::from([(DRUM_TRACK, jam.schedule.placements(&data))])
    }

    /// What the backing's voice plays: the progression round and round
    /// until an ending, which rings its first chord.
    pub(super) fn jam_voices(&self) -> HashMap<TrackId, (TabScore, TabLoop)> {
        let Some(jam) = &self.jam else {
            return HashMap::new();
        };
        let BackingPlay::Voice {
            bars,
            instrument,
            rhythm,
        } = &jam.backing.play
        else {
            return HashMap::new();
        };
        let end = jam.schedule.end();
        let until = jam.schedule.ending_start().unwrap_or(u64::MAX);
        let looped =
            jam::progression_loop(bars, *instrument, *rhythm, jam.setup.time_signature, until);
        let score = match (jam.schedule.ending_start(), end) {
            (Some(at), Some(end)) => jam::ending_chord(bars, *instrument, at, end - at),
            _ => TabScore::new(),
        };
        HashMap::from([(BACKING_TRACK, (score, looped))])
    }

    /// Hands the audio thread the jam's tempo map, the drums' score and
    /// where an ending stops, after the schedule changed.
    fn jam_send(&mut self) -> Result<()> {
        let scores = self.jam_scores();
        let voices = self.jam_voices();
        let (Some(live), Some(jam)) = (&mut self.live, &self.jam) else {
            return Ok(());
        };
        let map = jam.schedule.tempo_map(jam.setup.time_signature);
        let sample_rate = live.stream.info().sample_rate;
        let stop = jam
            .schedule
            .end()
            .map(|t| map.ticks_to_samples(t as f64, sample_rate));
        if live.transport().tempo_map() != &map {
            live.set_tempo_map(map);
        }
        live.transport().stop_at(stop);
        for (id, score) in scores {
            if let Source::Drums { sequencer, .. } = &mut live.track_mut(id)?.source {
                if sequencer.score() != &score && !sequencer.set_score(score) {
                    bail!("the audio thread is not taking the drums");
                }
            }
        }
        for (id, (score, looped)) in voices {
            if let Source::Audio { voice, .. } = &mut live.track_mut(id)?.source {
                if (voice.score() != &score && !voice.set_score(score))
                    || (voice.looped() != &looped && !voice.set_loop(looped))
                {
                    bail!("the audio thread is not taking the backing");
                }
            }
        }
        Ok(())
    }

    /// The open jam, for the status.
    pub(super) fn jam_status(&self) -> Option<JamStatus> {
        let jam = self.jam.as_ref()?;
        let sample_rate = self
            .live
            .as_ref()
            .map_or(self.project.sample_rate, |l| l.stream.info().sample_rate);
        let map = jam.schedule.tempo_map(jam.setup.time_signature);
        let bar = jam.setup.bar_ticks();
        Some(JamStatus {
            setup: jam.setup.clone(),
            sections: jam.sections(),
            bars: jam
                .patterns
                .iter()
                .map(|(s, p)| (*s, p.ticks() / bar))
                .collect(),
            marks: jam.schedule.marks(&map, sample_rate),
            end: jam
                .schedule
                .end()
                .map(|t| map.ticks_to_samples(t as f64, sample_rate)),
            bar_ticks: bar,
            tempo: jam.schedule.tempo(),
            missing: jam.missing.clone(),
            backing_missing: jam.backing.missing.clone(),
            tempo_locked: jam.backing.bpm().is_some(),
            capture_minutes: self.settings.capture_minutes,
            kept: jam.kept.as_ref().map(|k| KeptStatus {
                seconds: k.frames as f64 / k.sample_rate,
                dry: k.dry.display().to_string(),
                processed: k.wet.display().to_string(),
            }),
            presets: cantina_project::PRESETS
                .iter()
                .map(|p| PresetInfo {
                    genre: p.genre,
                    name: p.name,
                    chords: p.chords,
                })
                .collect(),
        })
    }

    /// Whether a change waits for a bar line: while the jam rolls, and on
    /// the count-in's last beat. Before that, the timeline has not
    /// moved and the change can be the jam's start.
    fn jam_changes_wait(&self) -> bool {
        self.live.as_ref().is_some_and(|l| {
            let t = l.controller.transport();
            t.is_playing() || t.clock().count_in() >= cantina_engine::transport::COUNT_IN_BEATS
        })
    }

    /// Opens jam mode: the engine hears the last jam, or a first one, in
    /// place of the project, which stays as it is. Refused while
    /// recording.
    pub fn jam_open(&mut self) -> Result<EngineStatus> {
        if self.jam.is_some() {
            return Ok(self.publish());
        }
        if !self.recording.is_empty() {
            bail!("stop recording before jamming");
        }
        let position = match &mut self.live {
            Some(live) => {
                live.transport().stop();
                live.transport().position()
            }
            None => 0,
        };
        if self.rig_audition.take().is_some() {
            self.plugin_states.retain(|id, _| id.0 < AUDITION_IDS);
        }
        self.audition = None;
        self.capture_plugin_states();
        let setup = self.jam_setup();
        let session = match self.jam_build(setup, position, None) {
            Ok(s) => s,
            // A remembered jam that no longer builds gives way to a first one.
            Err(_) if self.settings.jam.is_some() => {
                self.settings.jam = None;
                let setup = self.jam_setup();
                self.jam_build(setup, position, None)?
            }
            Err(e) => return Err(e),
        };
        self.jam = Some(session);
        if let Some(live) = &mut self.live {
            live.transport().set_rate(1.0);
            live.transport().locate(0);
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// Leaves jam mode: the project is heard again, with its playhead
    /// where it was.
    pub fn jam_close(&mut self) -> Result<EngineStatus> {
        let Some(jam) = self.jam.take() else {
            return Ok(self.publish());
        };
        self.plugin_states
            .retain(|id, _| !(JAM_IDS..AUDITION_IDS).contains(&id.0));
        if let Some(live) = &mut self.live {
            live.transport().stop();
            live.transport().stop_at(None);
            live.transport().set_rate(self.practice.speed);
            live.set_tempo_map(tempo_map(&self.project.tempo));
            live.transport().locate(jam.project_position);
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// Changes what the jam is: the groove, kit, tempo, key or rig. The
    /// setup is remembered for next time. While it plays, a new tempo
    /// lands on the next bar; anything else is heard at once.
    pub fn jam_set(&mut self, setup: Jam) -> Result<EngineStatus> {
        let waits = self.jam_changes_wait();
        let Some(old) = self.jam.take() else {
            bail!("open a jam first");
        };
        let mut session = match self.jam_build(setup, old.project_position, Some(&old)) {
            Ok(s) => s,
            Err(e) => {
                self.jam = Some(old);
                return Err(e);
            }
        };
        let rig_changed = session.setup.rig != old.setup.rig;
        let mut old = old;
        session.kept = old.kept.take();
        let backing_changed = source_of(&session.setup.backing) != source_of(&old.setup.backing);
        // Audio sits on the timeline from the jam's first sample at its own
        // tempo: a jam rolling at another cannot take it, or leave it, in
        // time. It stops, to start again in step.
        let regrid =
            backing_changed && (session.backing.bpm().is_some() || old.backing.bpm().is_some());
        if waits && regrid {
            if let Some(live) = &mut self.live {
                live.transport().stop();
                live.transport().stop_at(None);
                live.transport().locate(0);
            }
        }
        if waits && !regrid {
            // The music goes on: the same sections, a new tempo on the
            // next bar.
            session.schedule = old.schedule;
            let tempo = session.setup.tempo;
            self.jam = Some(session);
            self.jam_tempo_at_next_bar(tempo);
        } else {
            session.restart(Some(old.schedule.section_at(0)));
            self.jam = Some(session);
        }
        if rig_changed {
            self.with_track_faded(INPUT_TRACK, |e| e.sync())?;
        } else if backing_changed && old.setup.backing != Backing::None {
            self.with_track_faded(BACKING_TRACK, |e| e.sync())?;
        } else {
            self.sync()?;
        }
        self.jam_send()?;
        self.jam_remember();
        Ok(self.publish())
    }

    /// The audio tracks of a project a jam can play as its backing.
    pub fn jam_tracks(&self, project: &str) -> Result<Vec<JamTrackInfo>> {
        let source = if project == self.project.name {
            self.project.clone()
        } else {
            self.projects.open(project)?.0
        };
        Ok(source
            .tracks
            .iter()
            .filter(|t| !t.is_drums() && !t.clips.is_empty())
            .map(|t| JamTrackInfo {
                name: t.name.clone(),
                bpm: source.tempo.bpm,
                seconds: t.clips.iter().map(|c| c.end()).max().unwrap_or(0) as f64
                    / source.sample_rate,
            })
            .collect())
    }

    /// Sets how many minutes of input the jam keeps: 1, 2, 5 or 10. New
    /// rings start empty.
    pub fn jam_capture_set(&mut self, minutes: f32) -> Result<EngineStatus> {
        if !crate::settings::CAPTURE_MINUTES.contains(&minutes) {
            bail!("the capture keeps 1, 2, 5 or 10 minutes, not {minutes}");
        }
        self.settings.capture_minutes = minutes;
        self.persist();
        self.sync()?;
        Ok(self.publish())
    }

    /// Keep that: writes the last minutes of the input, dry and through
    /// the rig, to `Jams/Takes`, on the input's recorder's writer thread,
    /// and holds on to what the jam was doing under them for
    /// [`Engine::jam_keep_place`].
    pub fn jam_keep(&mut self) -> Result<EngineStatus> {
        let Some(jam) = &self.jam else {
            bail!("open a jam first");
        };
        let live = self.live.as_mut().context("start the engine first")?;
        let capture = live
            .capture
            .as_ref()
            .context("nothing has been captured yet")?;
        let sample_rate = live.stream.info().sample_rate;
        let (dry, wet) = (Arc::clone(&capture.dry), Arc::clone(&capture.wet));
        let to = dry.written().min(wet.written());
        // Once the rings have come round, a second's room from the frames
        // being overwritten.
        let held = dry.oldest().max(wet.oldest());
        let from = if held == 0 {
            0
        } else {
            (held + sample_rate as u64).min(to)
        };
        if to.saturating_sub(from) < (sample_rate / 2.0) as u64 {
            bail!("there is less than half a second to keep yet");
        }
        let start = dry
            .anchor()
            .map(|a| a.position as i64 + from as i64 - a.frame as i64);
        let dir = self.projects.root().join("Jams").join("Takes");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let (file, _) = take_name(&dir, "Jam");
        let dry_path = dir.join(&file);
        let wet_path = dir.join(file.replace(".wav", " processed.wav"));
        let (done, result) = mpsc::channel();
        let job = cantina_engine::KeepJob {
            rings: vec![(dry, dry_path.clone()), (wet, wet_path.clone())],
            from,
            to,
            sample_rate,
            done,
        };
        let Source::Audio { recorder, .. } = &live.track(INPUT_TRACK)?.source else {
            bail!("the jam's input has no recorder");
        };
        recorder.keep(job)?;
        let kept = result
            .recv_timeout(KEEP_TIMEOUT)
            .map_err(|_| anyhow!("writing the take took too long"))?
            .map_err(|e| anyhow!("{e}"))?;
        let segments = jam.schedule.segments();
        let take = KeptTake {
            dry: dry_path,
            wet: wet_path,
            frames: kept.frames,
            sample_rate,
            start,
            map: jam.schedule.tempo_map(jam.setup.time_signature),
            segments,
            patterns: jam.patterns.clone(),
            setup: jam.setup.clone(),
            chain: jam.chain.clone(),
        };
        if let Some(jam) = &mut self.jam {
            jam.kept = Some(take);
        }
        Ok(self.publish())
    }

    /// Puts the take Keep that wrote into the open project, or a new one,
    /// as one undoable edit: an audio track with the jam's rig and the dry
    /// take, a muted one with the take through the rig, and a drum track
    /// with the sections that played under it, all on the project's bars.
    /// An empty project takes the jam's tempo; one with music in it keeps
    /// its own and the take goes after its last bar.
    pub fn jam_keep_place(&mut self, into: KeepInto) -> Result<EngineStatus> {
        let take = self
            .jam
            .as_mut()
            .and_then(|j| j.kept.take())
            .context("keep a take first")?;
        let name = match into {
            KeepInto::Takes => return Ok(self.publish()),
            KeepInto::Open => None,
            KeepInto::New { name } => Some(name),
        };
        if let Some(name) = name {
            if let Err(e) = self.project_new(&name) {
                if let Some(jam) = &mut self.jam {
                    jam.kept = Some(take);
                }
                return Err(e);
            }
        }
        let audio_dir = cantina_project::audio_dir(&self.project_dir);
        cantina_project::create_layout(&self.project_dir)?;
        let rate = self.project.sample_rate;
        let mut files = Vec::new();
        for (path, stem) in [(&take.dry, "Jam take"), (&take.wet, "Jam take processed")] {
            let (file, _) = take_name(&audio_dir, stem);
            if take.sample_rate == rate {
                std::fs::copy(path, audio_dir.join(&file))
                    .with_context(|| format!("copying {}", path.display()))?;
            } else {
                let (_, channels) = wav::read_all(path)?;
                wav::write(
                    &audio_dir.join(&file),
                    rate,
                    &fit_audio(channels, take.sample_rate, rate),
                )?;
            }
            files.push(file);
        }
        let frames = (take.frames as f64 * rate / take.sample_rate).round() as u64;
        let project = &self.project;
        let empty = project
            .tracks
            .iter()
            .all(|t| t.clips.is_empty() && t.pattern_clips.is_empty());
        let meter = take.setup.time_signature;
        let tempo = if empty {
            let end = take
                .start
                .map_or(0, |s| (s + take.frames as i64).max(0) as u64);
            Tempo {
                bpm: take
                    .map
                    .tempo_at(take.map.samples_to_ticks(end, take.sample_rate) as u64),
                numerator: meter.numerator,
                denominator: meter.denominator,
            }
        } else {
            project.tempo.clone()
        };
        let bar = u64::from(cantina_project::PPQ) * 4 / u64::from(tempo.denominator.max(1))
            * u64::from(tempo.numerator.max(1));
        let at = if empty {
            0
        } else {
            let per_tick = rate * 60.0 / (tempo.bpm * f64::from(cantina_project::PPQ));
            let end = project
                .tracks
                .iter()
                .flat_map(|t| {
                    t.clips
                        .iter()
                        .map(|c| (c.end() as f64 / per_tick) as u64)
                        .chain(t.pattern_clips.iter().map(|c| c.end()))
                })
                .max()
                .unwrap_or(0);
            end.div_ceil(bar) * bar
        };
        let jam_bar = take.setup.bar_ticks();
        let placed = crate::jam::place_take(
            take.start,
            take.frames,
            &take.map,
            take.sample_rate,
            &take.segments,
            |s| take.patterns.get(&s).map_or(jam_bar, |p| p.ticks()),
            jam_bar,
            at,
            tempo.bpm,
        );
        let scale = |f: u64| (f as f64 * rate / take.sample_rate).round() as u64;
        let mut next = self.project.fresh_id();
        let mut id = || {
            next += 1;
            next - 1
        };
        let mut commands = Vec::new();
        if empty && tempo != self.project.tempo {
            commands.push(Command::SetTempo {
                tempo: tempo.clone(),
            });
        }
        // The patterns the drums play, renamed on a clash.
        let mut taken: Vec<String> = self
            .project
            .patterns
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut pattern_ids: BTreeMap<Section, PatternId> = BTreeMap::new();
        for (section, ..) in &placed.drums {
            if pattern_ids.contains_key(section) {
                continue;
            }
            let Some(p) = take.patterns.get(section) else {
                continue;
            };
            let names: Vec<&str> = taken.iter().map(String::as_str).collect();
            let pattern = cantina_project::imported(p.clone(), PatternId(id()), &names);
            taken.push(pattern.name.clone());
            pattern_ids.insert(*section, pattern.id);
            commands.push(Command::InsertPattern {
                index: usize::MAX,
                pattern,
            });
        }
        let index = self.project.tracks.len();
        let clip = |id: u64, name: &str, file: String| Clip {
            id: ClipId(id),
            name: name.into(),
            file,
            start: scale(placed.start),
            offset: scale(placed.offset),
            length: frames.saturating_sub(scale(placed.offset)),
            tab: None,
        };
        let take_id = TrackId(id());
        let mut dry = Track::new(take_id, "Jam take");
        dry.chain = take
            .chain
            .iter()
            .map(|p| Pedal {
                id: PedalId(id()),
                ..p.clone()
            })
            .collect();
        for (new, old) in dry.chain.iter().zip(&take.chain) {
            if let Some(state) = self.plugin_states.get(&old.id).cloned() {
                self.plugin_states.insert(new.id, state);
            }
        }
        dry.clips.push(clip(id(), "Jam take", files[0].clone()));
        let mut wet = Track::new(TrackId(id()), "Jam take, processed");
        wet.strip.mute = true;
        wet.clips
            .push(clip(id(), "Jam take, processed", files[1].clone()));
        let mut tracks = vec![dry, wet];
        if !pattern_ids.is_empty() {
            let mut drums = Track::drums(TrackId(id()), "Jam drums", take.setup.drums.kit.clone());
            drums.strip.gain_db += take.setup.drums.level_db;
            for (section, start, offset, length) in &placed.drums {
                if let Some(pattern) = pattern_ids.get(section) {
                    drums.pattern_clips.push(cantina_project::PatternClip {
                        id: cantina_project::PatternClipId(id()),
                        pattern: *pattern,
                        start: *start,
                        offset: *offset,
                        length: *length,
                        fill: None,
                    });
                }
            }
            tracks.push(drums);
        }
        for (i, track) in tracks.into_iter().enumerate() {
            commands.push(Command::InsertTrack {
                index: index + i,
                track,
            });
        }
        let seconds = take.frames as f64 / take.sample_rate;
        self.apply(Command::Batch { commands })?;
        self.notice = Some(format!(
            "Kept {}:{:04.1} of jamming in {}",
            (seconds / 60.0) as u64,
            seconds % 60.0,
            self.project.name
        ));
        self.notice_seq = self.notice_seq.wrapping_add(1);
        Ok(self.publish())
    }

    fn jam_remember(&mut self) {
        if let Some(jam) = &self.jam {
            let mut setup = jam.setup.clone();
            setup.tempo = jam.schedule.tempo();
            self.settings.jam = Some(setup);
            self.persist();
        }
    }

    /// Plays the jam from the top, after a count-in when count-in is on.
    pub fn jam_play(&mut self) -> Result<EngineStatus> {
        let Some(jam) = &mut self.jam else {
            bail!("open a jam first");
        };
        jam.setup.tempo = jam.schedule.tempo();
        // A section picked while stopped is where it starts.
        jam.restart(Some(jam.schedule.section_at(0)));
        let Some(live) = &mut self.live else {
            bail!("start the engine before playing");
        };
        live.transport().stop();
        live.transport().locate(0);
        self.jam_send()?;
        let count_in = self.settings.count_in;
        if let Some(live) = &mut self.live {
            let sample_rate = live.stream.info().sample_rate;
            if count_in {
                live.transport().count_in(sample_rate);
            } else {
                live.transport().play();
            }
        }
        Ok(self.publish())
    }

    /// Stops the jam and goes back to the top.
    pub fn jam_stop(&mut self) -> Result<EngineStatus> {
        if let Some(live) = &mut self.live {
            live.transport().stop();
            live.transport().stop_at(None);
            live.transport().locate(0);
        }
        if let Some(jam) = &mut self.jam {
            jam.restart(None);
        }
        self.jam_send()?;
        Ok(self.publish())
    }

    /// Plays `section` from the next bar; stopped, starts the jam with
    /// it next time.
    pub fn jam_section(&mut self, section: Section) -> Result<EngineStatus> {
        let waits = self.jam_changes_wait();
        let next = self.jam_next_bar();
        let Some(jam) = &mut self.jam else {
            bail!("open a jam first");
        };
        let sections = jam.sections();
        if !sections.contains(&section) {
            bail!(
                "the drums have no {}",
                cantina_project::section_name(section)
            );
        }
        let length = jam.length(section);
        let home = jam::home(&sections);
        match next {
            Some(at) if waits => jam.schedule.switch(at, section, length, home),
            _ => {
                jam.setup.tempo = jam.schedule.tempo();
                jam.restart(Some(section));
            }
        }
        self.jam_send()?;
        Ok(self.publish())
    }

    /// Sets the tempo: rolling, from the next bar; stopped, from the top.
    pub fn jam_tempo(&mut self, bpm: f64) -> Result<EngineStatus> {
        if self.jam.is_none() {
            bail!("open a jam first");
        }
        let (slow, fast) = JAM_TEMPO;
        if !bpm.is_finite() {
            bail!("{bpm} is not a tempo");
        }
        if self.jam.as_ref().is_some_and(|j| j.backing.bpm().is_some()) {
            bail!("the backing plays at its own tempo; the jam follows it");
        }
        let bpm = (bpm.clamp(slow, fast) * 10.0).round() / 10.0;
        if self.jam_changes_wait() {
            self.jam_tempo_at_next_bar(bpm);
        } else if let Some(jam) = &mut self.jam {
            jam.setup.tempo = bpm;
            jam.restart(Some(jam.schedule.section_at(0)));
        }
        self.jam_send()?;
        self.jam_remember();
        Ok(self.publish())
    }

    fn jam_tempo_at_next_bar(&mut self, bpm: f64) {
        let next = self.jam_next_bar();
        if let (Some(jam), Some(at)) = (&mut self.jam, next) {
            jam.schedule.set_tempo(at, bpm);
        }
    }

    /// The next bar line a change sent now can land on.
    fn jam_next_bar(&self) -> Option<u64> {
        let live = self.live.as_ref()?;
        let jam = self.jam.as_ref()?;
        let info = live.stream.info();
        let map = jam.schedule.tempo_map(jam.setup.time_signature);
        Some(jam.schedule.next_bar(
            &map,
            live.controller.transport().position(),
            info.sample_rate,
            margin(info.sample_rate, info.buffer_frames),
        ))
    }
}
