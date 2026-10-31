//! The autosave journal: every applied command appended as one line of
//! JSON, so a crash loses nothing that was done and never corrupts
//! `project.json`, which is only written by a clean save.
//!
//! Each line carries the project's revision after the command. The
//! saved `project.json` carries the revision it was saved at, so on open
//! the lines with a higher revision are exactly the edits since the last
//! save, in order. A clean save clears the journal. A line that does not
//! parse is skipped with a warning rather than blocking the project; the
//! lines after it are still replayed, since each is self-contained.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::model::Project;

pub const JOURNAL_FILE: &str = "journal.jsonl";

/// One applied command and the revision it brought the project to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub revision: u64,
    pub command: Command,
}

/// The journal of one project directory.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
}

/// What a read found: the entries that parsed, and a warning per line
/// that did not.
#[derive(Debug, Default, PartialEq)]
pub struct Read {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

impl Journal {
    pub fn in_dir(project_dir: &Path) -> Self {
        Self {
            path: project_dir.join(JOURNAL_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one entry. The line is flushed to the OS before this
    /// returns, so a crash of the app after it keeps the entry; a crash
    /// of the machine may lose the tail, which is the trade for not
    /// waiting on the disk for every knob move.
    pub fn append(&self, revision: u64, command: &Command) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let entry = Entry {
            revision,
            command: command.clone(),
        };
        let mut line = serde_json::to_string(&entry).map_err(io::Error::other)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.flush()
    }

    /// Everything in the journal, in order. A missing file is empty.
    pub fn read(&self) -> io::Result<Read> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Read::default()),
            Err(e) => return Err(e),
        };
        let mut out = Read::default();
        for (i, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Entry>(&line) {
                Ok(entry) => out.entries.push(entry),
                Err(e) => out.warnings.push(format!(
                    "{} line {}: skipped, {e}",
                    self.path.display(),
                    i + 1
                )),
            }
        }
        Ok(out)
    }

    /// The entries newer than `revision`, in order, for replay over a
    /// project saved at that revision. Entries at or below it are from
    /// before the save and are dropped; a duplicate revision (the same
    /// edit journaled twice) is taken once.
    pub fn since(entries: Vec<Entry>, revision: u64) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for entry in entries {
            if entry.revision <= revision {
                continue;
            }
            if out
                .last()
                .is_some_and(|last| last.revision >= entry.revision)
            {
                continue;
            }
            out.push(entry);
        }
        out
    }

    /// Empties the journal, after a clean save.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Replays entries onto a project, bumping its revision to each entry's.
/// Stops at the first command that is refused and reports it; what was
/// replayed before it stays applied, since those edits were real.
pub fn replay(project: &mut Project, entries: &[Entry]) -> Result<usize, String> {
    for (i, entry) in entries.iter().enumerate() {
        if let Err(e) = entry.command.apply(project) {
            return Err(format!(
                "replayed {i} of {} edits; edit {} ({}) was refused: {e}",
                entries.len(),
                i + 1,
                entry.command.label()
            ));
        }
        project.revision = entry.revision;
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::model::{Clip, ClipId, Track, TrackId};

    fn project() -> Project {
        let mut p = Project::new("Song", 48_000.0);
        p.tracks.push(Track::new(TrackId(1), "Guitar"));
        p
    }

    fn clip(id: u64) -> Clip {
        Clip {
            id: ClipId(id),
            name: format!("c{id}"),
            file: format!("c{id}.wav"),
            start: id * 1000,
            offset: 0,
            length: 500,
        }
    }

    #[test]
    fn append_read_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::in_dir(dir.path());
        assert_eq!(journal.read().unwrap(), Read::default());
        journal
            .append(
                1,
                &Command::ArmTrack {
                    id: TrackId(1),
                    armed: true,
                },
            )
            .unwrap();
        journal
            .append(
                2,
                &Command::InsertClip {
                    track: TrackId(1),
                    clip: clip(2),
                },
            )
            .unwrap();
        let read = journal.read().unwrap();
        assert_eq!(read.entries.len(), 2);
        assert_eq!(read.entries[1].revision, 2);
        assert!(read.warnings.is_empty());
        journal.clear().unwrap();
        assert!(!journal.path().exists());
        assert_eq!(journal.read().unwrap(), Read::default());
        journal.clear().unwrap();
    }

    #[test]
    fn a_corrupt_line_is_skipped_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::in_dir(dir.path());
        journal
            .append(
                1,
                &Command::ArmTrack {
                    id: TrackId(1),
                    armed: true,
                },
            )
            .unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(journal.path())
            .unwrap();
        // A line cut short by a crash, then a good one after it.
        file.write_all(b"{\"revision\":2,\"command\":{\"type\":\"armTr\n")
            .unwrap();
        drop(file);
        journal
            .append(
                3,
                &Command::RenameTrack {
                    id: TrackId(1),
                    name: "Lead".into(),
                },
            )
            .unwrap();
        let read = journal.read().unwrap();
        assert_eq!(
            read.entries.iter().map(|e| e.revision).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(read.warnings.len(), 1);
        assert!(read.warnings[0].contains("line 2"), "{}", read.warnings[0]);
    }

    #[test]
    fn since_drops_stale_and_duplicate_entries() {
        let arm = Command::ArmTrack {
            id: TrackId(1),
            armed: true,
        };
        let entries: Vec<Entry> = [1, 2, 3, 3, 4]
            .into_iter()
            .map(|revision| Entry {
                revision,
                command: arm.clone(),
            })
            .collect();
        let kept = Journal::since(entries, 2);
        assert_eq!(
            kept.iter().map(|e| e.revision).collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    /// Edits are journaled, the app "crashes" before saving, and the
    /// relaunch replays the journal over the last save to the same state.
    #[test]
    fn replay_restores_the_exact_state_after_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("Song.cantina");
        let journal = Journal::in_dir(&project_dir);
        let mut p = project();
        crate::file::save(&p, &project_dir).unwrap();

        let mut h = History::new().coalesce_window(std::time::Duration::ZERO);
        let edits = vec![
            Command::InsertClip {
                track: TrackId(1),
                clip: clip(2),
            },
            Command::InsertClip {
                track: TrackId(1),
                clip: clip(3),
            },
            Command::SplitClip {
                id: ClipId(2),
                at: 2200,
                new_id: ClipId(4),
            },
            Command::RenameTrack {
                id: TrackId(1),
                name: "Lead".into(),
            },
        ];
        for command in edits {
            h.apply(&mut p, command.clone()).unwrap();
            p.revision += 1;
            journal.append(p.revision, &command).unwrap();
        }
        // An undo is an edit too, and is journaled as the inverse.
        h.undo(&mut p).unwrap();
        p.revision += 1;
        journal
            .append(p.revision, h.last_undone().unwrap())
            .unwrap();
        let live = p.clone();

        // The relaunch: the saved file is the old one.
        let mut reopened = crate::file::load(&project_dir).unwrap();
        assert_eq!(reopened.revision, 0);
        assert!(reopened.tracks[0].clips.is_empty());
        let read = journal.read().unwrap();
        let pending = Journal::since(read.entries, reopened.revision);
        assert_eq!(pending.len(), 5);
        assert_eq!(replay(&mut reopened, &pending).unwrap(), 5);
        assert_eq!(reopened, live);
        assert_eq!(reopened.tracks[0].name, "Guitar");
        assert_eq!(reopened.tracks[0].clips.len(), 3);
    }

    #[test]
    fn replay_stops_at_a_refused_edit_and_says_so() {
        let mut p = project();
        let entries = vec![
            Entry {
                revision: 1,
                command: Command::ArmTrack {
                    id: TrackId(1),
                    armed: true,
                },
            },
            Entry {
                revision: 2,
                command: Command::RemoveTrack { id: TrackId(99) },
            },
        ];
        let err = replay(&mut p, &entries).unwrap_err();
        assert!(err.contains("replayed 1 of 2"), "{err}");
        assert!(p.tracks[0].armed);
        assert_eq!(p.revision, 1);
    }
}
