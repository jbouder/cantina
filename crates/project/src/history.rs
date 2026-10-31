//! Undo and redo as a log of applied commands and their inverses.
//!
//! Every edit goes through [`History::apply`], which applies the command
//! and keeps its inverse. Undo applies the inverse and moves the pair to
//! the redo stack; redo applies the command again. A new edit clears the
//! redo stack, as every editor does.
//!
//! Continuous changes coalesce: a run of commands with the same
//! [`Command::coalesce_key`] within [`History::coalesce_window`] of each
//! other becomes one step whose inverse is the first value and whose
//! forward command is the last, so a fader drag undoes in one go.

use std::time::{Duration, Instant};

use crate::command::{Command, CommandError};
use crate::model::Project;

/// Steps kept; the oldest fall off beyond this.
pub const UNDO_LIMIT: usize = 1000;

/// How close two coalescable commands must be to merge, by default.
const DEFAULT_WINDOW: Duration = Duration::from_millis(800);

#[derive(Debug, Clone)]
struct Entry {
    command: Command,
    inverse: Command,
    label: String,
    key: Option<String>,
    at: Instant,
}

#[derive(Debug)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    window: Duration,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            window: DEFAULT_WINDOW,
        }
    }

    /// Sets how close two coalescable commands must be to merge. Zero
    /// turns coalescing off.
    pub fn coalesce_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Applies a command and records it. On an error the project and the
    /// history are unchanged.
    pub fn apply(&mut self, project: &mut Project, command: Command) -> Result<(), CommandError> {
        self.apply_at(project, command, Instant::now())
    }

    fn apply_at(
        &mut self,
        project: &mut Project,
        command: Command,
        now: Instant,
    ) -> Result<(), CommandError> {
        let inverse = command.apply(project)?;
        self.redo.clear();
        let key = command.coalesce_key();
        if let (Some(key), Some(top)) = (&key, self.undo.last_mut()) {
            if top.key.as_deref() == Some(key) && now.duration_since(top.at) < self.window {
                top.command = command;
                top.at = now;
                return Ok(());
            }
        }
        let label = command.label();
        self.undo.push(Entry {
            command,
            inverse,
            label,
            key,
            at: now,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        Ok(())
    }

    /// Reverts the last step. Returns its label, or `None` with nothing
    /// to undo. The step is kept for redo.
    pub fn undo(&mut self, project: &mut Project) -> Result<Option<String>, CommandError> {
        let Some(entry) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(e) = entry.inverse.apply(project) {
            self.undo.push(entry);
            return Err(e);
        }
        let label = entry.label.clone();
        self.redo.push(entry);
        Ok(Some(label))
    }

    /// Reapplies the last undone step. Returns its label, or `None`.
    pub fn redo(&mut self, project: &mut Project) -> Result<Option<String>, CommandError> {
        let Some(mut entry) = self.redo.pop() else {
            return Ok(None);
        };
        match entry.command.apply(project) {
            Ok(inverse) => {
                entry.inverse = inverse;
                // A redone step never merges with what came before it.
                entry.key = None;
                let label = entry.label.clone();
                self.undo.push(entry);
                Ok(Some(label))
            }
            Err(e) => {
                self.redo.push(entry);
                Err(e)
            }
        }
    }

    /// The last applied command, as it will be redone: the thing the
    /// shell should bring the graph in line with after an undo or redo
    /// is the command just applied, which these expose.
    pub fn last_undone(&self) -> Option<&Command> {
        self.redo.last().map(|e| &e.inverse)
    }

    pub fn last_redone(&self) -> Option<&Command> {
        self.undo.last().map(|e| &e.command)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// Forgets everything, as when another project is opened.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, ClipId, Pedal, PedalId, Strip, Tempo, Track, TrackId};
    use proptest::prelude::*;

    fn project() -> Project {
        let mut p = Project::new("Song", 48_000.0);
        let mut t = Track::new(TrackId(1), "Guitar");
        t.chain.push(Pedal {
            id: PedalId(2),
            spec: "overdrive".into(),
            values: vec![20.0, 3000.0, -6.0],
        });
        t.clips.push(Clip {
            id: ClipId(3),
            name: "take".into(),
            file: "take.wav".into(),
            start: 1000,
            offset: 0,
            length: 48_000,
        });
        p.tracks.push(t);
        p.tracks.push(Track::new(TrackId(4), "Bass"));
        p
    }

    #[test]
    fn undo_and_redo_walk_the_log() {
        let mut p = project();
        let original = p.clone();
        let mut h = History::new();
        h.apply(
            &mut p,
            Command::RenameTrack {
                id: TrackId(1),
                name: "Lead".into(),
            },
        )
        .unwrap();
        h.apply(&mut p, Command::RemoveClip { id: ClipId(3) })
            .unwrap();
        let after = p.clone();
        assert_eq!(h.undo_label(), Some("delete clip"));
        assert!(!h.can_redo());

        assert_eq!(h.undo(&mut p).unwrap().as_deref(), Some("delete clip"));
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert_eq!(h.redo_label(), Some("delete clip"));
        assert_eq!(
            h.undo(&mut p).unwrap().as_deref(),
            Some("rename track to Lead")
        );
        assert_eq!(p, original);
        assert_eq!(h.undo(&mut p).unwrap(), None);

        assert_eq!(
            h.redo(&mut p).unwrap().as_deref(),
            Some("rename track to Lead")
        );
        assert_eq!(h.redo(&mut p).unwrap().as_deref(), Some("delete clip"));
        assert_eq!(p, after);
        assert_eq!(h.redo(&mut p).unwrap(), None);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut p = project();
        let mut h = History::new();
        h.apply(&mut p, Command::RemoveClip { id: ClipId(3) })
            .unwrap();
        h.undo(&mut p).unwrap();
        assert!(h.can_redo());
        h.apply(
            &mut p,
            Command::ArmTrack {
                id: TrackId(1),
                armed: true,
            },
        )
        .unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn a_failed_command_leaves_no_trace() {
        let mut p = project();
        let mut h = History::new();
        assert!(h
            .apply(&mut p, Command::RemoveTrack { id: TrackId(99) })
            .is_err());
        assert!(h.is_empty());
        assert_eq!(p, project());
    }

    #[test]
    fn continuous_changes_coalesce_into_one_step() {
        let mut p = project();
        let mut h = History::new();
        let t0 = Instant::now();
        for (i, value) in [21.0, 24.0, 30.0].into_iter().enumerate() {
            h.apply_at(
                &mut p,
                Command::SetParam {
                    pedal: PedalId(2),
                    param: 0,
                    value,
                },
                t0 + Duration::from_millis(100 * i as u64),
            )
            .unwrap();
        }
        assert_eq!(h.len(), 1);
        // A different parameter is its own step.
        h.apply_at(
            &mut p,
            Command::SetParam {
                pedal: PedalId(2),
                param: 1,
                value: 1000.0,
            },
            t0 + Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(h.len(), 2);
        // The same parameter after a pause is a new step.
        h.apply_at(
            &mut p,
            Command::SetParam {
                pedal: PedalId(2),
                param: 0,
                value: 35.0,
            },
            t0 + Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(h.len(), 3);
        h.undo(&mut p).unwrap();
        assert_eq!(p.tracks[0].chain[0].values[0], 30.0);
        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        assert_eq!(p.tracks[0].chain[0].values[0], 20.0);
        h.redo(&mut p).unwrap();
        assert_eq!(
            p.tracks[0].chain[0].values[0], 30.0,
            "redo lands on the last value"
        );
    }

    #[test]
    fn the_log_is_bounded() {
        let mut p = project();
        let mut h = History::new().coalesce_window(Duration::ZERO);
        for i in 0..(UNDO_LIMIT + 10) {
            h.apply(
                &mut p,
                Command::SetTempo {
                    tempo: Tempo {
                        bpm: 60.0 + i as f64,
                        ..Tempo::default()
                    },
                },
            )
            .unwrap();
        }
        assert_eq!(h.len(), UNDO_LIMIT);
    }

    /// What a random edit does, resolved against the project it is
    /// applied to so most of them are valid; the invalid ones are
    /// refused and must leave nothing behind.
    #[derive(Debug, Clone)]
    enum Op {
        AddTrack(u8),
        RemoveTrack(usize),
        Rename(usize, u8),
        MoveTrack(usize, usize),
        Arm(usize, bool),
        AddClip(usize, u32, u32),
        RemoveClip(usize),
        MoveClip(usize, usize, u32),
        Trim(usize, u32, u32, u32),
        Split(usize, u32),
        AddPedal(usize, usize, u8),
        RemovePedal(usize),
        MovePedal(usize, usize),
        Param(usize, usize, f32),
        Tempo(u16),
        Gain(Option<usize>, f32),
        Pan(usize, f32),
        Mute(Option<usize>, bool),
        Solo(usize, bool),
        Batch(Vec<Op>),
    }

    fn op() -> impl Strategy<Value = Op> {
        let leaf = prop_oneof![
            any::<u8>().prop_map(Op::AddTrack),
            (0..4usize).prop_map(Op::RemoveTrack),
            (0..4usize, any::<u8>()).prop_map(|(t, n)| Op::Rename(t, n)),
            (0..4usize, 0..4usize).prop_map(|(t, to)| Op::MoveTrack(t, to)),
            (0..4usize, any::<bool>()).prop_map(|(t, a)| Op::Arm(t, a)),
            (0..4usize, 0..200_000u32, 1..50_000u32).prop_map(|(t, s, l)| Op::AddClip(t, s, l)),
            (0..8usize).prop_map(Op::RemoveClip),
            (0..8usize, 0..4usize, 0..200_000u32).prop_map(|(c, t, s)| Op::MoveClip(c, t, s)),
            (0..8usize, 0..200_000u32, 0..1000u32, 0..60_000u32)
                .prop_map(|(c, s, o, l)| Op::Trim(c, s, o, l)),
            (0..8usize, 0..200_000u32).prop_map(|(c, at)| Op::Split(c, at)),
            (0..4usize, 0..4usize, any::<u8>()).prop_map(|(t, i, k)| Op::AddPedal(t, i, k)),
            (0..8usize).prop_map(Op::RemovePedal),
            (0..8usize, 0..4usize).prop_map(|(p, to)| Op::MovePedal(p, to)),
            (0..8usize, 0..4usize, -60.0f32..60.0).prop_map(|(p, i, v)| Op::Param(p, i, v)),
            (40..240u16).prop_map(Op::Tempo),
            (proptest::option::of(0..4usize), -60.0f32..12.0).prop_map(|(t, v)| Op::Gain(t, v)),
            (0..4usize, -1.0f32..1.0).prop_map(|(t, v)| Op::Pan(t, v)),
            (proptest::option::of(0..4usize), any::<bool>()).prop_map(|(t, v)| Op::Mute(t, v)),
            (0..4usize, any::<bool>()).prop_map(|(t, v)| Op::Solo(t, v)),
        ];
        leaf.prop_recursive(2, 6, 3, |inner| {
            proptest::collection::vec(inner, 1..4).prop_map(Op::Batch)
        })
    }

    fn nth_track(p: &Project, i: usize) -> TrackId {
        p.tracks.get(i).map_or(TrackId(9999), |t| t.id)
    }

    fn nth_clip(p: &Project, i: usize) -> ClipId {
        p.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .nth(i)
            .map_or(ClipId(9999), |c| c.id)
    }

    fn nth_pedal(p: &Project, i: usize) -> PedalId {
        p.tracks
            .iter()
            .flat_map(|t| t.chain.iter())
            .nth(i)
            .map_or(PedalId(9999), |c| c.id)
    }

    /// Turns an op into a command against the current project. `extra`
    /// offsets fresh ids so the commands of one batch do not collide.
    fn resolve(p: &Project, op: &Op, extra: &mut u64) -> Command {
        let fresh = |extra: &mut u64| {
            *extra += 1;
            p.fresh_id() + *extra
        };
        match op {
            Op::AddTrack(n) => Command::InsertTrack {
                index: *n as usize,
                track: Track::new(TrackId(fresh(extra)), format!("T{n}")),
            },
            Op::RemoveTrack(i) => Command::RemoveTrack {
                id: nth_track(p, *i),
            },
            Op::Rename(i, n) => Command::RenameTrack {
                id: nth_track(p, *i),
                name: format!("N{n}"),
            },
            Op::MoveTrack(i, to) => Command::MoveTrack {
                id: nth_track(p, *i),
                to: *to,
            },
            Op::Arm(i, a) => Command::ArmTrack {
                id: nth_track(p, *i),
                armed: *a,
            },
            Op::AddClip(t, s, l) => Command::InsertClip {
                track: nth_track(p, *t),
                clip: Clip {
                    id: ClipId(fresh(extra)),
                    name: "c".into(),
                    file: "c.wav".into(),
                    start: *s as u64,
                    offset: 0,
                    length: *l as u64,
                },
            },
            Op::RemoveClip(c) => Command::RemoveClip {
                id: nth_clip(p, *c),
            },
            Op::MoveClip(c, t, s) => Command::MoveClip {
                id: nth_clip(p, *c),
                track: nth_track(p, *t),
                start: *s as u64,
            },
            Op::Trim(c, s, o, l) => Command::TrimClip {
                id: nth_clip(p, *c),
                start: *s as u64,
                offset: *o as u64,
                length: *l as u64,
            },
            Op::Split(c, at) => Command::SplitClip {
                id: nth_clip(p, *c),
                at: *at as u64,
                new_id: ClipId(fresh(extra)),
            },
            Op::AddPedal(t, i, k) => Command::InsertPedal {
                track: nth_track(p, *t),
                index: *i,
                pedal: Pedal {
                    id: PedalId(fresh(extra)),
                    spec: if k % 2 == 0 { "gain" } else { "overdrive" }.into(),
                    values: if k % 2 == 0 {
                        vec![0.0]
                    } else {
                        vec![20.0, 3000.0, -6.0]
                    },
                },
            },
            Op::RemovePedal(i) => Command::RemovePedal {
                id: nth_pedal(p, *i),
            },
            Op::MovePedal(i, to) => Command::MovePedal {
                id: nth_pedal(p, *i),
                to: *to,
            },
            Op::Param(i, param, v) => Command::SetParam {
                pedal: nth_pedal(p, *i),
                param: *param,
                value: *v,
            },
            Op::Tempo(bpm) => Command::SetTempo {
                tempo: Tempo {
                    bpm: *bpm as f64,
                    ..Tempo::default()
                },
            },
            Op::Gain(t, v) => Command::SetGain {
                track: t.map(|i| nth_track(p, i)),
                db: *v,
            },
            Op::Pan(t, v) => Command::SetPan {
                track: nth_track(p, *t),
                pan: *v,
            },
            Op::Mute(t, v) => Command::SetMute {
                track: t.map(|i| nth_track(p, i)),
                on: *v,
            },
            Op::Solo(t, v) => Command::SetSolo {
                track: nth_track(p, *t),
                on: *v,
            },
            Op::Batch(ops) => Command::Batch {
                commands: ops.iter().map(|o| resolve(p, o, extra)).collect(),
            },
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Any sequence of edits, undone in full, leaves the project
        /// exactly as it started; redone in full, exactly as it ended;
        /// and the model is valid at every step.
        #[test]
        fn undo_everything_restores_the_original(ops in proptest::collection::vec(op(), 1..40)) {
            let original = project();
            let mut p = original.clone();
            let mut h = History::new().coalesce_window(Duration::ZERO);
            let mut applied = 0;
            for op in &ops {
                let mut extra = 0;
                let command = resolve(&p, op, &mut extra);
                let before = p.clone();
                match h.apply(&mut p, command) {
                    Ok(()) => applied += 1,
                    Err(_) => prop_assert_eq!(&p, &before, "a refused command changed the project"),
                }
                prop_assert_eq!(p.validate(), Ok(()));
            }
            let end = p.clone();
            prop_assert_eq!(h.len(), applied.min(UNDO_LIMIT));
            while h.undo(&mut p).unwrap().is_some() {}
            prop_assert_eq!(&p, &original);
            while h.redo(&mut p).unwrap().is_some() {}
            prop_assert_eq!(&p, &end);
            prop_assert_eq!(p.master, end.master);
            let _ = Strip::default();
        }
    }
}
