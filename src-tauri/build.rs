//! Generates the Tauri context the shell embeds, and the docs pages the
//! assistant can read (`assistant/docs.rs`).

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    embed_docs();
    tauri_build::build()
}

/// Writes `docs_pages.rs` into `OUT_DIR`: every guide and reference page
/// by its slug, included as text, so the assistant's answers can quote
/// and link the docs of the build it is in.
fn embed_docs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/src/content/docs");
    let mut pages = Vec::new();
    for section in ["guides", "reference"] {
        let dir = root.join(section);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let stem = name
                .strip_suffix(".mdx")
                .or_else(|| name.strip_suffix(".md"));
            if let Some(stem) = stem {
                println!("cargo:rerun-if-changed={}", path.display());
                pages.push((
                    format!("{section}/{stem}"),
                    path.canonicalize().unwrap_or(path),
                ));
            }
        }
    }
    pages.sort();
    let mut out = String::from("/// Every guide and reference page: its slug and its Markdown.\npub static PAGES: &[(&str, &str)] = &[\n");
    for (slug, path) in &pages {
        let _ = writeln!(
            out,
            "    ({slug:?}, include_str!({:?})),",
            path.display().to_string()
        );
    }
    out.push_str("];\n");
    let dest =
        Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("docs_pages.rs");
    std::fs::write(dest, out).expect("writing docs_pages.rs");
}
