//! The docs site's guides and reference pages, compiled into the shell
//! by `build.rs`, so the assistant can search and read the docs of the
//! very build it runs in and link the player to the right page.

use serde_json::{json, Value};

include!(concat!(env!("OUT_DIR"), "/docs_pages.rs"));

/// The most of a page `read` returns, in characters.
const MAX_PAGE: usize = 16_000;

/// One page, split from its front matter.
pub struct Page {
    pub slug: &'static str,
    pub title: String,
    pub description: String,
    pub body: &'static str,
}

/// Every page.
pub fn pages() -> impl Iterator<Item = Page> {
    PAGES.iter().map(|(slug, text)| parse(slug, text))
}

fn parse(slug: &'static str, text: &'static str) -> Page {
    let (front, body) = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .unwrap_or(("", text));
    let field = |name: &str| {
        front
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name}:")))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_default()
    };
    Page {
        slug,
        title: field("title"),
        description: field("description"),
        body: body.trim_start(),
    }
}

/// The page's address on the docs site.
pub fn url(slug: &str) -> String {
    format!("{}{slug}/", crate::menu::DOCS)
}

/// A heading's anchor, as the docs site makes it.
pub fn anchor(heading: &str) -> String {
    let mut out = String::new();
    for c in heading.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if (c == ' ' || c == '-') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The pages that best match `query`, best first: each with its title,
/// address and the passage that matched.
pub fn search(query: &str, limit: usize) -> Vec<Value> {
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, Page, String)> = pages()
        .filter_map(|page| {
            let body = page.body.to_lowercase();
            let head = format!("{} {}", page.title, page.description).to_lowercase();
            let score: usize = terms
                .iter()
                .map(|t| body.matches(t.as_str()).count() + 5 * head.matches(t.as_str()).count())
                .sum();
            (score > 0).then(|| {
                let passage = best_passage(page.body, &terms);
                (score, page, passage)
            })
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.slug.cmp(b.1.slug)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, page, passage)| {
            json!({
                "page": page.slug,
                "title": page.title,
                "url": url(page.slug),
                "passage": passage,
            })
        })
        .collect()
}

/// The paragraph with the most of the terms in it, with the heading it
/// is under, cut to a few hundred characters.
fn best_passage(body: &str, terms: &[String]) -> String {
    let mut heading = "";
    let mut best = (0, "", "");
    for para in body.split("\n\n") {
        let para = para.trim();
        if let Some(h) = para
            .strip_prefix("## ")
            .or_else(|| para.strip_prefix("### "))
        {
            heading = h.lines().next().unwrap_or(h);
            continue;
        }
        let lower = para.to_lowercase();
        let score = terms.iter().filter(|t| lower.contains(t.as_str())).count();
        if score > best.0 {
            best = (score, heading, para);
        }
    }
    let text: String = best.2.chars().take(400).collect();
    if best.1.is_empty() {
        text
    } else {
        format!("[{}] {text}", best.1)
    }
}

/// A whole page by its slug, with the anchors of its sections.
pub fn read(slug: &str) -> Option<Value> {
    let slug = slug.trim().trim_matches('/');
    let slug = slug
        .strip_prefix(crate::menu::DOCS.trim_end_matches('/'))
        .unwrap_or(slug)
        .trim_matches('/');
    let page = pages().find(|p| p.slug == slug)?;
    let sections: Vec<Value> = page
        .body
        .lines()
        .filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("### ")))
        .map(|h| json!({ "heading": h, "url": format!("{}#{}", url(page.slug), anchor(h)) }))
        .collect();
    let mut text: String = page.body.chars().take(MAX_PAGE).collect();
    if text.len() < page.body.len() {
        text.push_str("\n\n[The rest of the page is cut.]");
    }
    Some(json!({
        "page": page.slug,
        "title": page.title,
        "url": url(page.slug),
        "sections": sections,
        "text": text,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_guide_and_reference_page_is_in_with_a_title() {
        let slugs: Vec<&str> = pages().map(|p| p.slug).collect();
        for slug in [
            "guides/troubleshooting",
            "guides/latency",
            "guides/pedals",
            "reference/cli",
            "guides/assistant",
        ] {
            assert!(slugs.contains(&slug), "{slug}");
        }
        assert!(
            pages().all(|p| !p.title.is_empty()),
            "every page has a title"
        );
    }

    #[test]
    fn a_search_finds_the_page_and_the_passage() {
        let found = search("crackles dropouts buffer size", 3);
        assert!(!found.is_empty());
        let pages: Vec<&str> = found.iter().map(|f| f["page"].as_str().unwrap()).collect();
        assert!(
            pages.contains(&"guides/troubleshooting") || pages.contains(&"guides/latency"),
            "{pages:?}"
        );
        assert!(found[0]["url"].as_str().unwrap().starts_with("https://"));
        assert!(search("a an", 3).is_empty());
    }

    #[test]
    fn a_page_reads_with_its_section_links() {
        let page = read("guides/latency").unwrap();
        assert_eq!(page["url"], url("guides/latency"));
        assert!(page["sections"][0]["url"]
            .as_str()
            .unwrap()
            .contains("guides/latency/#"));
        assert!(
            read(&url("guides/latency")).is_some(),
            "a full address works too"
        );
        assert!(read("guides/nothing").is_none());
        assert_eq!(anchor("Pick your interface"), "pick-your-interface");
        assert_eq!(anchor("What is sent, and when"), "what-is-sent-and-when");
    }
}
