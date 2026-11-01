//! Every edit to a project is a [`Command`]. Applying one returns the
//! command that undoes it, so undo is "apply the inverse", redo is "apply
//! the original", and a log of applied commands replays into the same
//! project. Commands carry the ids of the objects they create, which is
//! what makes a replay land on the same ids.
//!
//! A command either applies completely or leaves the project untouched:
//! a [`Command::Batch`] that fails part way reverts what it had done
//! before returning the error.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{
    Clip, ClipId, Pattern, PatternClip, PatternClipId, PatternId, PatternRow, Pedal, PedalId,
    Project, Strip, Tempo, Track, TrackId, MAX_PATTERN_STEPS,
};

/// Why a command was refused. The project is unchanged when one of these
/// comes back.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error("no track {0}")]
    NoTrack(TrackId),
    #[error("no clip {0}")]
    NoClip(ClipId),
    #[error("no pedal {0}")]
    NoPedal(PedalId),
    #[error("no pattern {0}")]
    NoPattern(PatternId),
    #[error("no pattern clip {0}")]
    NoPatternClip(PatternClipId),
    #[error("pedal {pedal} has no parameter {param}")]
    NoParam { pedal: PedalId, param: usize },
    #[error("id {0} is already in use")]
    IdInUse(u64),
    #[error("{0}")]
    Invalid(String),
}

/// An edit with an inverse. Serialized with a `type` tag so the journal
/// and the UI can read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    /// Adds a track at `index` (clamped to the end). The inverse of
    /// `RemoveTrack`, carrying everything the track held.
    InsertTrack {
        index: usize,
        track: Track,
    },
    RemoveTrack {
        id: TrackId,
    },
    RenameTrack {
        id: TrackId,
        name: String,
    },
    MoveTrack {
        id: TrackId,
        to: usize,
    },
    ArmTrack {
        id: TrackId,
        armed: bool,
    },
    InsertClip {
        track: TrackId,
        clip: Clip,
    },
    RemoveClip {
        id: ClipId,
    },
    /// Puts a clip at `start` on `track`, which may be another track.
    MoveClip {
        id: ClipId,
        track: TrackId,
        start: u64,
    },
    /// Sets a clip's whole geometry. Trimming either edge is a special
    /// case; so is undoing a split.
    TrimClip {
        id: ClipId,
        start: u64,
        offset: u64,
        length: u64,
    },
    /// Cuts a clip at timeline sample `at`, strictly inside it. The right
    /// part becomes a new clip with `new_id`.
    SplitClip {
        id: ClipId,
        at: u64,
        new_id: ClipId,
    },
    InsertPedal {
        track: TrackId,
        index: usize,
        pedal: Pedal,
    },
    RemovePedal {
        id: PedalId,
    },
    MovePedal {
        id: PedalId,
        to: usize,
    },
    /// Swaps what a pedal is (a cabinet's impulse response) keeping its
    /// id, position and values.
    SetPedalSpec {
        id: PedalId,
        spec: String,
    },
    /// Replaces every parameter value of a pedal at once: what the
    /// designer does when a pedal's parameters change shape.
    SetPedalValues {
        id: PedalId,
        values: Vec<f32>,
    },
    SetParam {
        pedal: PedalId,
        param: usize,
        value: f32,
    },
    SetTempo {
        tempo: Tempo,
    },
    /// Gain in decibels on a track's strip, or the master's with no track.
    SetGain {
        track: Option<TrackId>,
        db: f32,
    },
    SetPan {
        track: TrackId,
        pan: f32,
    },
    /// Mute on a track's strip, or the master's with no track.
    SetMute {
        track: Option<TrackId>,
        on: bool,
    },
    SetSolo {
        track: TrackId,
        on: bool,
    },
    /// Changes a drum track's kit. Refused on an audio track.
    SetTrackKit {
        id: TrackId,
        kit: String,
    },
    /// Adds a pattern at `index` (clamped to the end).
    InsertPattern {
        index: usize,
        pattern: Pattern,
    },
    RemovePattern {
        id: PatternId,
    },
    RenamePattern {
        id: PatternId,
        name: String,
    },
    /// Sets one cell. A velocity of zero clears it; a row with no hits
    /// left is dropped, and a hit on a pad with no row makes one.
    SetPatternStep {
        pattern: PatternId,
        pad: String,
        step: usize,
        velocity: f32,
    },
    /// Sets the step count. Rows are cut or padded with silence; the
    /// inverse carries the rows as they were.
    SetPatternLength {
        id: PatternId,
        length: u16,
    },
    /// Steps per beat: 1, 2, 3, 4, 6 or 8. The steps keep their
    /// velocities and play faster or slower.
    SetPatternResolution {
        id: PatternId,
        steps_per_beat: u8,
    },
    SetPatternSwing {
        id: PatternId,
        swing: f32,
    },
    /// Replaces a pattern whole. The inverse of a length change.
    ReplacePattern {
        pattern: Pattern,
    },
    /// Puts a pattern on a drum track. Positions in ticks.
    InsertPatternClip {
        track: TrackId,
        clip: PatternClip,
    },
    RemovePatternClip {
        id: PatternClipId,
    },
    /// Puts a pattern clip at `start` on `track`, which may be another
    /// drum track.
    MovePatternClip {
        id: PatternClipId,
        track: TrackId,
        start: u64,
    },
    /// Sets a pattern clip's whole geometry, in ticks.
    TrimPatternClip {
        id: PatternClipId,
        start: u64,
        offset: u64,
        length: u64,
    },
    /// Cuts a pattern clip at tick `at`, strictly inside it. The right
    /// part becomes a new clip with `new_id`.
    SplitPatternClip {
        id: PatternClipId,
        at: u64,
        new_id: PatternClipId,
    },
    /// Several commands as one edit. Applied in order; the inverse
    /// reverts them in reverse.
    Batch {
        commands: Vec<Command>,
    },
}

impl Command {
    /// A short description for an undo menu.
    pub fn label(&self) -> String {
        match self {
            Command::InsertTrack { track, .. } => format!("add track {}", track.name),
            Command::RemoveTrack { .. } => "remove track".into(),
            Command::RenameTrack { name, .. } => format!("rename track to {name}"),
            Command::MoveTrack { .. } => "move track".into(),
            Command::ArmTrack { armed: true, .. } => "arm track".into(),
            Command::ArmTrack { armed: false, .. } => "disarm track".into(),
            Command::InsertClip { clip, .. } => format!("add clip {}", clip.name),
            Command::RemoveClip { .. } => "delete clip".into(),
            Command::MoveClip { .. } => "move clip".into(),
            Command::TrimClip { .. } => "trim clip".into(),
            Command::SplitClip { .. } => "split clip".into(),
            Command::InsertPedal { pedal, .. } => format!("add {}", pedal.spec),
            Command::RemovePedal { .. } => "remove pedal".into(),
            Command::MovePedal { .. } => "move pedal".into(),
            Command::SetPedalSpec { .. } => "change pedal".into(),
            Command::SetPedalValues { .. } => "change pedal knobs".into(),
            Command::SetParam { .. } => "change parameter".into(),
            Command::SetTempo { .. } => "change tempo".into(),
            Command::SetGain { track: None, .. } => "change master gain".into(),
            Command::SetGain { .. } => "change gain".into(),
            Command::SetPan { .. } => "change pan".into(),
            Command::SetMute { on, .. } => if *on { "mute" } else { "unmute" }.into(),
            Command::SetSolo { on, .. } => if *on { "solo" } else { "unsolo" }.into(),
            Command::SetTrackKit { kit, .. } => format!("change kit to {kit}"),
            Command::InsertPattern { pattern, .. } => format!("add pattern {}", pattern.name),
            Command::RemovePattern { .. } => "remove pattern".into(),
            Command::RenamePattern { name, .. } => format!("rename pattern to {name}"),
            Command::SetPatternStep { velocity, .. } if *velocity > 0.0 => "set step".into(),
            Command::SetPatternStep { .. } => "clear step".into(),
            Command::SetPatternLength { .. } => "change pattern length".into(),
            Command::SetPatternResolution { .. } => "change step resolution".into(),
            Command::SetPatternSwing { .. } => "change swing".into(),
            Command::ReplacePattern { .. } => "edit pattern".into(),
            Command::InsertPatternClip { .. } => "place pattern".into(),
            Command::RemovePatternClip { .. } => "delete pattern clip".into(),
            Command::MovePatternClip { .. } => "move pattern clip".into(),
            Command::TrimPatternClip { .. } => "trim pattern clip".into(),
            Command::SplitPatternClip { .. } => "split pattern clip".into(),
            Command::Batch { commands } => commands
                .first()
                .map_or_else(|| "edit".into(), Command::label),
        }
    }

    /// True for a command that only changes a continuous value, which an
    /// undo history may merge with the previous one of the same kind.
    pub fn coalesce_key(&self) -> Option<String> {
        match self {
            Command::SetParam { pedal, param, .. } => Some(format!("param:{pedal}:{param}")),
            Command::SetTempo { .. } => Some("tempo".into()),
            Command::SetGain { track, .. } => Some(format!("gain:{}", strip_key(*track))),
            Command::SetPan { track, .. } => Some(format!("pan:{track}")),
            Command::SetPatternStep {
                pattern, pad, step, ..
            } => Some(format!("step:{pattern}:{pad}:{step}")),
            Command::SetPatternSwing { id, .. } => Some(format!("swing:{id}")),
            _ => None,
        }
    }

    /// Applies the command and returns its inverse. On an error the
    /// project is as it was.
    pub fn apply(&self, project: &mut Project) -> Result<Command, CommandError> {
        match self {
            Command::InsertTrack { index, track } => {
                ensure_fresh(project, track.id.0)?;
                for clip in &track.clips {
                    ensure_fresh(project, clip.id.0)?;
                }
                for pedal in &track.chain {
                    ensure_fresh(project, pedal.id.0)?;
                }
                let index = (*index).min(project.tracks.len());
                project.tracks.insert(index, track.clone());
                Ok(Command::RemoveTrack { id: track.id })
            }
            Command::RemoveTrack { id } => {
                let index = track_index(project, *id)?;
                let track = project.tracks.remove(index);
                Ok(Command::InsertTrack { index, track })
            }
            Command::RenameTrack { id, name } => {
                let track = track_mut(project, *id)?;
                let was = std::mem::replace(&mut track.name, name.clone());
                Ok(Command::RenameTrack { id: *id, name: was })
            }
            Command::MoveTrack { id, to } => {
                let from = track_index(project, *id)?;
                let to = (*to).min(project.tracks.len() - 1);
                let track = project.tracks.remove(from);
                project.tracks.insert(to, track);
                Ok(Command::MoveTrack { id: *id, to: from })
            }
            Command::ArmTrack { id, armed } => {
                let track = track_mut(project, *id)?;
                let was = std::mem::replace(&mut track.armed, *armed);
                Ok(Command::ArmTrack {
                    id: *id,
                    armed: was,
                })
            }
            Command::InsertClip { track, clip } => {
                if clip.length == 0 {
                    return Err(CommandError::Invalid(format!(
                        "clip {} has no length",
                        clip.name
                    )));
                }
                ensure_fresh(project, clip.id.0)?;
                let t = track_mut(project, *track)?;
                t.clips.push(clip.clone());
                t.clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::RemoveClip { id: clip.id })
            }
            Command::RemoveClip { id } => {
                let (track, index) = clip_index(project, *id)?;
                let clip = project.tracks[track].clips.remove(index);
                Ok(Command::InsertClip {
                    track: project.tracks[track].id,
                    clip,
                })
            }
            Command::MoveClip { id, track, start } => {
                let (from, index) = clip_index(project, *id)?;
                let to = track_index(project, *track)?;
                let from_id = project.tracks[from].id;
                let mut clip = project.tracks[from].clips.remove(index);
                let was = clip.start;
                clip.start = *start;
                let dest = &mut project.tracks[to];
                dest.clips.push(clip);
                dest.clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::MoveClip {
                    id: *id,
                    track: from_id,
                    start: was,
                })
            }
            Command::TrimClip {
                id,
                start,
                offset,
                length,
            } => {
                if *length == 0 {
                    return Err(CommandError::Invalid(
                        "a clip cannot be trimmed to nothing".into(),
                    ));
                }
                let (track, index) = clip_index(project, *id)?;
                let clips = &mut project.tracks[track].clips;
                let clip = &mut clips[index];
                let inverse = Command::TrimClip {
                    id: *id,
                    start: clip.start,
                    offset: clip.offset,
                    length: clip.length,
                };
                clip.start = *start;
                clip.offset = *offset;
                clip.length = *length;
                clips.sort_by_key(|c| (c.start, c.id));
                Ok(inverse)
            }
            Command::SplitClip { id, at, new_id } => {
                ensure_fresh(project, new_id.0)?;
                let (track, index) = clip_index(project, *id)?;
                let clips = &mut project.tracks[track].clips;
                let clip = &mut clips[index];
                if *at <= clip.start || *at >= clip.end() {
                    return Err(CommandError::Invalid(format!(
                        "sample {at} is not inside clip {} ({}..{})",
                        clip.id,
                        clip.start,
                        clip.end()
                    )));
                }
                let left = at - clip.start;
                let original = Command::TrimClip {
                    id: *id,
                    start: clip.start,
                    offset: clip.offset,
                    length: clip.length,
                };
                let right = Clip {
                    id: *new_id,
                    name: clip.name.clone(),
                    file: clip.file.clone(),
                    start: *at,
                    offset: clip.offset + left,
                    length: clip.length - left,
                };
                clip.length = left;
                clips.insert(index + 1, right);
                clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::Batch {
                    commands: vec![Command::RemoveClip { id: *new_id }, original],
                })
            }
            Command::InsertPedal {
                track,
                index,
                pedal,
            } => {
                ensure_fresh(project, pedal.id.0)?;
                let t = track_mut(project, *track)?;
                let index = (*index).min(t.chain.len());
                t.chain.insert(index, pedal.clone());
                Ok(Command::RemovePedal { id: pedal.id })
            }
            Command::RemovePedal { id } => {
                let (track, index) = pedal_index(project, *id)?;
                let pedal = project.tracks[track].chain.remove(index);
                Ok(Command::InsertPedal {
                    track: project.tracks[track].id,
                    index,
                    pedal,
                })
            }
            Command::MovePedal { id, to } => {
                let (track, from) = pedal_index(project, *id)?;
                let chain = &mut project.tracks[track].chain;
                let to = (*to).min(chain.len() - 1);
                let pedal = chain.remove(from);
                chain.insert(to, pedal);
                Ok(Command::MovePedal { id: *id, to: from })
            }
            Command::SetPedalSpec { id, spec } => {
                let (track, index) = pedal_index(project, *id)?;
                let pedal = &mut project.tracks[track].chain[index];
                let was = std::mem::replace(&mut pedal.spec, spec.clone());
                Ok(Command::SetPedalSpec { id: *id, spec: was })
            }
            Command::SetPedalValues { id, values } => {
                if let Some(bad) = values.iter().find(|v| !v.is_finite()) {
                    return Err(CommandError::Invalid(format!(
                        "{bad} is not a usable parameter value"
                    )));
                }
                let (track, index) = pedal_index(project, *id)?;
                let pedal = &mut project.tracks[track].chain[index];
                let was = std::mem::replace(&mut pedal.values, values.clone());
                Ok(Command::SetPedalValues {
                    id: *id,
                    values: was,
                })
            }
            Command::SetParam {
                pedal,
                param,
                value,
            } => {
                if !value.is_finite() {
                    return Err(CommandError::Invalid(format!(
                        "{value} is not a usable parameter value"
                    )));
                }
                let (track, index) = pedal_index(project, *pedal)?;
                let values = &mut project.tracks[track].chain[index].values;
                let slot = values.get_mut(*param).ok_or(CommandError::NoParam {
                    pedal: *pedal,
                    param: *param,
                })?;
                let was = std::mem::replace(slot, *value);
                Ok(Command::SetParam {
                    pedal: *pedal,
                    param: *param,
                    value: was,
                })
            }
            Command::SetTempo { tempo } => {
                if !(tempo.bpm.is_finite() && tempo.bpm > 0.0)
                    || tempo.numerator == 0
                    || tempo.denominator == 0
                {
                    return Err(CommandError::Invalid(format!(
                        "{} bpm in {}/{} is not a usable tempo",
                        tempo.bpm, tempo.numerator, tempo.denominator
                    )));
                }
                let was = std::mem::replace(&mut project.tempo, tempo.clone());
                Ok(Command::SetTempo { tempo: was })
            }
            Command::SetGain { track, db } => {
                if !db.is_finite() {
                    return Err(CommandError::Invalid(format!(
                        "{db} dB is not a usable gain"
                    )));
                }
                let strip = strip_mut(project, *track)?;
                let was = std::mem::replace(&mut strip.gain_db, *db);
                Ok(Command::SetGain {
                    track: *track,
                    db: was,
                })
            }
            Command::SetPan { track, pan } => {
                if !pan.is_finite() || !(-1.0..=1.0).contains(pan) {
                    return Err(CommandError::Invalid(format!(
                        "{pan} is not a pan position"
                    )));
                }
                let strip = strip_mut(project, Some(*track))?;
                let was = std::mem::replace(&mut strip.pan, *pan);
                Ok(Command::SetPan {
                    track: *track,
                    pan: was,
                })
            }
            Command::SetMute { track, on } => {
                let strip = strip_mut(project, *track)?;
                let was = std::mem::replace(&mut strip.mute, *on);
                Ok(Command::SetMute {
                    track: *track,
                    on: was,
                })
            }
            Command::SetSolo { track, on } => {
                let strip = strip_mut(project, Some(*track))?;
                let was = std::mem::replace(&mut strip.solo, *on);
                Ok(Command::SetSolo {
                    track: *track,
                    on: was,
                })
            }
            Command::SetTrackKit { id, kit } => {
                if kit.is_empty() || kit.contains(['/', '\\']) {
                    return Err(CommandError::Invalid(format!("{kit:?} is not a kit id")));
                }
                let track = track_mut(project, *id)?;
                let Some(slot) = track.kit.as_mut() else {
                    return Err(CommandError::Invalid(format!(
                        "track {} is an audio track and has no kit",
                        track.name
                    )));
                };
                let was = std::mem::replace(slot, kit.clone());
                Ok(Command::SetTrackKit { id: *id, kit: was })
            }
            Command::InsertPattern { index, pattern } => {
                check_pattern(pattern)?;
                ensure_fresh(project, pattern.id.0)?;
                let index = (*index).min(project.patterns.len());
                let mut pattern = pattern.clone();
                pattern.rows.sort_by(|a, b| a.pad.cmp(&b.pad));
                project.patterns.insert(index, pattern);
                Ok(Command::RemovePattern {
                    id: pattern_id_at(project, index),
                })
            }
            Command::RemovePattern { id } => {
                let index = pattern_index(project, *id)?;
                if project.pattern_in_use(*id) {
                    return Err(CommandError::Invalid(format!(
                        "pattern {} is on the timeline; delete its clips first",
                        project.patterns[index].name
                    )));
                }
                let pattern = project.patterns.remove(index);
                Ok(Command::InsertPattern { index, pattern })
            }
            Command::RenamePattern { id, name } => {
                if name.is_empty() {
                    return Err(CommandError::Invalid("a pattern needs a name".into()));
                }
                let pattern = pattern_mut(project, *id)?;
                let was = std::mem::replace(&mut pattern.name, name.clone());
                Ok(Command::RenamePattern { id: *id, name: was })
            }
            Command::SetPatternStep {
                pattern,
                pad,
                step,
                velocity,
            } => {
                if !velocity.is_finite() || !(0.0..=1.0).contains(velocity) {
                    return Err(CommandError::Invalid(format!(
                        "{velocity} is not a velocity between 0 and 1"
                    )));
                }
                if pad.is_empty() {
                    return Err(CommandError::Invalid("a step needs a pad".into()));
                }
                let p = pattern_mut(project, *pattern)?;
                if *step >= p.length as usize {
                    return Err(CommandError::Invalid(format!(
                        "step {step} is past the end of a {}-step pattern",
                        p.length
                    )));
                }
                let was = p.velocity(pad, *step);
                match p.rows.iter().position(|r| &r.pad == pad) {
                    Some(i) => {
                        p.rows[i].steps[*step] = *velocity;
                        if p.rows[i].steps.iter().all(|v| *v == 0.0) {
                            p.rows.remove(i);
                        }
                    }
                    None if *velocity > 0.0 => {
                        let mut steps = vec![0.0; p.length as usize];
                        steps[*step] = *velocity;
                        p.rows.push(PatternRow {
                            pad: pad.clone(),
                            steps,
                        });
                        p.rows.sort_by(|a, b| a.pad.cmp(&b.pad));
                    }
                    None => {}
                }
                Ok(Command::SetPatternStep {
                    pattern: *pattern,
                    pad: pad.clone(),
                    step: *step,
                    velocity: was,
                })
            }
            Command::SetPatternLength { id, length } => {
                if *length == 0 || *length as usize > MAX_PATTERN_STEPS {
                    return Err(CommandError::Invalid(format!(
                        "a pattern has 1 to {MAX_PATTERN_STEPS} steps, not {length}"
                    )));
                }
                let p = pattern_mut(project, *id)?;
                let was = p.clone();
                p.length = *length;
                for row in &mut p.rows {
                    row.steps.resize(*length as usize, 0.0);
                }
                p.rows.retain(|r| r.steps.iter().any(|v| *v > 0.0));
                Ok(Command::ReplacePattern { pattern: was })
            }
            Command::SetPatternResolution { id, steps_per_beat } => {
                if !matches!(steps_per_beat, 1 | 2 | 3 | 4 | 6 | 8) {
                    return Err(CommandError::Invalid(format!(
                        "{steps_per_beat} steps per beat; 1, 2, 3, 4, 6 or 8 are allowed"
                    )));
                }
                let p = pattern_mut(project, *id)?;
                let was = std::mem::replace(&mut p.steps_per_beat, *steps_per_beat);
                Ok(Command::SetPatternResolution {
                    id: *id,
                    steps_per_beat: was,
                })
            }
            Command::SetPatternSwing { id, swing } => {
                if !swing.is_finite() || !(0.0..=1.0).contains(swing) {
                    return Err(CommandError::Invalid(format!(
                        "{swing} is not a swing between 0 and 1"
                    )));
                }
                let p = pattern_mut(project, *id)?;
                let was = std::mem::replace(&mut p.swing, *swing);
                Ok(Command::SetPatternSwing {
                    id: *id,
                    swing: was,
                })
            }
            Command::ReplacePattern { pattern } => {
                check_pattern(pattern)?;
                let index = pattern_index(project, pattern.id)?;
                let mut pattern = pattern.clone();
                pattern.rows.sort_by(|a, b| a.pad.cmp(&b.pad));
                let was = std::mem::replace(&mut project.patterns[index], pattern);
                Ok(Command::ReplacePattern { pattern: was })
            }
            Command::InsertPatternClip { track, clip } => {
                if clip.length == 0 {
                    return Err(CommandError::Invalid(
                        "a pattern clip needs a length".into(),
                    ));
                }
                ensure_fresh(project, clip.id.0)?;
                pattern_index(project, clip.pattern)?;
                let t = track_mut(project, *track)?;
                if !t.is_drums() {
                    return Err(CommandError::Invalid(format!(
                        "{} is an audio track; patterns go on drum tracks",
                        t.name
                    )));
                }
                t.pattern_clips.push(clip.clone());
                t.pattern_clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::RemovePatternClip { id: clip.id })
            }
            Command::RemovePatternClip { id } => {
                let (track, index) = pattern_clip_index(project, *id)?;
                let clip = project.tracks[track].pattern_clips.remove(index);
                Ok(Command::InsertPatternClip {
                    track: project.tracks[track].id,
                    clip,
                })
            }
            Command::MovePatternClip { id, track, start } => {
                let (from, index) = pattern_clip_index(project, *id)?;
                let to = track_index(project, *track)?;
                if !project.tracks[to].is_drums() {
                    return Err(CommandError::Invalid(format!(
                        "{} is an audio track; patterns go on drum tracks",
                        project.tracks[to].name
                    )));
                }
                let from_id = project.tracks[from].id;
                let mut clip = project.tracks[from].pattern_clips.remove(index);
                let was = clip.start;
                clip.start = *start;
                let dest = &mut project.tracks[to];
                dest.pattern_clips.push(clip);
                dest.pattern_clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::MovePatternClip {
                    id: *id,
                    track: from_id,
                    start: was,
                })
            }
            Command::TrimPatternClip {
                id,
                start,
                offset,
                length,
            } => {
                if *length == 0 {
                    return Err(CommandError::Invalid(
                        "a pattern clip cannot be trimmed to nothing".into(),
                    ));
                }
                let (track, index) = pattern_clip_index(project, *id)?;
                let clips = &mut project.tracks[track].pattern_clips;
                let clip = &mut clips[index];
                let inverse = Command::TrimPatternClip {
                    id: *id,
                    start: clip.start,
                    offset: clip.offset,
                    length: clip.length,
                };
                clip.start = *start;
                clip.offset = *offset;
                clip.length = *length;
                clips.sort_by_key(|c| (c.start, c.id));
                Ok(inverse)
            }
            Command::SplitPatternClip { id, at, new_id } => {
                ensure_fresh(project, new_id.0)?;
                let (track, index) = pattern_clip_index(project, *id)?;
                let clips = &mut project.tracks[track].pattern_clips;
                let clip = &mut clips[index];
                if *at <= clip.start || *at >= clip.end() {
                    return Err(CommandError::Invalid(format!(
                        "tick {at} is not inside pattern clip {} ({}..{})",
                        clip.id,
                        clip.start,
                        clip.end()
                    )));
                }
                let left = at - clip.start;
                let original = Command::TrimPatternClip {
                    id: *id,
                    start: clip.start,
                    offset: clip.offset,
                    length: clip.length,
                };
                let right = PatternClip {
                    id: *new_id,
                    pattern: clip.pattern,
                    start: *at,
                    offset: clip.offset + left,
                    length: clip.length - left,
                };
                clip.length = left;
                clips.insert(index + 1, right);
                clips.sort_by_key(|c| (c.start, c.id));
                Ok(Command::Batch {
                    commands: vec![Command::RemovePatternClip { id: *new_id }, original],
                })
            }
            Command::Batch { commands } => {
                let mut inverses = Vec::with_capacity(commands.len());
                for command in commands {
                    match command.apply(project) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(e) => {
                            for inverse in inverses.iter().rev() {
                                inverse
                                    .apply(project)
                                    .expect("reverting an applied command cannot fail");
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                Ok(Command::Batch { commands: inverses })
            }
        }
    }
}

fn strip_key(track: Option<TrackId>) -> String {
    track.map_or_else(|| "master".to_string(), |t| t.to_string())
}

/// A track's strip, or the master's for `None`.
fn strip_mut(project: &mut Project, track: Option<TrackId>) -> Result<&mut Strip, CommandError> {
    match track {
        None => Ok(&mut project.master),
        Some(id) => Ok(&mut track_mut(project, id)?.strip),
    }
}

fn ensure_fresh(project: &Project, id: u64) -> Result<(), CommandError> {
    if project.ids().any(|used| used == id) {
        Err(CommandError::IdInUse(id))
    } else {
        Ok(())
    }
}

fn track_index(project: &Project, id: TrackId) -> Result<usize, CommandError> {
    project.track_index(id).ok_or(CommandError::NoTrack(id))
}

fn track_mut(project: &mut Project, id: TrackId) -> Result<&mut Track, CommandError> {
    project.track_mut(id).ok_or(CommandError::NoTrack(id))
}

/// Track index and clip index within it.
fn clip_index(project: &Project, id: ClipId) -> Result<(usize, usize), CommandError> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(t, track)| track.clips.iter().position(|c| c.id == id).map(|c| (t, c)))
        .ok_or(CommandError::NoClip(id))
}

fn pattern_index(project: &Project, id: PatternId) -> Result<usize, CommandError> {
    project
        .patterns
        .iter()
        .position(|p| p.id == id)
        .ok_or(CommandError::NoPattern(id))
}

/// Track index and pattern clip index within it.
fn pattern_clip_index(
    project: &Project,
    id: PatternClipId,
) -> Result<(usize, usize), CommandError> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(t, track)| {
            track
                .pattern_clips
                .iter()
                .position(|c| c.id == id)
                .map(|c| (t, c))
        })
        .ok_or(CommandError::NoPatternClip(id))
}

fn pattern_id_at(project: &Project, index: usize) -> PatternId {
    project.patterns[index].id
}

fn pattern_mut(project: &mut Project, id: PatternId) -> Result<&mut Pattern, CommandError> {
    project.pattern_mut(id).ok_or(CommandError::NoPattern(id))
}

/// A pattern must be valid on its own before it goes in.
fn check_pattern(pattern: &Pattern) -> Result<(), CommandError> {
    let mut scratch = Project::new("check", 48_000.0);
    scratch.patterns.push(pattern.clone());
    scratch.validate().map_err(CommandError::Invalid)
}

fn pedal_index(project: &Project, id: PedalId) -> Result<(usize, usize), CommandError> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(t, track)| track.chain.iter().position(|p| p.id == id).map(|p| (t, p)))
        .ok_or(CommandError::NoPedal(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: u64, start: u64, length: u64) -> Clip {
        Clip {
            id: ClipId(id),
            name: format!("clip {id}"),
            file: format!("clip-{id}.wav"),
            start,
            offset: 0,
            length,
        }
    }

    fn project() -> Project {
        let mut p = Project::new("Song", 48_000.0);
        let mut t = Track::new(TrackId(1), "Guitar");
        t.chain.push(Pedal {
            id: PedalId(2),
            spec: "gain".into(),
            values: vec![0.0],
        });
        t.clips.push(clip(3, 0, 1000));
        p.tracks.push(t);
        p.tracks.push(Track::new(TrackId(4), "Bass"));
        p
    }

    /// Applies, checks the inverse restores the original, then reapplies.
    fn round_trip(p: &Project, command: Command) -> Project {
        let mut after = p.clone();
        let inverse = command.apply(&mut after).unwrap();
        assert_ne!(&after, p, "{command:?} changed nothing");
        let mut restored = after.clone();
        inverse.apply(&mut restored).unwrap();
        assert_eq!(&restored, p, "inverse of {command:?} did not restore");
        let mut again = p.clone();
        command.apply(&mut again).unwrap();
        assert_eq!(again, after, "{command:?} is not deterministic");
        after
    }

    #[test]
    fn tracks() {
        let p = project();
        let added = round_trip(
            &p,
            Command::InsertTrack {
                index: 99,
                track: Track::new(TrackId(5), "Drums"),
            },
        );
        assert_eq!(added.tracks[2].name, "Drums");
        round_trip(&p, Command::RemoveTrack { id: TrackId(1) });
        round_trip(
            &p,
            Command::RenameTrack {
                id: TrackId(4),
                name: "Bass VI".into(),
            },
        );
        let moved = round_trip(
            &p,
            Command::MoveTrack {
                id: TrackId(4),
                to: 0,
            },
        );
        assert_eq!(moved.tracks[0].id, TrackId(4));
        round_trip(
            &p,
            Command::ArmTrack {
                id: TrackId(1),
                armed: true,
            },
        );
        assert_eq!(
            Command::RemoveTrack { id: TrackId(9) }.apply(&mut p.clone()),
            Err(CommandError::NoTrack(TrackId(9)))
        );
        assert_eq!(
            Command::InsertTrack {
                index: 0,
                track: Track::new(TrackId(1), "dup")
            }
            .apply(&mut p.clone()),
            Err(CommandError::IdInUse(1))
        );
    }

    #[test]
    fn clips() {
        let p = project();
        let added = round_trip(
            &p,
            Command::InsertClip {
                track: TrackId(1),
                clip: clip(5, 2000, 500),
            },
        );
        assert_eq!(added.tracks[0].clips.len(), 2);
        round_trip(&p, Command::RemoveClip { id: ClipId(3) });
        let moved = round_trip(
            &p,
            Command::MoveClip {
                id: ClipId(3),
                track: TrackId(4),
                start: 100,
            },
        );
        assert!(moved.tracks[0].clips.is_empty());
        assert_eq!(moved.tracks[1].clips[0].start, 100);
        let trimmed = round_trip(
            &p,
            Command::TrimClip {
                id: ClipId(3),
                start: 10,
                offset: 10,
                length: 990,
            },
        );
        assert_eq!(trimmed.tracks[0].clips[0].offset, 10);
        let split = round_trip(
            &p,
            Command::SplitClip {
                id: ClipId(3),
                at: 400,
                new_id: ClipId(6),
            },
        );
        let clips = &split.tracks[0].clips;
        assert_eq!((clips[0].start, clips[0].length), (0, 400));
        assert_eq!(
            (
                clips[1].id,
                clips[1].start,
                clips[1].offset,
                clips[1].length
            ),
            (ClipId(6), 400, 400, 600)
        );
        for at in [0, 1000, 5000] {
            assert!(matches!(
                Command::SplitClip {
                    id: ClipId(3),
                    at,
                    new_id: ClipId(6)
                }
                .apply(&mut p.clone()),
                Err(CommandError::Invalid(_))
            ));
        }
        assert!(matches!(
            Command::TrimClip {
                id: ClipId(3),
                start: 0,
                offset: 0,
                length: 0
            }
            .apply(&mut p.clone()),
            Err(CommandError::Invalid(_))
        ));
    }

    #[test]
    fn clips_stay_sorted_by_start() {
        let mut p = project();
        Command::InsertClip {
            track: TrackId(1),
            clip: clip(5, 2000, 500),
        }
        .apply(&mut p)
        .unwrap();
        Command::MoveClip {
            id: ClipId(3),
            track: TrackId(1),
            start: 3000,
        }
        .apply(&mut p)
        .unwrap();
        let order: Vec<_> = p.tracks[0].clips.iter().map(|c| c.id).collect();
        assert_eq!(order, vec![ClipId(5), ClipId(3)]);
    }

    #[test]
    fn pedals_and_params() {
        let p = project();
        let added = round_trip(
            &p,
            Command::InsertPedal {
                track: TrackId(1),
                index: 0,
                pedal: Pedal {
                    id: PedalId(5),
                    spec: "overdrive".into(),
                    values: vec![20.0, 3000.0, -6.0],
                },
            },
        );
        assert_eq!(added.tracks[0].chain[0].id, PedalId(5));
        round_trip(&p, Command::RemovePedal { id: PedalId(2) });
        round_trip(
            &added,
            Command::MovePedal {
                id: PedalId(5),
                to: 1,
            },
        );
        let swapped = round_trip(
            &p,
            Command::SetPedalSpec {
                id: PedalId(2),
                spec: "cab:4x12-modern".into(),
            },
        );
        assert_eq!(swapped.tracks[0].chain[0].spec, "cab:4x12-modern");
        assert_eq!(
            swapped.tracks[0].chain[0].values,
            p.tracks[0].chain[0].values
        );
        assert!(matches!(
            Command::SetPedalSpec {
                id: PedalId(99),
                spec: "cab".into()
            }
            .apply(&mut p.clone()),
            Err(CommandError::NoPedal(PedalId(99)))
        ));
        let reshaped = round_trip(
            &p,
            Command::SetPedalValues {
                id: PedalId(2),
                values: vec![1.0, 2.0],
            },
        );
        assert_eq!(reshaped.tracks[0].chain[0].values, vec![1.0, 2.0]);
        assert!(matches!(
            Command::SetPedalValues {
                id: PedalId(2),
                values: vec![f32::NAN],
            }
            .apply(&mut p.clone()),
            Err(CommandError::Invalid(_))
        ));
        let set = round_trip(
            &p,
            Command::SetParam {
                pedal: PedalId(2),
                param: 0,
                value: -12.0,
            },
        );
        assert_eq!(set.tracks[0].chain[0].values[0], -12.0);
        assert_eq!(
            Command::SetParam {
                pedal: PedalId(2),
                param: 3,
                value: 1.0
            }
            .apply(&mut p.clone()),
            Err(CommandError::NoParam {
                pedal: PedalId(2),
                param: 3
            })
        );
        assert!(matches!(
            Command::SetParam {
                pedal: PedalId(2),
                param: 0,
                value: f32::NAN
            }
            .apply(&mut p.clone()),
            Err(CommandError::Invalid(_))
        ));
    }

    #[test]
    fn tempo() {
        let p = project();
        round_trip(
            &p,
            Command::SetTempo {
                tempo: Tempo {
                    bpm: 96.0,
                    numerator: 3,
                    denominator: 4,
                },
            },
        );
        assert!(matches!(
            Command::SetTempo {
                tempo: Tempo {
                    bpm: 0.0,
                    ..Tempo::default()
                }
            }
            .apply(&mut p.clone()),
            Err(CommandError::Invalid(_))
        ));
    }

    #[test]
    fn strips() {
        let p = project();
        let gained = round_trip(
            &p,
            Command::SetGain {
                track: Some(TrackId(1)),
                db: -6.0,
            },
        );
        assert_eq!(gained.tracks[0].strip.gain_db, -6.0);
        let master = round_trip(
            &p,
            Command::SetGain {
                track: None,
                db: -3.0,
            },
        );
        assert_eq!(master.master.gain_db, -3.0);
        round_trip(
            &p,
            Command::SetPan {
                track: TrackId(1),
                pan: 0.5,
            },
        );
        round_trip(
            &p,
            Command::SetMute {
                track: None,
                on: true,
            },
        );
        let soloed = round_trip(
            &p,
            Command::SetSolo {
                track: TrackId(4),
                on: true,
            },
        );
        assert!(soloed.tracks[1].strip.solo);
        assert!(matches!(
            Command::SetPan {
                track: TrackId(1),
                pan: 2.0
            }
            .apply(&mut p.clone()),
            Err(CommandError::Invalid(_))
        ));
        assert_eq!(
            Command::SetSolo {
                track: TrackId(9),
                on: true
            }
            .apply(&mut p.clone()),
            Err(CommandError::NoTrack(TrackId(9)))
        );
        assert_eq!(
            Command::SetGain {
                track: None,
                db: 0.0
            }
            .coalesce_key()
            .as_deref(),
            Some("gain:master")
        );
    }

    #[test]
    fn patterns() {
        let mut p = project();
        p.tracks.push(Track::drums(TrackId(7), "Drums", "acoustic"));
        let beat = Pattern::new(PatternId(8), "Beat");
        let added = round_trip(
            &p,
            Command::InsertPattern {
                index: 0,
                pattern: beat.clone(),
            },
        );
        assert_eq!(added.patterns[0].id, PatternId(8));
        round_trip(&added, Command::RemovePattern { id: PatternId(8) });
        round_trip(
            &added,
            Command::RenamePattern {
                id: PatternId(8),
                name: "Verse".into(),
            },
        );
        let hit = round_trip(
            &added,
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "snare".into(),
                step: 4,
                velocity: 0.9,
            },
        );
        assert_eq!(hit.patterns[0].rows.len(), 1, "a row appears for the pad");
        assert_eq!(hit.patterns[0].velocity("snare", 4), 0.9);
        let two = round_trip(
            &hit,
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "kick".into(),
                step: 0,
                velocity: 1.0,
            },
        );
        let pads: Vec<&str> = two.patterns[0]
            .rows
            .iter()
            .map(|r| r.pad.as_str())
            .collect();
        assert_eq!(pads, ["kick", "snare"], "rows stay sorted by pad");
        let cleared = round_trip(
            &two,
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "kick".into(),
                step: 0,
                velocity: 0.0,
            },
        );
        assert_eq!(cleared.patterns[0].rows.len(), 1, "an empty row goes away");
        // Clearing a cell that is already clear changes nothing, which
        // is not an error.
        let mut same = cleared.clone();
        Command::SetPatternStep {
            pattern: PatternId(8),
            pad: "ride".into(),
            step: 1,
            velocity: 0.0,
        }
        .apply(&mut same)
        .unwrap();
        assert_eq!(same, cleared);
        let shorter = round_trip(
            &two,
            Command::SetPatternLength {
                id: PatternId(8),
                length: 4,
            },
        );
        assert_eq!(shorter.patterns[0].length, 4);
        assert_eq!(
            shorter.patterns[0].rows.len(),
            1,
            "the snare at step 4 was cut"
        );
        assert_eq!(shorter.patterns[0].rows[0].steps.len(), 4);
        let longer = round_trip(
            &two,
            Command::SetPatternLength {
                id: PatternId(8),
                length: 32,
            },
        );
        assert_eq!(longer.patterns[0].rows[0].steps.len(), 32);
        round_trip(
            &two,
            Command::SetPatternResolution {
                id: PatternId(8),
                steps_per_beat: 8,
            },
        );
        round_trip(
            &two,
            Command::SetPatternSwing {
                id: PatternId(8),
                swing: 0.5,
            },
        );
        round_trip(
            &p,
            Command::SetTrackKit {
                id: TrackId(7),
                kit: "rock".into(),
            },
        );
        for bad in [
            Command::SetTrackKit {
                id: TrackId(1),
                kit: "rock".into(),
            },
            Command::SetTrackKit {
                id: TrackId(7),
                kit: "../x".into(),
            },
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "kick".into(),
                step: 16,
                velocity: 1.0,
            },
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "kick".into(),
                step: 0,
                velocity: 2.0,
            },
            Command::SetPatternLength {
                id: PatternId(8),
                length: 0,
            },
            Command::SetPatternResolution {
                id: PatternId(8),
                steps_per_beat: 5,
            },
            Command::SetPatternSwing {
                id: PatternId(8),
                swing: -1.0,
            },
            Command::RenamePattern {
                id: PatternId(8),
                name: String::new(),
            },
            Command::InsertPattern {
                index: 0,
                pattern: Pattern {
                    length: 0,
                    ..Pattern::new(PatternId(9), "x")
                },
            },
        ] {
            assert!(
                matches!(bad.apply(&mut added.clone()), Err(CommandError::Invalid(_))),
                "{bad:?} was not refused"
            );
        }
        assert_eq!(
            Command::RemovePattern { id: PatternId(99) }.apply(&mut p.clone()),
            Err(CommandError::NoPattern(PatternId(99)))
        );
        assert_eq!(
            Command::SetPatternStep {
                pattern: PatternId(8),
                pad: "kick".into(),
                step: 3,
                velocity: 0.5
            }
            .coalesce_key()
            .as_deref(),
            Some("step:8:kick:3")
        );
    }

    #[test]
    fn pattern_clips() {
        let mut p = project();
        p.tracks.push(Track::drums(TrackId(7), "Drums", "acoustic"));
        p.tracks
            .push(Track::drums(TrackId(8), "More drums", "rock"));
        p.patterns.push(Pattern::new(PatternId(9), "Beat"));
        let clip = |id: u64, start: u64, length: u64| PatternClip {
            id: PatternClipId(id),
            pattern: PatternId(9),
            start,
            offset: 0,
            length,
        };
        let placed = round_trip(
            &p,
            Command::InsertPatternClip {
                track: TrackId(7),
                clip: clip(10, 3840, 7680),
            },
        );
        assert_eq!(placed.tracks[2].pattern_clips.len(), 1);
        assert!(matches!(
            Command::RemovePattern { id: PatternId(9) }.apply(&mut placed.clone()),
            Err(CommandError::Invalid(_))
        ));
        round_trip(
            &placed,
            Command::RemovePatternClip {
                id: PatternClipId(10),
            },
        );
        let moved = round_trip(
            &placed,
            Command::MovePatternClip {
                id: PatternClipId(10),
                track: TrackId(8),
                start: 0,
            },
        );
        assert!(moved.tracks[2].pattern_clips.is_empty());
        assert_eq!(moved.tracks[3].pattern_clips[0].start, 0);
        let trimmed = round_trip(
            &placed,
            Command::TrimPatternClip {
                id: PatternClipId(10),
                start: 4800,
                offset: 960,
                length: 6720,
            },
        );
        assert_eq!(trimmed.tracks[2].pattern_clips[0].offset, 960);
        let split = round_trip(
            &placed,
            Command::SplitPatternClip {
                id: PatternClipId(10),
                at: 3840 + 1920,
                new_id: PatternClipId(11),
            },
        );
        let clips = &split.tracks[2].pattern_clips;
        assert_eq!((clips[0].start, clips[0].length), (3840, 1920));
        assert_eq!(
            (
                clips[1].id,
                clips[1].start,
                clips[1].offset,
                clips[1].length
            ),
            (PatternClipId(11), 5760, 1920, 5760)
        );
        for bad in [
            Command::InsertPatternClip {
                track: TrackId(1),
                clip: clip(12, 0, 100),
            },
            Command::InsertPatternClip {
                track: TrackId(7),
                clip: clip(12, 0, 0),
            },
            Command::InsertPatternClip {
                track: TrackId(7),
                clip: PatternClip {
                    pattern: PatternId(99),
                    ..clip(12, 0, 100)
                },
            },
            Command::MovePatternClip {
                id: PatternClipId(10),
                track: TrackId(1),
                start: 0,
            },
            Command::TrimPatternClip {
                id: PatternClipId(10),
                start: 0,
                offset: 0,
                length: 0,
            },
            Command::SplitPatternClip {
                id: PatternClipId(10),
                at: 3840,
                new_id: PatternClipId(11),
            },
        ] {
            let mut q = placed.clone();
            let err = bad.apply(&mut q).unwrap_err();
            assert!(
                matches!(err, CommandError::Invalid(_) | CommandError::NoPattern(_)),
                "{bad:?}: {err}"
            );
            assert_eq!(q, placed, "{bad:?} changed the project");
        }
        assert_eq!(
            Command::RemovePatternClip {
                id: PatternClipId(99)
            }
            .apply(&mut p.clone()),
            Err(CommandError::NoPatternClip(PatternClipId(99)))
        );
    }

    #[test]
    fn batch_is_atomic() {
        let p = project();
        round_trip(
            &p,
            Command::Batch {
                commands: vec![
                    Command::RemoveClip { id: ClipId(3) },
                    Command::RemoveTrack { id: TrackId(1) },
                ],
            },
        );
        let mut failed = p.clone();
        let err = Command::Batch {
            commands: vec![
                Command::RemoveClip { id: ClipId(3) },
                Command::RemoveTrack { id: TrackId(99) },
            ],
        }
        .apply(&mut failed)
        .unwrap_err();
        assert_eq!(err, CommandError::NoTrack(TrackId(99)));
        assert_eq!(failed, p, "a failed batch left changes behind");
    }

    #[test]
    fn serializes_with_a_type_tag() {
        let json = serde_json::to_value(Command::ArmTrack {
            id: TrackId(1),
            armed: true,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "armTrack", "id": 1, "armed": true})
        );
        let back: Command = serde_json::from_value(json).unwrap();
        assert_eq!(
            back,
            Command::ArmTrack {
                id: TrackId(1),
                armed: true
            }
        );
    }

    #[test]
    fn labels_and_coalescing() {
        assert_eq!(
            Command::SetParam {
                pedal: PedalId(2),
                param: 1,
                value: 0.0
            }
            .coalesce_key()
            .as_deref(),
            Some("param:2:1")
        );
        assert_eq!(Command::RemoveClip { id: ClipId(1) }.coalesce_key(), None);
        assert_eq!(Command::RemoveClip { id: ClipId(1) }.label(), "delete clip");
    }
}
