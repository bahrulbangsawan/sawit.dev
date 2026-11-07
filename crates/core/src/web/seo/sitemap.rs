//! `sitemap.xml` read in Rust: a URL set or a sitemap index, each entry's `<loc>` and
//! `<lastmod>`, and `llms.txt`'s Markdown links.

use std::sync::LazyLock;

use regex::Regex;

/// A sitemap's entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sitemap {
    /// A sitemap index: its entries are other sitemaps.
    pub index: bool,
    /// `(loc, lastmod)` per entry.
    pub entries: Vec<(String, Option<String>)>,
}

static ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<(url|sitemap)\b[^>]*>(.*?)</(?:url|sitemap)>").expect("regex")
});
static LOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<loc>\s*(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?\s*</loc>").expect("regex")
});
static LASTMOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<lastmod>\s*(.*?)\s*</lastmod>").expect("regex"));

impl Sitemap {
    /// `None` when `xml` isn't a sitemap (no `<urlset>` or `<sitemapindex>`).
    pub fn parse(xml: &str) -> Option<Sitemap> {
        let lower = xml[..xml.len().min(4096)].to_ascii_lowercase();
        let index = lower.contains("<sitemapindex");
        if !index && !lower.contains("<urlset") {
            return None;
        }
        let entries = ENTRY
            .captures_iter(xml)
            .filter_map(|c| {
                let body = c.get(2)?.as_str();
                let loc = LOC
                    .captures(body)?
                    .get(1)?
                    .as_str()
                    .trim()
                    .replace("&amp;", "&");
                let lastmod = LASTMOD
                    .captures(body)
                    .and_then(|m| m.get(1))
                    .map(|m| m.as_str().to_string());
                Some((loc, lastmod))
            })
            .collect();
        Some(Sitemap { index, entries })
    }

    /// The share of entries with a `<lastmod>`, 0 to 1.
    pub fn lastmod_share(&self) -> f64 {
        if self.entries.is_empty() {
            return 0.;
        }
        self.entries.iter().filter(|(_, l)| l.is_some()).count() as f64 / self.entries.len() as f64
    }
}

static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)\s]+)(?:\s+[^)]*)?\)").expect("regex"));

/// The Markdown links in `text`, `(label, target)`.
pub fn markdown_links(text: &str) -> Vec<(String, String)> {
    MD_LINK
        .captures_iter(text)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

/// How `llms.txt` is laid out: an `# H1` title first, a `>` summary, `##` sections and links.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LlmsShape {
    pub h1: bool,
    pub summary: bool,
    pub sections: usize,
    pub links: usize,
    pub lines: usize,
}

pub fn llms_shape(text: &str) -> LlmsShape {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    LlmsShape {
        h1: first.starts_with("# "),
        summary: text.lines().any(|l| l.trim_start().starts_with("> ")),
        sections: text.lines().filter(|l| l.starts_with("## ")).count(),
        links: markdown_links(text).len(),
        lines: text.lines().count(),
    }
}
