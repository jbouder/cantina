//! The engine's side of jam mode: opening and closing a jam, its setup,
//! and the transport's play, stop, sections and tempo. The schedule
//! itself is [`crate::jam`].

use std::collections::BTreeMap;

use cantina_project::{Jam, Meter, Section, DRUM_GAIN_DB, JAM_TEMPO};

use super::*;
use crate::jam::{self, JamMark, Schedule, DRUM_TRACK, FIRST_PEDAL, INPUT_TRACK, JAM_IDS};

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
}

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
                let first = previous.map_or(FIRST_PEDAL, |p| p.first_pedal + RIG_IDS);
                let first = if first + RIG_IDS >= AUDITION_IDS {
                    FIRST_PEDAL
                } else {
                    first
                };
                let (chain, states, missing) = match setup.rig.clone() {
                    None => (Vec::new(), Vec::new(), Vec::new()),
                    Some(id) => match self.rig_path(&id) {
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
                self.plugin_states
                    .retain(|id, _| !(JAM_IDS..AUDITION_IDS).contains(&id.0));
                for (id, spec, state) in states {
                    self.plugin_states.insert(id, (spec, state));
                }
                (chain, missing, first)
            }
        };
        let mut session = JamSession {
            first_pedal,
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

    /// Hands the audio thread the jam's tempo map, the drums' score and
    /// where an ending stops, after the schedule changed.
    fn jam_send(&mut self) -> Result<()> {
        let scores = self.jam_scores();
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
        if waits {
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
        } else {
            self.sync()?;
        }
        self.jam_send()?;
        self.jam_remember();
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
