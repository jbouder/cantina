//! Where projects live and how they are opened.
//!
//! Projects are directories under one folder, `~/Music/Cantina/` by
//! default, and are referred to by name: `Song` is `~/Music/Cantina/Song.cantina/`.
//! A new project starts with one track carrying the stock chain, so the
//! first launch makes a sound.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_project::{Pedal, PedalId, Project, Track, TrackId};
use tauri::{AppHandle, Manager};

/// The name a project gets when nothing else is asked for.
pub const UNTITLED: &str = "Untitled";

/// The pedals a new project's first track starts with.
const DEFAULT_CHAIN: &[(&str, &[f32])] = &[("gain", &[0.0]), ("overdrive", &[20.0, 3000.0, -6.0])];

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
        let mut track = Track::new(TrackId(1), "Track 1");
        for (i, (spec, values)) in DEFAULT_CHAIN.iter().enumerate() {
            track.chain.push(Pedal {
                id: PedalId(2 + i as u64),
                spec: spec.to_string(),
                values: values.to_vec(),
            });
        }
        project.tracks.push(track);
        project
    }

    /// Creates and saves a new project. The name becomes a directory, so
    /// it cannot be empty, contain a slash, or already be in use.
    pub fn create(&self, name: &str, sample_rate: f64) -> Result<(Project, PathBuf)> {
        let name = check_name(name)?;
        let dir = self.dir_for(&name);
        if dir.exists() {
            bail!("there is already a project called {name}");
        }
        let project = Self::fresh(&name, sample_rate);
        cantina_project::save(&project, &dir)?;
        Ok((project, dir))
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
