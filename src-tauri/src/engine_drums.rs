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
    /// The whole order, starting from one of its steps the first time
    /// round.
    Order {
        /// The step it starts from.
        #[serde(default)]
        from: usize,
    },
}

/// A clip the designer plays, and what it is of.
#[derive(Debug, Clone, PartialEq)]
struct Placed {
    clip: PatternClip,
    /// The section, by index.
    section: usize,
    /// The step of the order, for a clip of the order.
    step: Option<usize>,
}

/// What plays where, for the view's playhead. Mirrors `PlaySpan` in the
/// UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaySpan {
    /// Tick it starts on.
    pub start: u64,
    /// Ticks it lasts.
    pub length: u64,
    /// The section, by name.
    pub section: String,
    /// The step of the order, for the whole song.
    pub step: Option<usize>,
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
    /// What played before `start`: what was rolling when a change was
    /// asked for, cut at the bar line the change lands on.
    lead: Vec<Placed>,
    /// The tick what `plays` names starts on.
    start: u64,
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
    /// The tick the loop starts on.
    pub loop_start: u64,
    /// The tick the loop wraps at.
    pub loop_end: u64,
    /// What plays where, up to the loop's end.
    pub spans: Vec<PlaySpan>,
}

/// Ticks in a bar of `tempo`'s time signature.
fn bar_ticks(tempo: &Tempo) -> u64 {
    u64::from(cantina_project::PPQ) * 4 / u64::from(tempo.denominator.max(1))
        * u64::from(tempo.numerator.max(1))
}

impl DrumDesign {
    /// The fill section `section` plays, as a pattern clip's fill.
    fn fill_of(&self, section: usize) -> Option<Fill> {
        let s = &self.part.sections[section];
        s.fill.as_ref().and_then(|f| {
            self.part.section_index(&f.section).map(|i| Fill {
                pattern: PatternId(DESIGN_PATTERNS + i as u64),
                every: f.every,
                at_end: f.at_end,
            })
        })
    }

    /// What `plays` names, from tick 0, and the loop within it: one clip
    /// for a section, long enough for its fill to come round; for the
    /// order, the steps from `from` to the end and then the whole order,
    /// which loops.
    fn material(&self, bar: u64) -> (Vec<Placed>, u64, u64) {
        let clip = |section: usize, step: Option<usize>, start: u64, length: u64| Placed {
            clip: PatternClip {
                id: PatternClipId(0),
                pattern: PatternId(DESIGN_PATTERNS + section as u64),
                start,
                offset: 0,
                length,
                fill: self.fill_of(section),
            },
            section,
            step,
        };
        match &self.plays {
            DesignPlays::Section { name } => {
                let Some(i) = self.part.section_index(name) else {
                    return (Vec::new(), 0, 0);
                };
                let ticks = self.part.sections[i].pattern.ticks().max(1);
                let every = self.fill_of(i).map_or(0, |f| u64::from(f.every)) * bar;
                let length = every.max(ticks).div_ceil(ticks) * ticks;
                (vec![clip(i, None, 0, length)], 0, length)
            }
            DesignPlays::Order { from } => {
                let spans = self.part.layout();
                let total = spans.last().map_or(0, |s| s.start + s.length);
                let skip = spans.get(*from).map_or(0, |s| s.start);
                let rest = total - skip;
                let mut out: Vec<Placed> = spans
                    .iter()
                    .filter(|s| s.start >= skip && skip > 0)
                    .map(|s| clip(s.section, Some(s.step), s.start - skip, s.length))
                    .collect();
                let at = if skip > 0 { rest } else { 0 };
                out.extend(
                    spans
                        .iter()
                        .map(|s| clip(s.section, Some(s.step), at + s.start, s.length)),
                );
                (out, at, total)
            }
        }
    }

    /// Every clip the designer plays, with fresh ids: the lead, then what
    /// `plays` names from `start`.
    fn placed(&self, bar: u64) -> Vec<Placed> {
        let (material, _, _) = self.material(bar);
        let mut out = self.lead.clone();
        out.extend(material.into_iter().map(|mut p| {
            p.clip.start += self.start;
            p
        }));
        for (i, p) in out.iter_mut().enumerate() {
            p.clip.id = PatternClipId(DESIGN_CLIPS + i as u64);
        }
        out
    }

    /// The clips the designer plays.
    fn clips(&self, bar: u64) -> Vec<PatternClip> {
        self.placed(bar).into_iter().map(|p| p.clip).collect()
    }

    /// The loop, in ticks: where it starts and where it wraps.
    fn loop_ticks(&self, bar: u64) -> (u64, u64) {
        let (_, at, length) = self.material(bar);
        (self.start + at, self.start + at + length)
    }

    /// Plays `plays` from the bar line `at`: what was playing carries on
    /// up to it.
    fn switch(&mut self, plays: DesignPlays, at: u64, bar: u64) {
        self.lead = self
            .placed(bar)
            .into_iter()
            .filter(|p| p.clip.start < at)
            .map(|mut p| {
                p.clip.length = p.clip.length.min(at - p.clip.start);
                p
            })
            .collect();
        self.start = at;
        self.plays = plays;
    }

    /// Back to the top: nothing before what `plays` names.
    fn rewind(&mut self) {
        self.lead.clear();
        self.start = 0;
    }

    /// What plays: the named section if the part still has it, else the
    /// order if it has one, else the first section.
    fn settle_plays(&mut self) {
        let keep = match &self.plays {
            DesignPlays::Section { name } => self.part.section(name).is_some(),
            DesignPlays::Order { from } => *from < self.part.layout().len(),
        };
        if !keep {
            self.plays = if self.part.order_ticks() > 0 {
                DesignPlays::Order { from: 0 }
            } else {
                DesignPlays::Section {
                    name: self.part.sections[0].name.clone(),
                }
            };
        }
        let sections = self.part.sections.len();
        self.lead.retain(|p| p.section < sections);
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

    fn design_sample_rate(&self) -> f64 {
        self.live
            .as_ref()
            .map_or(self.project.sample_rate, |l| l.stream.info().sample_rate)
    }

    /// The loop the designer plays, in samples.
    pub(super) fn design_loop(&self) -> Option<LoopRange> {
        let design = self.drum_design.as_ref()?;
        let (start, end) = design.loop_ticks(bar_ticks(&self.project.tempo));
        let map = tempo_map(&self.project.tempo);
        let rate = self.design_sample_rate();
        let range = LoopRange {
            start: map.ticks_to_samples(start as f64, rate),
            end: map.ticks_to_samples(end as f64, rate),
        };
        (range.end > range.start).then_some(range)
    }

    /// The open designer, for the status.
    pub(super) fn design_status(&self) -> Option<DrumDesignStatus> {
        let design = self.drum_design.as_ref()?;
        let bar = bar_ticks(&self.project.tempo);
        let (loop_start, loop_end) = design.loop_ticks(bar);
        Some(DrumDesignStatus {
            track: design.track.0,
            part: design.part.clone(),
            plays: design.plays.clone(),
            changed: design.part != design.base,
            bar_ticks: bar,
            loop_start,
            loop_end,
            spans: design
                .placed(bar)
                .into_iter()
                .filter_map(|p| {
                    Some(PlaySpan {
                        start: p.clip.start,
                        length: p.clip.length,
                        section: design.part.sections.get(p.section)?.name.clone(),
                        step: p.step,
                    })
                })
                .collect(),
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
            DesignPlays::Order { from: 0 }
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
            lead: Vec::new(),
            start: 0,
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
        let bar = bar_ticks(&self.project.tempo);
        let Some(design) = &mut self.drum_design else {
            bail!("open the drum designer first");
        };
        let same_kit = design.part.kit == part.kit;
        let before = design.loop_ticks(bar);
        design.part = part;
        design.settle_plays();
        let after = design.loop_ticks(bar);
        if same_kit && before == after {
            self.design_refresh()?;
        } else {
            self.sync()?;
        }
        Ok(self.publish())
    }

    /// Plays a section, or the order from one of its steps, round and
    /// round. While it rolls the change lands on the next bar line far
    /// enough ahead to reach the audio thread first; stopped, it is where
    /// play starts.
    pub fn drum_design_plays(&mut self, plays: DesignPlays) -> Result<EngineStatus> {
        let bar = bar_ticks(&self.project.tempo);
        let next = self.design_next_bar();
        let Some(design) = &mut self.drum_design else {
            bail!("open the drum designer first");
        };
        match &plays {
            DesignPlays::Section { name } if design.part.section(name).is_none() => {
                bail!("the part has no section {name}")
            }
            DesignPlays::Order { .. } if design.part.order_ticks() == 0 => {
                bail!("the order is empty")
            }
            DesignPlays::Order { from } if *from >= design.part.layout().len() => {
                bail!("the order has no step {}", from + 1)
            }
            _ => {}
        }
        if design.plays == plays {
            return Ok(self.publish());
        }
        match next {
            Some(at) => design.switch(plays, at, bar),
            None => {
                design.plays = plays;
                design.rewind();
                if let Some(live) = &mut self.live {
                    live.transport().locate(0);
                }
            }
        }
        self.sync()?;
        Ok(self.publish())
    }

    /// The bar line a change asked for now lands on while the part rolls:
    /// the next one far enough ahead to reach the audio thread first, and
    /// no later than the loop's end. None when stopped.
    fn design_next_bar(&self) -> Option<u64> {
        let live = self.live.as_ref()?;
        let design = self.drum_design.as_ref()?;
        let transport = live.controller.transport();
        if !transport.is_playing() {
            return None;
        }
        let info = live.stream.info();
        let bar = bar_ticks(&self.project.tempo);
        let map = tempo_map(&self.project.tempo);
        let margin = 2 * u64::from(info.buffer_frames) + (info.sample_rate * 0.02) as u64;
        let tick = map.samples_to_ticks(transport.position() + margin, info.sample_rate);
        let next = (tick / bar as f64).ceil() as u64 * bar;
        let (_, end) = design.loop_ticks(bar);
        Some(next.min(end))
    }

    /// Plays the part from the top of what it plays.
    pub fn drum_design_play(&mut self) -> Result<EngineStatus> {
        let Some(design) = &mut self.drum_design else {
            bail!("open the drum designer first");
        };
        design.rewind();
        self.sync()?;
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
        if let Some(design) = &mut self.drum_design {
            design.rewind();
        }
        self.sync()?;
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
            lead: Vec::new(),
            start: 0,
            project_position: 0,
        }
    }

    #[test]
    fn a_section_loops_long_enough_for_its_fill() {
        let mut p = part();
        let plays = DesignPlays::Section {
            name: "Verse".into(),
        };
        assert_eq!(design(p.clone(), plays.clone()).loop_ticks(3840), (0, 3840));
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
        let d = design(p, DesignPlays::Order { from: 0 });
        let clips = d.clips(3840);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].start, 8 * 3840);
        assert_eq!(d.loop_ticks(3840), (0, 9 * 3840));
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
        // The song, which the part has; with no order, the first section.
        assert_eq!(d.plays, DesignPlays::Order { from: 0 });
        d.part.order.clear();
        d.plays = DesignPlays::Section {
            name: "Gone".into(),
        };
        d.settle_plays();
        assert_eq!(
            d.plays,
            DesignPlays::Section {
                name: "Verse".into()
            }
        );
    }

    #[test]
    fn a_change_while_rolling_lands_on_the_bar_line() {
        let mut d = design(
            part(),
            DesignPlays::Section {
                name: "Verse".into(),
            },
        );
        // Looping the one-bar verse, the fill is asked for: it plays from
        // the loop's end, after the verse.
        d.switch(
            DesignPlays::Section {
                name: "Fill".into(),
            },
            3840,
            3840,
        );
        let clips = d.clips(3840);
        assert_eq!(clips.len(), 2);
        assert_eq!((clips[0].start, clips[0].length), (0, 3840));
        assert_eq!(clips[0].pattern, PatternId(DESIGN_PATTERNS));
        assert_eq!((clips[1].start, clips[1].length), (3840, 3840));
        assert_eq!(clips[1].pattern, PatternId(DESIGN_PATTERNS + 1));
        assert_eq!(d.loop_ticks(3840), (3840, 2 * 3840));
        // A second change cuts the fill short at its bar line.
        d.switch(DesignPlays::Order { from: 0 }, 2 * 3840, 3840);
        let clips = d.clips(3840);
        assert_eq!(clips[1].length, 3840);
        assert_eq!(clips[2].start, 2 * 3840);
        assert_eq!(d.loop_ticks(3840), (2 * 3840, 10 * 3840));
        assert_ne!(clips[0].id, clips[1].id);
        // Stopping goes back to the top with nothing before it.
        d.rewind();
        assert_eq!(d.clips(3840).len(), 1);
        assert_eq!(d.loop_ticks(3840), (0, 8 * 3840));
    }

    #[test]
    fn the_order_can_start_from_any_step() {
        let mut p = part();
        p.order = vec![
            OrderStep {
                section: "Verse".into(),
                times: 2,
            },
            OrderStep {
                section: "Fill".into(),
                times: 1,
            },
            OrderStep {
                section: "Verse".into(),
                times: 1,
            },
        ];
        let d = design(p, DesignPlays::Order { from: 1 });
        let clips = d.clips(3840);
        // The fill and the last verse, then the whole order on loop.
        let starts: Vec<_> = clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, [0, 3840, 2 * 3840, 4 * 3840, 5 * 3840]);
        assert_eq!(d.loop_ticks(3840), (2 * 3840, 6 * 3840));
    }
}
