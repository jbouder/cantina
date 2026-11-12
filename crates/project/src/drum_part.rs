//! A drum part: what the drum designer builds. Sections (intro, verse,
//! chorus and any you name), each with a pattern of its own length, and
//! an order that arranges them into a song.
//!
//! The designer works on a part away from the project: it is made from a
//! drum track's pattern clips when the designer opens, heard in place of
//! the project while it is open, and becomes clips on the track again
//! when it is placed.

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::model::{
    Fill, Pattern, PatternClip, PatternClipId, PatternId, PatternRow, Project, StepMark, Track,
    TrackId,
};
use crate::pattern_file::Section;

/// Times a step of the order can repeat its section, and bars apart a
/// fill can come, at most.
pub const MAX_REPEAT: u16 = 64;

/// A drum part: a kit, its sections and the order they play in.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrumPart {
    /// The kit it plays, by id.
    pub kit: String,
    /// The sections, in the order the designer lists them.
    pub sections: Vec<PartSection>,
    /// The song: sections by name, each repeated a number of times.
    #[serde(default)]
    pub order: Vec<OrderStep>,
}

/// A section of a part: a name, the kind of section it is when it is
/// one of the usual ones, and its pattern.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartSection {
    /// Unique within the part; the order refers to the section by it.
    pub name: String,
    /// Intro, verse, chorus, bridge, fill or ending; none for a section
    /// of your own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Section>,
    /// What it plays, looped for as long as it lasts.
    pub pattern: Pattern,
    /// Another section's first bar played as a fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<SectionFill>,
    /// Variations B, C and D of the pattern (which is A), at most three,
    /// each with its steps and resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<Pattern>,
    /// How the variations take turns, pass by pass.
    #[serde(default, skip_serializing_if = "Vary::is_alternate")]
    pub vary: Vary,
}

/// How a section's variations take turns, one pass of the pattern each.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Vary {
    /// A between each of the others: A B A C A D.
    #[default]
    Alternate,
    /// One after another: A B C D.
    Chain,
}

impl Vary {
    fn is_alternate(&self) -> bool {
        *self == Vary::Alternate
    }
}

/// Variations a section can have besides its pattern: B, C and D.
pub const MAX_VARIATIONS: usize = 3;

impl PartSection {
    /// Which pattern each pass plays, by index (0 for A, 1 for B…), round
    /// and round.
    pub fn passes(&self) -> Vec<usize> {
        let n = self.variations.len().min(MAX_VARIATIONS);
        match self.vary {
            _ if n == 0 => vec![0],
            Vary::Chain => (0..=n).collect(),
            Vary::Alternate => (1..=n).flat_map(|v| [0, v]).collect(),
        }
    }

    /// What the section plays: its pattern, or with variations, one
    /// pattern holding a whole round of its passes, so a fill counts its
    /// bars across them. The feel is A's.
    pub fn played(&self) -> Pattern {
        let passes = self.passes();
        if passes.len() < 2 {
            return self.pattern.clone();
        }
        let a = &self.pattern;
        let length = a.length as usize;
        let of = |i: usize| if i == 0 { a } else { &self.variations[i - 1] };
        let mut pads: Vec<&str> = Vec::new();
        for i in &passes {
            for r in &of(*i).rows {
                if !pads.contains(&r.pad.as_str()) {
                    pads.push(&r.pad);
                }
            }
        }
        let rows = pads
            .into_iter()
            .map(|pad| {
                let mut row = a
                    .row(pad)
                    .map(|r| PatternRow {
                        marks: Vec::new(),
                        steps: Vec::new(),
                        ..r.clone()
                    })
                    .unwrap_or_else(|| PatternRow::new(pad, Vec::new()));
                for (pass, i) in passes.iter().enumerate() {
                    let p = of(*i);
                    let at = (pass * length) as u16;
                    match p.row(pad) {
                        Some(r) => {
                            row.steps.extend(r.steps.iter().take(length));
                            row.steps.resize((pass + 1) * length, 0.0);
                            row.marks.extend(r.marks.iter().map(|m| StepMark {
                                step: m.step + at,
                                ..*m
                            }));
                        }
                        None => row.steps.resize((pass + 1) * length, 0.0),
                    }
                }
                row
            })
            .collect();
        Pattern {
            length: (length * passes.len()) as u16,
            rows,
            ..a.clone()
        }
    }
}

/// When a section plays a fill: every few bars, at its end, or both, as
/// a pattern clip's fill does.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionFill {
    /// The section whose first bar is the fill.
    pub section: String,
    /// The last bar of every this many is a fill; 0 for none.
    #[serde(default)]
    pub every: u16,
    /// The section's last bar in the order is a fill.
    #[serde(default)]
    pub at_end: bool,
}

/// One step of a part's order: a section, played through so many times.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderStep {
    /// The section, by name.
    pub section: String,
    /// Passes of its pattern, 1 to [`MAX_REPEAT`].
    pub times: u16,
}

/// A part as the drum designer placed it on a drum track: the part
/// itself, so the designer can open on it again, the tick it starts on,
/// and what placing it put in the project, so placing again can take it
/// back out.
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedPart {
    /// The part as it was placed.
    pub part: DrumPart,
    /// The timeline tick it starts on.
    pub start: u64,
    /// The pattern clips placing it made.
    pub clips: Vec<PatternClipId>,
    /// The patterns placing it made: one per section.
    pub patterns: Vec<PatternId>,
}

/// Where placing a part puts it on a drum track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaceMode {
    /// In place of what the designer placed last, starting where it
    /// started; a track never placed on has its pattern clips replaced,
    /// from the start of the first.
    Replace,
    /// After the track's last pattern clip, on the next bar line.
    Append,
}

/// The edit that places `part` on drum track `track`, as one command:
/// what is replaced comes out (clips, and patterns the designer made that
/// nothing else plays), a pattern goes in for each section (a whole round
/// of its variations), a clip for each step of the order with the
/// section's fill, the kit when it differs, and the part, so the designer
/// can open on it again. Clips are in ticks, `bar` a bar, so they stay on
/// their bars whatever the tempo does.
pub fn place_part(
    project: &Project,
    track: TrackId,
    part: &DrumPart,
    mode: PlaceMode,
    bar: u64,
) -> Result<Command, String> {
    part.validate()?;
    let t = project
        .track(track)
        .ok_or_else(|| format!("no track {track}"))?;
    if !t.is_drums() {
        return Err(format!("{} is not a drum track", t.name));
    }
    if part.order_ticks() == 0 {
        return Err("the order is empty: there is nothing to place".into());
    }
    let bar = bar.max(1);
    // What comes out: the last placement's clips (or, never placed, all
    // the track's clips) when replacing.
    let replaced: Vec<&PatternClip> = match (mode, &t.part) {
        (PlaceMode::Append, _) => Vec::new(),
        (PlaceMode::Replace, Some(placed)) => t
            .pattern_clips
            .iter()
            .filter(|c| placed.clips.contains(&c.id))
            .collect(),
        (PlaceMode::Replace, None) => t.pattern_clips.iter().collect(),
    };
    let start = match (mode, &t.part) {
        (PlaceMode::Replace, Some(placed)) => placed.start,
        (PlaceMode::Replace, None) => replaced.iter().map(|c| c.start).min().unwrap_or(0),
        (PlaceMode::Append, _) => t
            .pattern_clips
            .iter()
            .map(PatternClip::end)
            .max()
            .map_or(0, |end| end.div_ceil(bar) * bar),
    };
    let mut commands = Vec::new();
    for c in &replaced {
        commands.push(Command::RemovePatternClip { id: c.id });
    }
    // Patterns the last placement made that no clip plays once its clips
    // are gone.
    let gone: Vec<PatternClipId> = replaced.iter().map(|c| c.id).collect();
    let still_played = |id: PatternId| {
        project.tracks.iter().any(|tr| {
            tr.pattern_clips.iter().any(|c| {
                !gone.contains(&c.id)
                    && (c.pattern == id || c.fill.as_ref().is_some_and(|f| f.pattern == id))
            })
        })
    };
    let mut removed_patterns = Vec::new();
    if mode == PlaceMode::Replace {
        if let Some(placed) = &t.part {
            for id in &placed.patterns {
                if project.pattern(*id).is_some() && !still_played(*id) {
                    commands.push(Command::RemovePattern { id: *id });
                    removed_patterns.push(*id);
                }
            }
        }
    }
    // Fresh ids, past every id the project has.
    let mut next = project.fresh_id();
    let mut fresh = || {
        let id = next;
        next += 1;
        id
    };
    let mut taken: Vec<String> = project
        .patterns
        .iter()
        .filter(|p| !removed_patterns.contains(&p.id))
        .map(|p| p.name.clone())
        .collect();
    let mut ids = Vec::new();
    for (i, s) in part.sections.iter().enumerate() {
        let id = PatternId(fresh());
        let name = unique_name(
            &format!("{} {}", t.name, s.name),
            taken.iter().map(String::as_str),
        );
        taken.push(name.clone());
        commands.push(Command::InsertPattern {
            index: project.patterns.len() + i,
            pattern: Pattern {
                id,
                name,
                ..s.played()
            },
        });
        ids.push(id);
    }
    let mut clips = Vec::new();
    for span in part.layout() {
        let section = &part.sections[span.section];
        let fill = section.fill.as_ref().and_then(|f| {
            part.section_index(&f.section).map(|i| Fill {
                pattern: ids[i],
                every: f.every,
                at_end: f.at_end,
            })
        });
        let id = PatternClipId(fresh());
        commands.push(Command::InsertPatternClip {
            track,
            clip: PatternClip {
                id,
                pattern: ids[span.section],
                start: start + span.start,
                offset: 0,
                length: span.length,
                fill,
            },
        });
        clips.push(id);
    }
    if t.kit.as_deref() != Some(part.kit.as_str()) {
        commands.push(Command::SetTrackKit {
            id: track,
            kit: part.kit.clone(),
        });
    }
    // First, so the history names the edit after it.
    commands.insert(
        0,
        Command::SetTrackPart {
            track,
            part: Some(PlacedPart {
                part: part.clone(),
                start,
                clips,
                patterns: ids,
            }),
        },
    );
    Ok(Command::Batch { commands })
}

/// Where a step of the order falls, in ticks from the top of the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The step of the order.
    pub step: usize,
    /// The section it plays, by index.
    pub section: usize,
    /// Tick it starts on.
    pub start: u64,
    /// Ticks it lasts.
    pub length: u64,
}

impl DrumPart {
    /// The section called `name`.
    pub fn section(&self, name: &str) -> Option<&PartSection> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The index of the section called `name`.
    pub fn section_index(&self, name: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.name == name)
    }

    /// Where each step of the order falls: one after another from the top,
    /// each its section's pattern `times` over. Steps naming no section are
    /// left out.
    pub fn layout(&self) -> Vec<Span> {
        let mut at = 0;
        let mut spans = Vec::new();
        for (step, o) in self.order.iter().enumerate() {
            let Some(section) = self.section_index(&o.section) else {
                continue;
            };
            let length = self.sections[section].pattern.ticks() * u64::from(o.times.max(1));
            spans.push(Span {
                step,
                section,
                start: at,
                length,
            });
            at += length;
        }
        spans
    }

    /// Bars of the order, counted from 0 at the top, `bar` ticks each,
    /// where a section's fill plays instead of it: the last bar of every
    /// `every` of the step, and its last bar with `at_end`, as a pattern
    /// clip's fill does.
    pub fn fill_bars(&self, bar: u64) -> Vec<u64> {
        let bar = bar.max(1);
        let mut out = Vec::new();
        for span in self.layout() {
            let section = &self.sections[span.section];
            let Some(fill) = section
                .fill
                .as_ref()
                .filter(|f| self.section(&f.section).is_some())
            else {
                continue;
            };
            let fill = crate::model::Fill {
                pattern: PatternId(0),
                every: fill.every,
                at_end: fill.at_end,
            };
            let bars = span.length / bar;
            let first = span.start / bar;
            out.extend(
                (0..bars)
                    .filter(|b| fill.plays_at(*b, bars))
                    .map(|b| first + b),
            );
        }
        out
    }

    /// Bars the whole order lasts, `bar` ticks each, rounded up.
    pub fn order_bars(&self, bar: u64) -> u64 {
        self.order_ticks().div_ceil(bar.max(1))
    }

    /// Ticks the whole order lasts.
    pub fn order_ticks(&self) -> u64 {
        self.layout().last().map_or(0, |s| s.start + s.length)
    }

    /// Checks the part: a kit, at least one section, names that are there
    /// and unique, patterns that are valid, and an order and fills that
    /// name sections it has.
    pub fn validate(&self) -> Result<(), String> {
        if self.kit.trim().is_empty() {
            return Err("the part has no kit".into());
        }
        if self.sections.is_empty() {
            return Err("the part has no sections".into());
        }
        let mut names = std::collections::HashSet::new();
        for s in &self.sections {
            if s.name.trim().is_empty() {
                return Err("a section has no name".into());
            }
            if !names.insert(s.name.as_str()) {
                return Err(format!("two sections are called {:?}", s.name));
            }
            s.pattern
                .validate()
                .map_err(|e| format!("section {:?}: {e}", s.name))?;
            if s.variations.len() > MAX_VARIATIONS {
                return Err(format!(
                    "section {:?} has {} variations; B, C and D at most",
                    s.name,
                    s.variations.len()
                ));
            }
            for (i, v) in s.variations.iter().enumerate() {
                let letter = (b'B' + i as u8) as char;
                v.validate()
                    .map_err(|e| format!("section {:?} variation {letter}: {e}", s.name))?;
                if v.length != s.pattern.length || v.steps_per_beat != s.pattern.steps_per_beat {
                    return Err(format!(
                        "section {:?} variation {letter} is not the same steps as A",
                        s.name
                    ));
                }
            }
        }
        for s in &self.sections {
            if let Some(f) = &s.fill {
                if f.section == s.name {
                    return Err(format!("section {:?} fills with itself", s.name));
                }
                if !names.contains(f.section.as_str()) {
                    return Err(format!(
                        "section {:?} fills with {:?}, which the part does not have",
                        s.name, f.section
                    ));
                }
                if f.every > MAX_REPEAT {
                    return Err(format!(
                        "section {:?} fills every {} bars; at most {MAX_REPEAT}",
                        s.name, f.every
                    ));
                }
            }
        }
        for (i, o) in self.order.iter().enumerate() {
            if !names.contains(o.section.as_str()) {
                return Err(format!(
                    "step {} of the order plays {:?}, which the part does not have",
                    i + 1,
                    o.section
                ));
            }
            if !(1..=MAX_REPEAT).contains(&o.times) {
                return Err(format!(
                    "step {} of the order plays {} times; 1 to {MAX_REPEAT}",
                    i + 1,
                    o.times
                ));
            }
        }
        Ok(())
    }

    /// The part a drum track plays now: the part the designer last placed
    /// on it, if it was; else a section for each pattern its clips play,
    /// in the order they first come, and an order that follows the clips.
    /// A track with no clips gets a section for each of the project's
    /// patterns, and with none of those an empty verse.
    pub fn from_track(project: &Project, track: &Track) -> Self {
        if let Some(placed) = &track.part {
            return placed.part.clone();
        }
        let kit = track.kit.clone().unwrap_or_default();
        let mut clips: Vec<_> = track.pattern_clips.iter().collect();
        clips.sort_by_key(|c| c.start);
        let mut used: Vec<PatternId> = Vec::new();
        for c in &clips {
            for id in std::iter::once(c.pattern).chain(c.fill.as_ref().map(|f| f.pattern)) {
                if !used.contains(&id) && project.pattern(id).is_some() {
                    used.push(id);
                }
            }
        }
        if used.is_empty() {
            used = project.patterns.iter().map(|p| p.id).collect();
        }
        let mut sections: Vec<PartSection> = Vec::new();
        let mut name_of = std::collections::HashMap::new();
        for id in used {
            let Some(pattern) = project.pattern(id) else {
                continue;
            };
            let name = unique_name(&pattern.name, sections.iter().map(|s| s.name.as_str()));
            name_of.insert(id, name.clone());
            sections.push(PartSection {
                kind: kind_of(&pattern.name),
                name,
                pattern: pattern.clone(),
                fill: None,
                variations: Vec::new(),
                vary: Vary::Alternate,
            });
        }
        if sections.is_empty() {
            let mut pattern = Pattern::new(PatternId(1), "Verse");
            pattern.length = 16;
            sections.push(PartSection {
                name: "Verse".into(),
                kind: Some(Section::Verse),
                pattern,
                fill: None,
                variations: Vec::new(),
                vary: Vary::Alternate,
            });
        }
        let mut order = Vec::new();
        for c in &clips {
            let Some(name) = name_of.get(&c.pattern) else {
                continue;
            };
            let ticks = project.pattern(c.pattern).map_or(1, |p| p.ticks().max(1));
            let times = c.length.div_ceil(ticks).clamp(1, u64::from(MAX_REPEAT)) as u16;
            if let Some(f) = &c.fill {
                if let (Some(fill), Some(i)) = (
                    name_of.get(&f.pattern),
                    sections.iter().position(|s| &s.name == name),
                ) {
                    if fill != name && sections[i].fill.is_none() {
                        sections[i].fill = Some(SectionFill {
                            section: fill.clone(),
                            every: f.every.min(MAX_REPEAT),
                            at_end: f.at_end,
                        });
                    }
                }
            }
            match order.last_mut() {
                Some(OrderStep { section, times: t })
                    if section == name && *t + times <= MAX_REPEAT =>
                {
                    *t += times
                }
                _ => order.push(OrderStep {
                    section: name.clone(),
                    times,
                }),
            }
        }
        if order.is_empty() {
            order = sections
                .iter()
                .filter(|s| s.kind != Some(Section::Fill))
                .map(|s| OrderStep {
                    section: s.name.clone(),
                    times: 1,
                })
                .collect();
        }
        Self {
            kit,
            sections,
            order,
        }
    }
}

/// The kind of section a name says, when it says one: "Verse", "chorus
/// 2" and "Rock — Fill" all do.
pub fn kind_of(name: &str) -> Option<Section> {
    let lower = name.to_lowercase();
    [
        ("intro", Section::Intro),
        ("verse", Section::Verse),
        ("chorus", Section::Chorus),
        ("bridge", Section::Bridge),
        ("fill", Section::Fill),
        ("ending", Section::Ending),
        ("outro", Section::Ending),
    ]
    .into_iter()
    .find(|(word, _)| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == *word)
    })
    .map(|(_, s)| s)
}

/// `name`, or `name 2`, `name 3`… if it is taken.
pub fn unique_name<'a>(name: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let base = if name.trim().is_empty() {
        "Section"
    } else {
        name.trim()
    };
    if !taken.clone().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|candidate| !taken.clone().any(|t| t == candidate))
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Fill, PatternClip, PatternClipId, PatternRow, TrackId};

    fn pattern(id: u64, name: &str, length: u16) -> Pattern {
        let mut p = Pattern::new(PatternId(id), name);
        p.length = length;
        p.rows
            .push(PatternRow::new("kick", vec![0.0; length as usize]));
        p
    }

    fn clip(id: u64, pattern: u64, start: u64, length: u64) -> PatternClip {
        PatternClip {
            id: PatternClipId(id),
            pattern: PatternId(pattern),
            start,
            offset: 0,
            length,
            fill: None,
        }
    }

    const BAR: u64 = 3840;

    #[test]
    fn a_track_becomes_sections_and_an_order() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![
            pattern(1, "Verse", 16),
            pattern(2, "Chorus", 32),
            pattern(3, "Fill", 16),
        ];
        let mut track = Track::drums(TrackId(9), "Drums", "rock");
        track.pattern_clips = vec![
            clip(10, 1, 0, 2 * BAR),
            clip(11, 1, 2 * BAR, 2 * BAR),
            clip(12, 2, 4 * BAR, 4 * BAR),
        ];
        track.pattern_clips[2].fill = Some(Fill {
            pattern: PatternId(3),
            every: 4,
            at_end: false,
        });
        let part = DrumPart::from_track(&project, &track);
        assert_eq!(part.kit, "rock");
        let names: Vec<_> = part.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Verse", "Chorus", "Fill"]);
        assert_eq!(part.sections[0].kind, Some(Section::Verse));
        assert_eq!(
            part.order,
            vec![
                OrderStep {
                    section: "Verse".into(),
                    times: 4
                },
                OrderStep {
                    section: "Chorus".into(),
                    times: 2
                },
            ]
        );
        assert_eq!(
            part.sections[1].fill,
            Some(SectionFill {
                section: "Fill".into(),
                every: 4,
                at_end: false
            })
        );
        part.validate().unwrap();
        assert_eq!(part.order_ticks(), 8 * BAR);
    }

    #[test]
    fn a_track_without_clips_takes_the_projects_patterns_or_a_verse() {
        let mut project = Project::new("Song", 48_000.0);
        let track = Track::drums(TrackId(9), "Drums", "rock");
        let empty = DrumPart::from_track(&project, &track);
        assert_eq!(empty.sections.len(), 1);
        assert_eq!(empty.sections[0].kind, Some(Section::Verse));
        assert_eq!(empty.order.len(), 1);
        empty.validate().unwrap();

        project.patterns = vec![pattern(1, "Beat", 16), pattern(2, "Beat", 16)];
        let part = DrumPart::from_track(&project, &track);
        let names: Vec<_> = part.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Beat", "Beat 2"]);
        assert_eq!(part.order.len(), 2);
        part.validate().unwrap();
    }

    #[test]
    fn the_layout_follows_the_order() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![pattern(1, "Verse", 16), pattern(2, "Chorus", 32)];
        let mut part = DrumPart::from_track(&project, &Track::drums(TrackId(1), "D", "k"));
        part.order = vec![
            OrderStep {
                section: "Verse".into(),
                times: 2,
            },
            OrderStep {
                section: "Chorus".into(),
                times: 1,
            },
        ];
        let spans = part.layout();
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].length, 2 * BAR);
        assert_eq!(spans[1].start, 2 * BAR);
        assert_eq!(spans[1].length, 2 * BAR);
    }

    #[test]
    fn validation_catches_what_does_not_add_up() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![pattern(1, "Verse", 16)];
        let good = DrumPart::from_track(&project, &Track::drums(TrackId(1), "D", "k"));
        let mut bad = good.clone();
        bad.order[0].section = "Nope".into();
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.order[0].times = 0;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.sections.push(bad.sections[0].clone());
        assert!(bad.validate().unwrap_err().contains("two sections"));
        let mut bad = good;
        bad.sections[0].fill = Some(SectionFill {
            section: "Verse".into(),
            every: 4,
            at_end: false,
        });
        assert!(bad.validate().unwrap_err().contains("itself"));
    }

    #[test]
    fn the_order_counts_its_bars() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![pattern(1, "Verse", 16), pattern(2, "Chorus", 32)];
        let mut part = DrumPart::from_track(&project, &Track::drums(TrackId(1), "D", "k"));
        part.order = vec![
            OrderStep {
                section: "Verse".into(),
                times: 4,
            },
            OrderStep {
                section: "Chorus".into(),
                times: 2,
            },
            OrderStep {
                section: "Verse".into(),
                times: 1,
            },
        ];
        assert_eq!(part.order_bars(BAR), 4 + 4 + 1);
        // In 3/4 the same ticks are more bars.
        assert_eq!(part.order_bars(2880), (9 * BAR).div_ceil(2880));
        part.order[0].times = 3;
        assert_eq!(part.layout()[1].start, 3 * BAR);
    }

    #[test]
    fn fills_fall_on_their_bars() {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![
            pattern(1, "Verse", 16),
            pattern(2, "Chorus", 32),
            pattern(3, "Fill", 16),
        ];
        let mut part = DrumPart::from_track(&project, &Track::drums(TrackId(1), "D", "k"));
        part.order = vec![
            OrderStep {
                section: "Verse".into(),
                times: 8,
            },
            OrderStep {
                section: "Chorus".into(),
                times: 2,
            },
        ];
        part.sections[0].fill = Some(SectionFill {
            section: "Fill".into(),
            every: 4,
            at_end: false,
        });
        part.sections[1].fill = Some(SectionFill {
            section: "Fill".into(),
            every: 0,
            at_end: true,
        });
        // The verse's bars 4 and 8, then the chorus's last (bar 12).
        assert_eq!(part.fill_bars(BAR), vec![3, 7, 11]);
        // A fill naming a section that is gone plays nothing.
        part.sections.retain(|s| s.name != "Fill");
        assert!(part.fill_bars(BAR).is_empty());
    }

    #[test]
    fn variations_take_turns() {
        let mut project = Project::new("Song", 48_000.0);
        let mut a = pattern(1, "Verse", 4);
        a.rows[0].steps = vec![1.0, 0.0, 0.0, 0.0];
        a.rows[0].marks = vec![StepMark {
            flam: true,
            ..StepMark::plain(0)
        }];
        project.patterns = vec![a];
        let mut part = DrumPart::from_track(&project, &Track::drums(TrackId(1), "D", "k"));
        let section = &mut part.sections[0];
        assert_eq!(section.played(), section.pattern);
        let mut b = pattern(2, "B", 4);
        b.rows = vec![PatternRow::new("snare", vec![0.0, 0.0, 1.0, 0.0])];
        let mut c = pattern(3, "C", 4);
        c.rows[0].steps = vec![0.0, 0.0, 0.0, 0.5];
        section.variations = vec![b, c];
        assert_eq!(section.passes(), [0, 1, 0, 2]);
        let played = section.played();
        assert_eq!(played.length, 16);
        assert_eq!(
            played.row("kick").unwrap().steps,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5]
        );
        assert_eq!(played.row("snare").unwrap().steps[6], 1.0);
        let marks: Vec<_> = played
            .row("kick")
            .unwrap()
            .marks
            .iter()
            .map(|m| m.step)
            .collect();
        assert_eq!(marks, [0, 8]);
        played.validate().unwrap();
        section.vary = Vary::Chain;
        assert_eq!(section.passes(), [0, 1, 2]);
        assert_eq!(section.played().length, 12);
        part.validate().unwrap();
        part.sections[0].variations[0].steps_per_beat = 8;
        assert!(part.validate().unwrap_err().contains("same steps"));
    }

    /// A project with a drum track playing two bars of a verse, and a
    /// part of a two-bar verse, a one-bar fill every 4 and a chorus.
    fn placing() -> (Project, DrumPart) {
        let mut project = Project::new("Song", 48_000.0);
        project.patterns = vec![pattern(1, "Verse", 16)];
        let mut track = Track::drums(TrackId(9), "Drums", "rock");
        track.pattern_clips = vec![clip(10, 1, 0, 2 * BAR)];
        project.tracks.push(track);
        let mut part = DrumPart::from_track(&project, &project.tracks[0]);
        part.kit = "metal".into();
        part.sections[0].pattern.length = 32;
        for r in &mut part.sections[0].pattern.rows {
            r.steps.resize(32, 0.0);
        }
        part.sections.push(PartSection {
            name: "Fill".into(),
            kind: Some(Section::Fill),
            pattern: pattern(2, "Fill", 16),
            fill: None,
            variations: Vec::new(),
            vary: Vary::Alternate,
        });
        part.sections.push(PartSection {
            name: "Chorus".into(),
            kind: Some(Section::Chorus),
            pattern: pattern(3, "Chorus", 16),
            fill: None,
            variations: Vec::new(),
            vary: Vary::Alternate,
        });
        part.sections[0].fill = Some(SectionFill {
            section: "Fill".into(),
            every: 4,
            at_end: false,
        });
        part.order = vec![
            OrderStep {
                section: "Verse".into(),
                times: 2,
            },
            OrderStep {
                section: "Chorus".into(),
                times: 4,
            },
        ];
        (project, part)
    }

    #[test]
    fn placing_puts_the_order_on_the_track_as_one_undoable_edit() {
        let (mut project, part) = placing();
        let before = project.clone();
        let cmd = place_part(&project, TrackId(9), &part, PlaceMode::Replace, BAR).unwrap();
        assert_eq!(cmd.label(), "place drum part");
        let undo = cmd.apply(&mut project).unwrap();
        let t = project.track(TrackId(9)).unwrap();
        // The track's own clip is replaced by a clip per step of the order.
        let spans: Vec<_> = t
            .pattern_clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        assert_eq!(spans, [(0, 4 * BAR), (4 * BAR, 4 * BAR)]);
        assert_eq!(t.kit.as_deref(), Some("metal"));
        let placed = t.part.as_ref().unwrap();
        assert_eq!(placed.part, part);
        assert_eq!(placed.clips.len(), 2);
        assert_eq!(placed.patterns.len(), 3);
        // The verse's clip plays the fill every four bars.
        let verse = &t.pattern_clips[0];
        let fill = verse.fill.as_ref().unwrap();
        assert_eq!((fill.every, fill.pattern), (4, placed.patterns[1]));
        // Patterns named after the track, the old one kept.
        let names: Vec<_> = project.patterns.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["Verse", "Drums Verse", "Drums Fill", "Drums Chorus"]
        );
        project.validate().unwrap();
        // Reopened, the designer gets the part back as placed.
        assert_eq!(
            DrumPart::from_track(&project, project.track(TrackId(9)).unwrap()),
            part
        );
        // One undo puts everything back.
        undo.apply(&mut project).unwrap();
        assert_eq!(project, before);
    }

    #[test]
    fn placing_again_replaces_what_it_placed_or_appends() {
        let (mut project, part) = placing();
        place_part(&project, TrackId(9), &part, PlaceMode::Replace, BAR)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        // A clip of the user's after it stays through a replace.
        let id = PatternClipId(project.fresh_id());
        Command::InsertPatternClip {
            track: TrackId(9),
            clip: clip(id.0, 1, 10 * BAR, BAR),
        }
        .apply(&mut project)
        .unwrap();
        let mut revised = part.clone();
        revised.order[1].times = 2;
        let first = project.track(TrackId(9)).unwrap().part.clone().unwrap();
        place_part(&project, TrackId(9), &revised, PlaceMode::Replace, BAR)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        let t = project.track(TrackId(9)).unwrap();
        let spans: Vec<_> = t
            .pattern_clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        assert_eq!(spans, [(0, 4 * BAR), (4 * BAR, 2 * BAR), (10 * BAR, BAR)]);
        // The first placing's patterns are gone; its names are free again.
        assert!(first.patterns.iter().all(|p| project.pattern(*p).is_none()));
        assert_eq!(project.patterns.len(), 4);
        project.validate().unwrap();

        // Appended, it starts on the bar after the last clip.
        place_part(&project, TrackId(9), &revised, PlaceMode::Append, BAR)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        let t = project.track(TrackId(9)).unwrap();
        let starts: Vec<_> = t.pattern_clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, [0, 4 * BAR, 10 * BAR, 11 * BAR, 15 * BAR]);
        assert_eq!(t.part.as_ref().unwrap().start, 11 * BAR);
        project.validate().unwrap();
    }

    #[test]
    fn placed_clips_stay_on_their_bars_through_a_tempo_change() {
        let (mut project, part) = placing();
        place_part(&project, TrackId(9), &part, PlaceMode::Replace, BAR)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        Command::SetTempo {
            tempo: crate::model::Tempo {
                bpm: 87.0,
                ..project.tempo.clone()
            },
        }
        .apply(&mut project)
        .unwrap();
        // Positions are ticks: every clip still starts and ends on a bar.
        for c in &project.track(TrackId(9)).unwrap().pattern_clips {
            assert_eq!(c.start % BAR, 0);
            assert_eq!(c.length % BAR, 0);
        }
    }

    #[test]
    fn placing_refuses_an_empty_order_or_an_audio_track() {
        let (mut project, mut part) = placing();
        project.tracks.push(Track::new(TrackId(20), "Guitar"));
        assert!(place_part(&project, TrackId(20), &part, PlaceMode::Replace, BAR).is_err());
        part.order.clear();
        assert!(
            place_part(&project, TrackId(9), &part, PlaceMode::Replace, BAR)
                .unwrap_err()
                .contains("nothing to place")
        );
    }

    #[test]
    fn names_say_their_kind() {
        assert_eq!(kind_of("Rock — Fill"), Some(Section::Fill));
        assert_eq!(kind_of("chorus 2"), Some(Section::Chorus));
        assert_eq!(kind_of("Outro"), Some(Section::Ending));
        assert_eq!(kind_of("Breakdown"), None);
        assert_eq!(kind_of("Fillet"), None);
    }
}
