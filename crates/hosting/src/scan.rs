//! Finding installed plugins without letting one of them take the app
//! down.
//!
//! Listing what is installed is cheap and runs no plugin code: Audio
//! Units come from the component registry, CLAP bundles from the standard
//! folders. Learning anything more (channel layout, whether there is an
//! editor, the plugins inside a CLAP bundle) means loading and
//! instantiating the plugin, and some plugin always crashes doing that.
//! So the [`Scanner`] hands a list of [`Source`]s to a child process over
//! its stdin and reads results back line by line. Every source is
//! bracketed by a `begin` and an `end` line, so when the child dies the
//! parent knows which source killed it, records that as a [`Failure`],
//! and starts a new child on the rest. A child that goes quiet for longer
//! than the timeout is killed and its source recorded the same way.
//!
//! The child is this crate's `child_main`. The shell re-invokes its own
//! binary with [`CHILD_FLAG`] so nothing extra has to be bundled; the
//! tests use the `cantina-scan` binary.
//!
//! Results are a [`PluginList`], cached on disk. Each source carries a
//! fingerprint (the component's version, or a bundle's modification time
//! and size), so a rescan only examines sources that are new or changed
//! and drops the ones that went away.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::PluginGui;
use clack_host::entry::PluginEntry as ClapEntry;
use clack_host::host::HostInfo;
use clack_host::plugin::PluginInstance;
use serde::{Deserialize, Serialize};

use crate::au::{self, ComponentId};
use crate::clap::{self, CantinaHost, Shared};

/// The argument the shell's binary takes to run as the scanner child.
pub const CHILD_FLAG: &str = "--scan-plugins";

/// Bumped when the cache's shape changes; an older cache is ignored.
pub const CACHE_VERSION: u32 = 1;

/// How long the child may go without printing a line before it is
/// presumed hung.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Every protocol line the child prints starts with this, so a plugin
/// that writes to stdout while loading cannot be mistaken for a result.
const MARK: &str = "@cantina ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Au,
    Clap,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Au => "Audio Unit",
            Format::Clap => "CLAP",
        })
    }
}

/// One thing the child loads as a unit: an Audio Unit component or a
/// CLAP bundle. Its fields are its fingerprint: a source equal to the
/// cached one has not changed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum Source {
    Au {
        /// `type:subtype:manufacturer`.
        id: String,
        name: String,
        version: u32,
    },
    Clap {
        path: PathBuf,
        /// Newest modification time inside the bundle, seconds since
        /// the epoch.
        modified: u64,
        /// Total size of the files inside the bundle.
        size: u64,
    },
}

impl Source {
    /// The key the cache stores the source under.
    pub fn key(&self) -> String {
        match self {
            Source::Au { id, .. } => format!("au:{id}"),
            Source::Clap { path, .. } => format!("clap:{}", path.display()),
        }
    }

    pub fn format(&self) -> Format {
        match self {
            Source::Au { .. } => Format::Au,
            Source::Clap { .. } => Format::Clap,
        }
    }

    /// What to call it before anything has loaded.
    pub fn display_name(&self) -> String {
        match self {
            Source::Au { name, .. } => name.clone(),
            Source::Clap { path, .. } => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
        }
    }
}

/// A plugin the scanner found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub format: Format,
    /// The spec the engine builds it from: `au:aufx:dely:appl` or
    /// `clap:<bundle path>#<plugin id>`.
    pub spec: String,
    /// Key of the [`Source`] it came from.
    pub source: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// Channels on the main input and output.
    pub inputs: u32,
    pub outputs: u32,
    /// Whether the plugin ships its own editor view.
    pub has_editor: bool,
}

/// A source the child could not examine, or died examining.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub format: Format,
    pub source: String,
    /// The plugin being examined when it failed, or the source's name.
    pub name: String,
    pub why: String,
}

/// Everything the scanner knows, as cached on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginList {
    pub version: u32,
    /// Seconds since the epoch of the last scan; zero before the first.
    pub scanned_at: u64,
    pub plugins: Vec<PluginEntry>,
    pub failed: Vec<Failure>,
    /// Every source examined, by key, with the fingerprint it had.
    pub sources: BTreeMap<String, Source>,
}

impl Default for PluginList {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            scanned_at: 0,
            plugins: Vec::new(),
            failed: Vec::new(),
            sources: BTreeMap::new(),
        }
    }
}

impl PluginList {
    /// Reads a cache. A missing file, or one from another cache version,
    /// is an empty list; a file that does not parse is an error.
    pub fn load(path: &Path) -> Result<PluginList> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let list: PluginList =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if list.version != CACHE_VERSION {
            return Ok(Self::default());
        }
        Ok(list)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
        std::fs::rename(&temp, path).with_context(|| format!("writing {}", path.display()))
    }

    /// True before any scan has run.
    pub fn is_unscanned(&self) -> bool {
        self.scanned_at == 0
    }

    /// Entries sorted by format, vendor and name, which is how a browser
    /// shows them.
    pub fn sorted(&self) -> Vec<&PluginEntry> {
        let mut entries: Vec<&PluginEntry> = self.plugins.iter().collect();
        entries.sort_by(|a, b| {
            (a.format, a.vendor.to_lowercase(), a.name.to_lowercase()).cmp(&(
                b.format,
                b.vendor.to_lowercase(),
                b.name.to_lowercase(),
            ))
        });
        entries
    }
}

// ----- discovery -----

/// Every source installed: effect Audio Units from the component
/// registry and CLAP bundles under the standard folders. Runs no plugin
/// code.
pub fn discover() -> Vec<Source> {
    let mut sources: Vec<Source> = au::list_effects()
        .into_iter()
        .map(|c| Source::Au {
            id: c.id.to_string(),
            name: c.name,
            version: c.version,
        })
        .collect();
    sources.extend(discover_clap(&clap::default_search_paths()));
    sources
}

/// CLAP bundles anywhere under `dirs`, with their fingerprints.
pub fn discover_clap(dirs: &[PathBuf]) -> Vec<Source> {
    let mut sources = Vec::new();
    for dir in dirs {
        let mut stack = vec![dir.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "clap") {
                    let (modified, size) = fingerprint(&path);
                    sources.push(Source::Clap {
                        path,
                        modified,
                        size,
                    });
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }
    sources.sort();
    sources
}

/// The newest modification time and the total size of everything under
/// `path`, or of the file itself.
fn fingerprint(path: &Path) -> (u64, u64) {
    fn seconds(time: SystemTime) -> u64 {
        time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }
    let mut newest = 0u64;
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if let Ok(modified) = meta.modified() {
            newest = newest.max(seconds(modified));
        }
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        } else {
            total += meta.len();
        }
    }
    (newest, total)
}

// ----- the child -----

/// One line from the child to the parent.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Line {
    /// About to load this source.
    Begin(String),
    /// About to instantiate this plugin within the current source.
    Examining(String),
    Plugin(PluginEntry),
    Failed(Failure),
    /// Done with this source.
    End(String),
}

/// Runs the scanner child: reads one JSON [`Source`] per line of stdin,
/// examines each and prints what it finds. Returns the exit code.
pub fn child_main() -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut emit = |line: &Line| {
        let json = serde_json::to_string(line).expect("protocol lines serialize");
        let _ = writeln!(out, "{MARK}{json}");
        let _ = out.flush();
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let source: Source = match serde_json::from_str(line) {
            Ok(source) => source,
            Err(e) => {
                eprintln!("scanner: unreadable source {line:?}: {e}");
                continue;
            }
        };
        let key = source.key();
        emit(&Line::Begin(key.clone()));
        let result = examine(&source, &mut |found| match found {
            Found::Examining(name) => emit(&Line::Examining(name)),
            Found::Plugin(plugin) => emit(&Line::Plugin(plugin)),
        });
        if let Err(e) = result {
            emit(&Line::Failed(Failure {
                format: source.format(),
                source: key.clone(),
                name: source.display_name(),
                why: format!("{e:#}"),
            }));
        }
        emit(&Line::End(key));
    }
    0
}

/// What [`examine`] reports as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// About to instantiate this plugin, so a crash can be pinned on it.
    Examining(String),
    Plugin(PluginEntry),
}

/// Loads a source in this process and reports the plugins in it, each
/// the moment it is described, so what came before a crash is not lost
/// with it. Runs plugin code: the scanner calls this in the child, and
/// nothing else should call it on a plugin it does not trust.
pub fn examine(source: &Source, report: &mut dyn FnMut(Found)) -> Result<()> {
    match source {
        Source::Au { id, name, version } => {
            report(Found::Examining(name.clone()));
            let component = ComponentId::parse(id)?;
            let info = au::describe_instance(component)?;
            let component_info = au::ComponentInfo {
                id: component,
                name: name.clone(),
                version: *version,
            };
            let (vendor, short_name) = component_info.vendor_and_name();
            report(Found::Plugin(PluginEntry {
                format: Format::Au,
                spec: format!("au:{id}"),
                source: source.key(),
                name: short_name,
                vendor,
                version: component_info.version_string(),
                inputs: info.inputs,
                outputs: info.outputs,
                has_editor: info.has_editor,
            }));
            Ok(())
        }
        Source::Clap { path, .. } => examine_clap(path, source, report),
    }
}

fn examine_clap(path: &Path, source: &Source, report: &mut dyn FnMut(Found)) -> Result<()> {
    // SAFETY: loading a plugin library runs its code; that is what the
    // child process is for.
    let entry = unsafe { ClapEntry::load(path) }
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("loading CLAP bundle {}", path.display()))?;
    let factory = entry
        .get_plugin_factory()
        .with_context(|| format!("{} has no plugin factory", path.display()))?;
    let descriptors: Vec<(String, String, String, String)> = factory
        .plugin_descriptors()
        .filter_map(|d| {
            Some((
                d.id()?.to_string_lossy().into_owned(),
                d.name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                d.vendor()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                d.version()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        })
        .collect();
    let host_info = HostInfo::new(
        "Cantina scanner",
        "Cantina",
        "https://github.com/jbouder/cantina",
        env!("CARGO_PKG_VERSION"),
    )?;
    for (id, name, vendor, version) in descriptors {
        report(Found::Examining(if name.is_empty() {
            id.clone()
        } else {
            name.clone()
        }));
        let c_id = std::ffi::CString::new(id.clone())?;
        let mut instance = PluginInstance::<CantinaHost>::new(
            |_| Shared::default(),
            |_| (),
            &entry,
            &c_id,
            &host_info,
        )
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("instantiating {name}"))?;
        let (inputs, outputs) = port_channels(&mut instance);
        let has_editor = instance
            .plugin_shared_handle()
            .get_extension::<PluginGui>()
            .is_some();
        report(Found::Plugin(PluginEntry {
            format: Format::Clap,
            spec: format!("clap:{}#{id}", path.display()),
            source: source.key(),
            name,
            vendor,
            version,
            inputs,
            outputs,
            has_editor,
        }));
    }
    Ok(())
}

/// Channels on the main input and output ports.
fn port_channels(instance: &mut PluginInstance<CantinaHost>) -> (u32, u32) {
    let Some(ports) = instance
        .plugin_shared_handle()
        .get_extension::<PluginAudioPorts>()
    else {
        return (0, 0);
    };
    let handle = instance.plugin_handle();
    let mut buffer = AudioPortInfoBuffer::new();
    let count = |is_input: bool, buffer: &mut AudioPortInfoBuffer| {
        ports
            .get(&handle, 0, is_input, buffer)
            .map_or(0, |info| info.channel_count)
    };
    let inputs = count(true, &mut buffer);
    let outputs = count(false, &mut buffer);
    (inputs, outputs)
}

// ----- the parent -----

/// Where a scan is, for a progress display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// Sources finished, including the ones reused from the cache.
    pub done: usize,
    pub total: usize,
    /// What the child is loading now.
    pub current: String,
}

/// What a scan did, beyond the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub list: PluginList,
    /// Sources the child examined.
    pub examined: usize,
    /// Sources taken from the cache unchanged.
    pub reused: usize,
    /// Child processes started.
    pub children: usize,
}

/// Runs the child process and collects a [`PluginList`].
pub struct Scanner {
    program: PathBuf,
    args: Vec<String>,
    pub timeout: Duration,
}

impl Scanner {
    /// A scanner that runs `program` with `args` as the child.
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Examines `sources`, reusing `previous` for the ones whose
    /// fingerprint has not changed unless `full` is set. Sources missing
    /// from `sources` are dropped. `progress` is called as sources
    /// finish.
    pub fn scan(
        &self,
        sources: Vec<Source>,
        previous: &PluginList,
        full: bool,
        mut progress: impl FnMut(&Progress),
    ) -> Result<Outcome> {
        let total = sources.len();
        let mut list = PluginList::default();
        let mut pending: Vec<Source> = Vec::new();
        let mut reused = 0;
        for source in sources {
            let key = source.key();
            let unchanged = !full && previous.sources.get(&key) == Some(&source);
            if unchanged {
                list.plugins
                    .extend(previous.plugins.iter().filter(|p| p.source == key).cloned());
                list.failed
                    .extend(previous.failed.iter().filter(|f| f.source == key).cloned());
                reused += 1;
            } else {
                pending.push(source.clone());
            }
            list.sources.insert(key, source);
        }
        progress(&Progress {
            done: reused,
            total,
            current: String::new(),
        });

        let examined = pending.len();
        let mut children = 0;
        while !pending.is_empty() {
            children += 1;
            let before = pending.len();
            self.run_child(
                &mut pending,
                &mut list,
                reused + examined - before,
                total,
                &mut progress,
            )?;
            if pending.len() == before {
                bail!(
                    "the scanner child made no progress on {}",
                    pending[0].display_name()
                );
            }
        }
        list.scanned_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        progress(&Progress {
            done: total,
            total,
            current: String::new(),
        });
        Ok(Outcome {
            list,
            examined,
            reused,
            children,
        })
    }

    /// Starts one child on `pending` and takes results until it finishes
    /// or dies. Sources it got through are removed from `pending`; the
    /// one it died on is recorded as failed and removed too.
    fn run_child(
        &self,
        pending: &mut Vec<Source>,
        list: &mut PluginList,
        done_before: usize,
        total: usize,
        progress: &mut impl FnMut(&Progress),
    ) -> Result<()> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("starting the scanner {}", self.program.display()))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        // Write every source up front; the child reads them as it goes.
        // The writer is its own thread so a child that never reads cannot
        // block us on a full pipe.
        let sources = pending.clone();
        let writer = thread::spawn(move || {
            for source in &sources {
                let json = serde_json::to_string(source).expect("sources serialize");
                if writeln!(stdin, "{json}").is_err() {
                    break;
                }
            }
        });
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(json) = line.strip_prefix(MARK) {
                    if let Ok(parsed) = serde_json::from_str::<Line>(json) {
                        if tx.send(parsed).is_err() {
                            break;
                        }
                    }
                }
                // Anything else is the plugin talking to itself.
            }
        });

        let mut current: Option<(String, String)> = None;
        let mut done = done_before;
        let outcome = loop {
            match rx.recv_timeout(self.timeout) {
                Ok(Line::Begin(key)) => {
                    let name = pending
                        .iter()
                        .find(|s| s.key() == key)
                        .map(|s| s.display_name())
                        .unwrap_or_else(|| key.clone());
                    progress(&Progress {
                        done,
                        total,
                        current: name.clone(),
                    });
                    current = Some((key, name));
                }
                Ok(Line::Examining(name)) => {
                    if let Some(c) = current.as_mut() {
                        c.1 = name;
                    }
                }
                Ok(Line::Plugin(plugin)) => list.plugins.push(plugin),
                Ok(Line::Failed(failure)) => list.failed.push(failure),
                Ok(Line::End(key)) => {
                    pending.retain(|s| s.key() != key);
                    current = None;
                    done += 1;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    break Err(format!(
                        "did not respond within {} s",
                        self.timeout.as_secs()
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        let status = child.wait().context("waiting for the scanner")?;
        let _ = reader.join();
        let _ = writer.join();

        let why = match outcome {
            Err(why) => Some(why),
            Ok(()) if status.success() => None,
            Ok(()) => Some(format!("crashed while loading ({status})")),
        };
        if let Some(why) = why {
            if let Some((key, name)) = current.take() {
                let format = pending
                    .iter()
                    .find(|s| s.key() == key)
                    .map_or(Format::Clap, |s| s.format());
                list.failed.push(Failure {
                    format,
                    source: key.clone(),
                    name,
                    why,
                });
                pending.retain(|s| s.key() != key);
            } else if !pending.is_empty() {
                // It died before saying what it was loading, or after
                // finishing: blame the next source so the scan still ends.
                let source = pending.remove(0);
                list.failed.push(Failure {
                    format: source.format(),
                    source: source.key(),
                    name: source.display_name(),
                    why,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trips_and_ignores_other_versions() {
        let dir = std::env::temp_dir().join(format!("cantina-scan-{}", std::process::id()));
        let path = dir.join("plugins.json");
        assert_eq!(PluginList::load(&path).unwrap(), PluginList::default());
        let mut list = PluginList {
            scanned_at: 42,
            ..Default::default()
        };
        let source = Source::Au {
            id: "aufx:dely:appl".into(),
            name: "Apple: AUDelay".into(),
            version: 0x00010203,
        };
        list.sources.insert(source.key(), source);
        list.plugins.push(PluginEntry {
            format: Format::Au,
            spec: "au:aufx:dely:appl".into(),
            source: "au:aufx:dely:appl".into(),
            name: "AUDelay".into(),
            vendor: "Apple".into(),
            version: "1.2.3".into(),
            inputs: 2,
            outputs: 2,
            has_editor: false,
        });
        list.save(&path).unwrap();
        assert_eq!(PluginList::load(&path).unwrap(), list);
        let mut old = list.clone();
        old.version = CACHE_VERSION + 1;
        old.save(&path).unwrap();
        assert!(PluginList::load(&path).unwrap().is_unscanned());
        std::fs::write(&path, "{").unwrap();
        assert!(PluginList::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sources_have_stable_keys_and_names() {
        let au = Source::Au {
            id: "aufx:dely:appl".into(),
            name: "Apple: AUDelay".into(),
            version: 1,
        };
        assert_eq!(au.key(), "au:aufx:dely:appl");
        assert_eq!(au.display_name(), "Apple: AUDelay");
        let clap = Source::Clap {
            path: "/Library/Audio/Plug-Ins/CLAP/Thing.clap".into(),
            modified: 1,
            size: 2,
        };
        assert_eq!(clap.key(), "clap:/Library/Audio/Plug-Ins/CLAP/Thing.clap");
        assert_eq!(clap.display_name(), "Thing");
        assert_eq!(clap.format(), Format::Clap);
    }

    #[test]
    fn sorted_groups_by_format_vendor_and_name() {
        let entry = |format, vendor: &str, name: &str| PluginEntry {
            format,
            spec: String::new(),
            source: String::new(),
            name: name.into(),
            vendor: vendor.into(),
            version: String::new(),
            inputs: 2,
            outputs: 2,
            has_editor: false,
        };
        let list = PluginList {
            plugins: vec![
                entry(Format::Clap, "Zed", "a"),
                entry(Format::Au, "apple", "Zeta"),
                entry(Format::Au, "Apple", "alpha"),
            ],
            ..Default::default()
        };
        let names: Vec<&str> = list.sorted().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Zeta", "a"]);
    }
}
