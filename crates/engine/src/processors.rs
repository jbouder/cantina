//! Builds processors from the short specs the examples and, later, the
//! project file use.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cantina_dsp::{stock, Processor, STOCK_NAMES};
use cantina_hosting::au::{self, AudioUnitProcessor};
use cantina_hosting::clap::{self, ClapProcessor};

/// Builds a processor from a spec, configured for `channels`:
///
/// - a stock name such as `overdrive`;
/// - `au:<identifier or name>` for an Audio Unit;
/// - `clap:<path or bundle name>[#<plugin id>]` for a CLAP plugin. A bare
///   name is looked up in the standard CLAP folders.
pub fn create(spec: &str, channels: usize) -> Result<Box<dyn Processor>> {
    if let Some(query) = spec.strip_prefix("au:") {
        let info = au::find(query)?;
        return Ok(Box::new(AudioUnitProcessor::new(info, channels)?));
    }
    if let Some(rest) = spec.strip_prefix("clap:") {
        let (location, plugin_id) = match rest.split_once('#') {
            Some((l, id)) => (l, Some(id)),
            None => (rest, None),
        };
        let path = resolve_clap(location)?;
        return Ok(Box::new(ClapProcessor::load(&path, plugin_id, channels)?));
    }
    stock(spec).with_context(|| {
        format!(
            "no processor {spec:?}; stock processors are {}, Audio Units are au:<name>, \
             CLAP plugins are clap:<bundle>",
            STOCK_NAMES.join(", ")
        )
    })
}

fn resolve_clap(location: &str) -> Result<PathBuf> {
    let direct = Path::new(location);
    if direct.exists() {
        return Ok(direct.to_path_buf());
    }
    let wanted = location.trim_end_matches(".clap").to_lowercase();
    let matches: Vec<PathBuf> = clap::installed_bundles()
        .into_iter()
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_lowercase() == wanted)
        })
        .collect();
    match matches.len() {
        0 => bail!(
            "no CLAP bundle {location:?}; looked in {}",
            clap::default_search_paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" and ")
        ),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!("more than one CLAP bundle is called {location:?}; give a path"),
    }
}
