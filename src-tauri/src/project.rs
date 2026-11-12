//! Where projects live and how they are opened.
//!
//! Projects are directories under one folder, `~/Music/Cantina/` by
//! default, and are referred to by name: `Song` is `~/Music/Cantina/Song.cantina/`.
//! A new project starts with one track carrying the stock chain, so the
//! first launch makes a sound.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_project::{Pedal, PedalId, Project, Tempo, Track, TrackId};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

/// The name a project gets when nothing else is asked for.
pub const UNTITLED: &str = "Untitled";

/// The pedals a new project's first track starts with.
const DEFAULT_CHAIN: &[(&str, &[f32])] = &[("gain", &[0.0]), ("overdrive", &[20.0, 3000.0, -6.0])];

/// The tracks a new project starts with. Mirrors `ProjectStart` in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Start {
    /// No tracks at all.
    Empty,
    /// One audio track with the stock chain: what a new project always had.
    #[default]
    Guitar,
    /// A guitar track and a bass track.
    GuitarBass,
    /// A guitar track and a drum track playing a kit.
    GuitarDrums,
}

/// What the start dialog asks of a new project. Every field but the name
/// may be left out, for the app's sample rate, 120 in 4/4 and one guitar
/// track. Mirrors `NewProjectOptions` in the UI.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewProject {
    pub sample_rate: Option<f64>,
    pub tempo: Option<Tempo>,
    pub start: Start,
}

/// The sample rates a project may be made at.
pub const SAMPLE_RATES: [f64; 4] = [44_100.0, 48_000.0, 88_200.0, 96_000.0];

pub struct Projects {
    root: PathBuf,
}

impl Projects {
    /// The projects folder: `Cantina` under the user's music folder, or
    /// under the app's data folder when there is no music folder.
    pub fn locate(app: &AppHandle) -> Self {
        let root = app
            .path()
            .audio_dir()
            .or_else(|_| app.path().app_data_dir())
            .map(|dir| dir.join("Cantina"))
            .unwrap_or_else(|_| PathBuf::from("Cantina"));
        Self { root }
    }

    #[cfg(test)]
    fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dir_for(&self, name: &str) -> PathBuf {
        cantina_project::dir_for(&self.root, name)
    }

    /// Where the user's own drum kits go: `Kits` next to the projects.
    pub fn kits_dir(&self) -> PathBuf {
        self.root.join("Kits")
    }

    /// Where the user's cabinet impulse responses go.
    pub fn irs_dir(&self) -> PathBuf {
        self.root.join("IRs")
    }

    /// Where the user's designed pedals go: the library.
    pub fn pedals_dir(&self) -> PathBuf {
        self.root.join("Pedals")
    }

    /// Where pattern files go: `Patterns` next to the projects.
    pub fn patterns_dir(&self) -> PathBuf {
        self.root.join("Patterns")
    }

    /// Where the user's rigs go: `Rigs` next to the projects.
    pub fn rigs_dir(&self) -> PathBuf {
        self.root.join("Rigs")
    }

    /// The user's jams.
    pub fn jams_dir(&self) -> PathBuf {
        self.root.join("Jams")
    }

    /// The user's exercises.
    pub fn exercises_dir(&self) -> PathBuf {
        self.root.join("Exercises")
    }

    pub fn list(&self) -> Result<Vec<String>> {
        cantina_project::list(&self.root)
            .with_context(|| format!("listing projects in {}", self.root.display()))
    }

    /// A project with one track and the stock chain, not yet on disk.
    pub fn untitled(sample_rate: f64) -> Project {
        Self::fresh(UNTITLED, sample_rate)
    }

    fn fresh(name: &str, sample_rate: f64) -> Project {
        let mut project = Project::new(name, sample_rate);
        project.tracks.push(stock_track(TrackId(1), "Track 1", 2));
        project
    }

    /// A project as the start dialog asks for it, not yet on disk: its
    /// rate, tempo and tracks. `kit` is the drum track's kit, needed only
    /// for [`Start::GuitarDrums`].
    pub fn shaped(
        name: &str,
        sample_rate: f64,
        options: &NewProject,
        kit: Option<&str>,
    ) -> Result<Project> {
        let sample_rate = options.sample_rate.unwrap_or(sample_rate);
        if !SAMPLE_RATES.contains(&sample_rate) {
            bail!("{sample_rate} Hz is not a sample rate Cantina records at");
        }
        let mut project = Project::new(name, sample_rate);
        if let Some(tempo) = &options.tempo {
            if !(20.0..=300.0).contains(&tempo.bpm) {
                bail!("a tempo is between 20 and 300 BPM");
            }
            if !(1..=16).contains(&tempo.numerator) || ![2, 4, 8, 16].contains(&tempo.denominator) {
                bail!(
                    "{}/{} is not a time signature Cantina keeps",
                    tempo.numerator,
                    tempo.denominator
                );
            }
            project.tempo = tempo.clone();
        }
        match options.start {
            Start::Empty => {}
            Start::Guitar => project.tracks.push(stock_track(TrackId(1), "Guitar", 2)),
            Start::GuitarBass => {
                project.tracks.push(stock_track(TrackId(1), "Guitar", 2));
                project.tracks.push(Track::new(TrackId(4), "Bass"));
            }
            Start::GuitarDrums => {
                let kit = kit.context("no drum kits are installed")?;
                project.tracks.push(stock_track(TrackId(1), "Guitar", 2));
                project.tracks.push(Track::drums(TrackId(4), "Drums", kit));
            }
        }
        Ok(project)
    }

    /// Creates and saves a new project. The name becomes a directory, so
    /// it cannot be empty, contain a slash, or already be in use.
    pub fn create(&self, name: &str, sample_rate: f64) -> Result<(Project, PathBuf)> {
        let (name, dir) = self.free_dir(name)?;
        let project = Self::fresh(&name, sample_rate);
        cantina_project::save(&project, &dir)?;
        Ok((project, dir))
    }

    /// Creates and saves a new project shaped by the start dialog's
    /// options, all at once: it opens with nothing to undo.
    pub fn create_with(
        &self,
        name: &str,
        sample_rate: f64,
        options: &NewProject,
        kit: Option<&str>,
    ) -> Result<(Project, PathBuf)> {
        let (name, dir) = self.free_dir(name)?;
        let project = Self::shaped(&name, sample_rate, options, kit)?;
        cantina_project::save(&project, &dir)?;
        Ok((project, dir))
    }

    /// When a project's file was last written, in milliseconds since the
    /// epoch; none when it is not there, moved or deleted.
    pub fn saved_at(&self, name: &str) -> Option<u64> {
        let file = self.dir_for(name).join(cantina_project::PROJECT_FILE);
        let modified = std::fs::metadata(file).ok()?.modified().ok()?;
        let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(since.as_millis() as u64)
    }

    /// The folder a project called `name` would have: the checked name
    /// and a folder not yet in use, for renaming and duplicating.
    pub fn free_dir(&self, name: &str) -> Result<(String, PathBuf)> {
        let name = check_name(name)?;
        let dir = self.dir_for(&name);
        if dir.exists() {
            bail!("there is already a project called {name}");
        }
        Ok((name, dir))
    }

    /// Moves a project's folder to the Trash, where Finder can put it
    /// back. Nothing in it is changed.
    pub fn trash(&self, name: &str) -> Result<()> {
        let name = check_name(name)?;
        let dir = self.dir_for(&name);
        if !dir.join(cantina_project::PROJECT_FILE).is_file() {
            bail!("there is no project called {name}");
        }
        move_to_trash(&dir)
    }

    /// The projects folder itself, which also holds the library folders.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(&self, name: &str) -> Result<(Project, PathBuf)> {
        let name = check_name(name)?;
        let dir = self.dir_for(&name);
        let project = cantina_project::load(&dir)?;
        Ok((project, dir))
    }

    /// The project to show on launch: the last one open, else `Untitled`,
    /// created if it does not exist yet.
    pub fn open_last_or_untitled(
        &self,
        last: Option<&str>,
        sample_rate: f64,
    ) -> Result<(Project, PathBuf)> {
        if let Some(name) = last {
            if self
                .dir_for(name)
                .join(cantina_project::PROJECT_FILE)
                .is_file()
            {
                return self.open(name);
            }
        }
        if self
            .dir_for(UNTITLED)
            .join(cantina_project::PROJECT_FILE)
            .is_file()
        {
            self.open(UNTITLED)
        } else {
            self.create(UNTITLED, sample_rate)
        }
    }
}

/// An audio track with the stock chain, its pedals numbered from `first`.
fn stock_track(id: TrackId, name: &str, first: u64) -> Track {
    let mut track = Track::new(id, name);
    for (i, (spec, values)) in DEFAULT_CHAIN.iter().enumerate() {
        track.chain.push(Pedal {
            id: PedalId(first + i as u64),
            spec: spec.to_string(),
            values: stock_values(spec, values),
            bypassed: false,
        });
    }
    track
}

/// A value for every parameter of the stock pedal `spec`: the given
/// ones first, the pedal's defaults for the rest, so a knob added to a
/// stock pedal later is still there to turn in a new project.
fn stock_values(spec: &str, given: &[f32]) -> Vec<f32> {
    match cantina_dsp::stock(spec) {
        Some(pedal) => with_defaults(given, pedal.params()),
        None => given.to_vec(),
    }
}

/// One value per parameter in `params`: the stored ones first, each
/// missing one at its parameter's default.
pub fn with_defaults(stored: &[f32], params: &[cantina_dsp::ParamInfo]) -> Vec<f32> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| stored.get(i).copied().unwrap_or(p.default))
        .collect()
}

/// Moves a file or folder to the user's Trash through the Finder's own
/// file manager.
fn move_to_trash(path: &Path) -> Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| {
            anyhow::anyhow!(
                "moving {} to the Trash: {}",
                path.display(),
                e.localizedDescription()
            )
        })
}

/// Copies a project folder: everything in it but the journal, whose
/// unsaved edits belong to the original.
pub fn copy_project_dir(from: &Path, to: &Path) -> Result<()> {
    copy_dir(from, to, true)
}

fn copy_dir(from: &Path, to: &Path, top: bool) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if top && name == cantina_project::JOURNAL_FILE {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if entry.file_type()?.is_dir() {
            copy_dir(&src, &dst, false)?;
        } else {
            std::fs::copy(&src, &dst)
                .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
        }
    }
    Ok(())
}

/// A project name that is safe as a directory name.
fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("a project needs a name");
    }
    if name.contains(['/', '\\', ':']) || name.starts_with('.') {
        bail!("a project name cannot contain / \\ or : or start with a dot");
    }
    if Path::new(name).components().count() != 1 {
        bail!("{name:?} is not a plain name");
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_projects_stock_pedals_store_every_knob() {
        let project = Projects::fresh("Song", 48_000.0);
        let chain = &project.tracks[0].chain;
        assert_eq!(chain.len(), DEFAULT_CHAIN.len());
        for (pedal, (spec, given)) in chain.iter().zip(DEFAULT_CHAIN) {
            let stock = cantina_dsp::stock(spec).unwrap();
            assert_eq!(pedal.values.len(), stock.params().len(), "{spec}");
            assert_eq!(&pedal.values[..given.len()], *given, "{spec}");
        }
    }

    #[test]
    fn missing_values_take_their_defaults() {
        let overdrive = cantina_dsp::stock("overdrive").unwrap();
        let params = overdrive.params();
        assert!(params.len() > 3);
        let filled = with_defaults(&[20.0, 3000.0, -6.0], params);
        assert_eq!(filled.len(), params.len());
        assert_eq!(&filled[..3], &[20.0, 3000.0, -6.0]);
        for (value, p) in filled[3..].iter().zip(&params[3..]) {
            assert_eq!(*value, p.default, "{}", p.name);
        }
        // Never more values than parameters.
        assert_eq!(with_defaults(&[1.0, 2.0], &params[..1]), vec![1.0]);
    }

    #[test]
    fn a_copy_leaves_the_journal_behind() {
        let root = std::env::temp_dir().join(format!("cantina-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let from = root.join("A.cantina");
        std::fs::create_dir_all(from.join("audio")).unwrap();
        std::fs::write(from.join("project.json"), "{}").unwrap();
        std::fs::write(from.join("audio/take-1.wav"), "riff").unwrap();
        std::fs::write(from.join(cantina_project::JOURNAL_FILE), "edits").unwrap();
        let to = root.join("B.cantina");
        copy_project_dir(&from, &to).unwrap();
        assert!(to.join("project.json").is_file());
        assert_eq!(
            std::fs::read_to_string(to.join("audio/take-1.wav")).unwrap(),
            "riff"
        );
        assert!(!to.join(cantina_project::JOURNAL_FILE).exists());
        assert!(from.join(cantina_project::JOURNAL_FILE).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn free_dir_refuses_a_name_in_use() {
        let root = std::env::temp_dir().join(format!("cantina-free-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = Projects::at(root.clone());
        projects.create("Song", 48_000.0).unwrap();
        assert!(projects.free_dir("Song").is_err());
        assert!(projects.free_dir("a/b").is_err());
        let (name, dir) = projects.free_dir(" Song 2 ").unwrap();
        assert_eq!(name, "Song 2");
        assert_eq!(dir, projects.dir_for("Song 2"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_a_project_can_go_to_the_trash() {
        // The move itself is the Finder's; this checks what is refused
        // before it, without filling the Trash.
        let root = std::env::temp_dir().join(format!("cantina-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = Projects::at(root.clone());
        projects.create("Song", 48_000.0).unwrap();
        assert!(projects.trash("Nothing here").is_err());
        assert!(projects.trash("../Song").is_err());
        assert!(projects.trash("").is_err());
        // A folder without a project file is not a project.
        std::fs::create_dir_all(projects.dir_for("Empty")).unwrap();
        assert!(projects.trash("Empty").is_err());
        assert!(projects.dir_for("Song").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_new_project_takes_its_shape_at_once() {
        let root = std::env::temp_dir().join(format!("cantina-shaped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = Projects::at(root.clone());
        let options = NewProject {
            sample_rate: Some(44_100.0),
            tempo: Some(Tempo {
                bpm: 92.0,
                numerator: 6,
                denominator: 8,
            }),
            start: Start::GuitarDrums,
        };
        let (song, dir) = projects
            .create_with(" Riff ", 48_000.0, &options, Some("acoustic"))
            .unwrap();
        assert_eq!(song.name, "Riff");
        assert_eq!(song.sample_rate, 44_100.0);
        assert_eq!(song.tempo.numerator, 6);
        let names: Vec<_> = song.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Guitar", "Drums"]);
        assert_eq!(song.tracks[1].kit.as_deref(), Some("acoustic"));
        assert_eq!(song.revision, 0, "nothing to undo or journal");
        assert_eq!(cantina_project::load(&dir).unwrap(), song);
        // Ids never collide: the drum track's id is past the pedals'.
        let mut ids: Vec<u64> = song.tracks.iter().map(|t| t.id.0).collect();
        ids.extend(
            song.tracks
                .iter()
                .flat_map(|t| t.chain.iter().map(|p| p.id.0)),
        );
        let len = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), len);
        assert!(projects.saved_at("Riff").is_some());
        assert!(projects.saved_at("Missing").is_none());

        let empty = Projects::shaped(
            "E",
            48_000.0,
            &NewProject {
                start: Start::Empty,
                ..NewProject::default()
            },
            None,
        )
        .unwrap();
        assert!(empty.tracks.is_empty());
        assert_eq!(empty.sample_rate, 48_000.0);
        let both = NewProject {
            start: Start::GuitarBass,
            ..NewProject::default()
        };
        let both = Projects::shaped("B", 48_000.0, &both, None).unwrap();
        assert_eq!(both.tracks.len(), 2);
        assert!(both.tracks[1].chain.is_empty() && both.tracks[1].kit.is_none());
        // A drum track needs a kit; odd rates and meters are refused.
        assert!(Projects::shaped("D", 48_000.0, &options, None).is_err());
        let odd = NewProject {
            sample_rate: Some(22_050.0),
            ..NewProject::default()
        };
        assert!(Projects::shaped("O", 48_000.0, &odd, None).is_err());
        let odd = NewProject {
            tempo: Some(Tempo {
                bpm: 120.0,
                numerator: 4,
                denominator: 3,
            }),
            ..NewProject::default()
        };
        assert!(Projects::shaped("O", 48_000.0, &odd, None).is_err());
        assert!(projects
            .create_with("Riff", 48_000.0, &NewProject::default(), None)
            .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(check_name("  My Song ").unwrap(), "My Song");
        for bad in ["", "a/b", "..", ".hidden", "c:d"] {
            assert!(check_name(bad).is_err(), "{bad:?} passed");
        }
    }

    #[test]
    fn create_open_and_fall_back() {
        let root = std::env::temp_dir().join(format!("cantina-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = Projects::at(root.clone());
        assert!(projects.list().unwrap().is_empty());

        let (untitled, dir) = projects.open_last_or_untitled(None, 48_000.0).unwrap();
        assert_eq!(untitled.name, UNTITLED);
        assert!(dir.join("project.json").is_file());
        assert_eq!(untitled.tracks[0].chain.len(), 2);

        let (song, _) = projects.create("Song", 44_100.0).unwrap();
        assert_eq!(song.sample_rate, 44_100.0);
        assert!(projects.create("Song", 44_100.0).is_err());
        assert_eq!(projects.list().unwrap(), vec!["Song", UNTITLED]);

        let (again, _) = projects
            .open_last_or_untitled(Some("Song"), 48_000.0)
            .unwrap();
        assert_eq!(again, song);
        let (fallback, _) = projects
            .open_last_or_untitled(Some("Missing"), 48_000.0)
            .unwrap();
        assert_eq!(fallback.name, UNTITLED);
        std::fs::remove_dir_all(root).unwrap();
    }
}
