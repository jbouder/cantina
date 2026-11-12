//! The engine's side of the drum designer: a copy of a drum track's part
//! heard in place of the project while the designer is open, played on
//! loop at the project's tempo. Nothing here touches the project; the
//! part goes back onto the track when it is placed.

use cantina_project::{DrumPart, Fill, PatternClip, PatternClipId, PatternId};

use super::*;

/// The first id the designer's track, patterns and clips take: apart
/// from the project's, a jam's and an audition's.
const DESIGN_IDS: u64 = 1 << 60;
/// The track the part plays on.
pub(super) const DESIGN_TRACK: TrackId = TrackId(DESIGN_IDS);
/// Section `i`'s pattern is `DESIGN_PATTERNS + i`.
const DESIGN_PATTERNS: u64 = DESIGN_IDS + 16;
/// Step `i` of the order is clip `DESIGN_CLIPS + i`.
const DESIGN_CLIPS: u64 = DESIGN_IDS + (1 << 32);

/// What the designer plays round and round. Mirrors `DesignPlays` in
/// the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DesignPlays {
    /// One section, by name.
    Section {
        /// The section's name.
        name: String,
    },
    /// The whole order.
    Order,
}

/// The drum designer while it is open.
pub(super) struct DrumDesign {
    /// The drum track it was opened from.
    track: TrackId,
    /// The part as the designer has it now.
    part: DrumPart,
    /// The part as it was opened, or last placed.
    base: DrumPart,
    plays: DesignPlays,
    /// Where the project's playhead was, put back on close.
    project_position: u64,
}

/// The open drum designer, for the status. Mirrors `DrumDesignStatus`
/// in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrumDesignStatus {
    /// The drum track it was opened from.
    pub track: u64,
    /// The part as the designer has it.
    pub part: DrumPart,
    /// What plays on loop.
    pub plays: DesignPlays,
    /// The part differs from the track's: closing loses it.
    pub changed: bool,
    /// Ticks in a bar, at the project's time signature.
    pub bar_ticks: u64,
    /// Ticks the loop lasts.
    pub loop_ticks: u64,
}

/// Ticks in a bar of `tempo`'s time signature.
fn bar_ticks(tempo: &Tempo) -> u64 {
    u64::from(cantina_project::PPQ) * 4 / u64::from(tempo.denominator.max(1))
        * u64::from(tempo.numerator.max(1))
}

impl DrumDesign {
    /// The clips the designer plays: one for a section, long enough for
    /// its fill to come round, or one per step of the order.
    fn clips(&self, bar: u64) -> Vec<PatternClip> {
        let fill_of = |section: usize| {
            let s = &self.part.sections[section];
            s.fill.as_ref().and_then(|f| {
                self.part.section_index(&f.section).map(|i| Fill {
                    pattern: PatternId(DESIGN_PATTERNS + i as u64),
                    every: f.every,
                    at_end: f.at_end,
                })
            })
        };
        match &self.plays {
            DesignPlays::Section { name } => {
                let Some(i) = self.part.section_index(name) else {
                    return Vec::new();
                };
                let ticks = self.part.sections[i].pattern.ticks().max(1);
                let fill = fill_of(i);
                let every = fill.as_ref().map_or(0, |f| u64::from(f.every)) * bar;
                let length = every.max(ticks).div_ceil(ticks) * ticks;
                vec![PatternClip {
                    id: PatternClipId(DESIGN_CLIPS),
                    pattern: PatternId(DESIGN_PATTERNS + i as u64),
                    start: 0,
                    offset: 0,
                    length,
                    fill,
                }]
            }
            DesignPlays::Order => self
                .part
                .layout()
                .into_iter()
                .map(|span| PatternClip {
                    id: PatternClipId(DESIGN_CLIPS + span.step as u64),
                    pattern: PatternId(DESIGN_PATTERNS + span.section as u64),
                    start: span.start,
                    offset: 0,
                    length: span.length,
                    fill: fill_of(span.section),
                })
                .collect(),
        }
    }

    /// Ticks the loop lasts.
    fn loop_ticks(&self, bar: u64) -> u64 {
        self.clips(bar).iter().map(|c| c.end()).max().unwrap_or(0)
    }

    /// What plays: the named section if the part still has it, else the
    /// order if it has one, else the first section.
    fn settle_plays(&mut self) {
        let keep = match &self.plays {
            DesignPlays::Section { name } => self.part.section(name).is_some(),
            DesignPlays::Order => self.part.order_ticks() > 0,
        };
        if !keep {
            self.plays = DesignPlays::Section {
                name: self.part.sections[0].name.clone(),
            };
        }
    }
}

impl Engine {
    /// The project the engine hears while the designer is open: the part
    /// on a drum track of its own, at the project's tempo, through the
    /// track's mixer strip.
    pub(super) fn design_project(&self) -> Option<Project> {
        let design = self.drum_design.as_ref()?;
        let mut project = Project::new("Drum designer", self.project.sample_rate);
        project.tempo = self.project.tempo.clone();
        let source = self.project.track(design.track);
        let mut track = Track::drums(DESIGN_TRACK, "Drums", design.part.kit.clone());
        if let Some(t) = source {
            track.strip.gain_db = t.strip.gain_db;
            track.strip.pan = t.strip.pan;
            track.pads = t.pads.clone();
        }
        track.pattern_clips = design.clips(bar_ticks(&self.project.tempo));
        project.patterns = design
            .part
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| Pattern {
                id: PatternId(DESIGN_PATTERNS + i as u64),
                ..s.pattern.clone()
            })
            .collect();
        project.tracks = vec![track];
        Some(project)
    }

    /// The loop the designer plays, in samples.
    pub(super) fn design_loop(&self) -> Option<LoopRange> {
        let design = self.drum_design.as_ref()?;
        let ticks = design.loop_ticks(bar_ticks(&self.project.tempo));
        let sample_rate = self
            .live
            .as_ref()
            .map_or(self.project.sample_rate, |l| l.stream.info().sample_rate);
        let end = tempo_map(&self.project.tempo).ticks_to_samples(ticks as f64, sample_rate);
        (end > 0).then_some(LoopRange { start: 0, end })
    }

    /// The open designer, for the status.
    pub(super) fn design_status(&self) -> Option<DrumDesignStatus> {
        let design = self.drum_design.as_ref()?;
        let bar = bar_ticks(&self.project.tempo);
        Some(DrumDesignStatus {
            track: design.track.0,
            part: design.part.clone(),
            plays: design.plays.clone(),
            changed: design.part != design.base,
            bar_ticks: bar,
            loop_ticks: design.loop_ticks(bar),
        })
    }

    /// Opens the drum designer on a drum track: a copy of its part is
    /// heard in place of the project, which stays as it is. Refused while
    /// recording or jamming.
    pub fn drum_design_open(&mut self, track: TrackId) -> Result<EngineStatus> {
        if self.drum_design.as_ref().is_some_and(|d| d.track == track) {
            return Ok(self.publish());
        }
        if self.drum_design.is_some() {
            bail!("the drum designer is open on another track");
        }
        if self.jam.is_some() {
            bail!("leave the jam before designing drums");
        }
        if !self.recording.is_empty() {
            bail!("stop recording before designing drums");
        }
        let t = self
            .project
            .track(track)
            .with_context(|| format!("no track {track}"))?;
        if !t.is_drums() {
            bail!("{} is not a drum track", t.name);
        }
        let mut part = DrumPart::from_track(&self.project, t);
        if self.kits.entry(&part.kit).is_none() {
            match self.kits.entries().first() {
                Some(k) => part.kit = k.id.clone(),
                None => bail!("no drum kit is installed"),
            }
        }
        let position = match &mut self.live {
            Some(live) => {
                live.transport().stop();
                live.transport().position()
            }
            None => 0,
        };
        self.audition = None;
        let plays = if part.order_ticks() > 0 {
            DesignPlays::Order
        } else {
            DesignPlays::Section {
                name: part.sections[0].name.clone(),
            }
        };
        self.drum_design = Some(DrumDesign {
            track,
            base: part.clone(),
            part,
            plays,
            project_position: position,
        });
        if let Some(live) = &mut self.live {
            live.transport().locate(0);
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// Closes the drum designer: the project is heard again, with its
    /// playhead where it was. A part not placed is dropped.
    pub fn drum_design_close(&mut self) -> Result<EngineStatus> {
        let Some(design) = self.drum_design.take() else {
            return Ok(self.publish());
        };
        if let Some(live) = &mut self.live {
            live.transport().stop();
            live.transport().locate(design.project_position);
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// Replaces the designer's part with `part`, heard at once.
    pub fn drum_design_set(&mut self, part: DrumPart) -> Result<EngineStatus> {
        part.validate().map_err(|e| anyhow!("{e}"))?;
        if self.kits.entry(&part.kit).is_none() {
            bail!("there is no kit {}", part.kit);
        }
        let Some(design) = &mut self.drum_design else {
            bail!("open the drum designer first");
        };
        let same_kit = design.part.kit == part.kit;
        let before = design.loop_ticks(bar_ticks(&self.project.tempo));
        design.part = part;
        design.settle_plays();
        let after = design.loop_ticks(bar_ticks(&self.project.tempo));
        if same_kit && before == after {
            self.design_refresh()?;
        } else {
            self.sync()?;
        }
        Ok(self.publish())
    }

    /// Plays a section, or the whole order, round and round.
    pub fn drum_design_plays(&mut self, plays: DesignPlays) -> Result<EngineStatus> {
        let Some(design) = &mut self.drum_design else {
            bail!("open the drum designer first");
        };
        match &plays {
            DesignPlays::Section { name } if design.part.section(name).is_none() => {
                bail!("the part has no section {name}")
            }
            DesignPlays::Order if design.part.order_ticks() == 0 => {
                bail!("the order is empty")
            }
            _ => {}
        }
        if design.plays == plays {
            return Ok(self.publish());
        }
        design.plays = plays;
        if let Some(live) = &mut self.live {
            live.transport().locate(0);
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// Plays the part from the top of its loop.
    pub fn drum_design_play(&mut self) -> Result<EngineStatus> {
        if self.drum_design.is_none() {
            bail!("open the drum designer first");
        }
        let Some(live) = &mut self.live else {
            bail!("start the engine before playing");
        };
        live.transport().locate(0);
        live.transport().play();
        Ok(self.publish())
    }

    /// Stops the part and goes back to the top.
    pub fn drum_design_stop(&mut self) -> Result<EngineStatus> {
        if let Some(live) = &mut self.live {
            live.transport().stop();
            live.transport().locate(0);
        }
        Ok(self.publish())
    }

    /// Sends the designer's drum track its score after an edit that left
    /// the kit and the loop as they were.
    fn design_refresh(&mut self) -> Result<()> {
        let heard = self.design_project();
        let (Some(live), Some(heard)) = (&mut self.live, heard) else {
            return Ok(());
        };
        live.refresh_scores(&heard, &mut self.kits, &None, &self.drum_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cantina_project::{OrderStep, PartSection, SectionFill};

    fn part() -> DrumPart {
        let section = |name: &str, length: u16| PartSection {
            name: name.into(),
            kind: cantina_project::kind_of(name),
            pattern: Pattern {
                length,
                ..Pattern::new(PatternId(1), name)
            },
            fill: None,
        };
        DrumPart {
            kit: "rock".into(),
            sections: vec![section("Verse", 16), section("Fill", 16)],
            order: vec![OrderStep {
                section: "Verse".into(),
                times: 8,
            }],
        }
    }

    fn design(part: DrumPart, plays: DesignPlays) -> DrumDesign {
        DrumDesign {
            track: TrackId(1),
            base: part.clone(),
            part,
            plays,
            project_position: 0,
        }
    }

    #[test]
    fn a_section_loops_long_enough_for_its_fill() {
        let mut p = part();
        let plays = DesignPlays::Section {
            name: "Verse".into(),
        };
        assert_eq!(design(p.clone(), plays.clone()).loop_ticks(3840), 3840);
        p.sections[0].fill = Some(SectionFill {
            section: "Fill".into(),
            every: 4,
            at_end: false,
        });
        let d = design(p, plays);
        let clips = d.clips(3840);
        assert_eq!(clips[0].length, 4 * 3840);
        assert_eq!(
            clips[0].fill.as_ref().map(|f| f.pattern),
            Some(PatternId(DESIGN_PATTERNS + 1))
        );
    }

    #[test]
    fn the_order_is_a_clip_per_step() {
        let mut p = part();
        p.order.push(OrderStep {
            section: "Fill".into(),
            times: 1,
        });
        let d = design(p, DesignPlays::Order);
        let clips = d.clips(3840);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].start, 8 * 3840);
        assert_eq!(d.loop_ticks(3840), 9 * 3840);
    }

    #[test]
    fn a_section_that_goes_falls_back() {
        let mut d = design(
            part(),
            DesignPlays::Section {
                name: "Fill".into(),
            },
        );
        d.part.sections.pop();
        d.settle_plays();
        assert_eq!(
            d.plays,
            DesignPlays::Section {
                name: "Verse".into()
            }
        );
    }
}
