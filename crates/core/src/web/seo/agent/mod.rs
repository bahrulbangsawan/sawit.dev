//! The agent-readiness checks: can agents find the site, read it, use it and pay it. Each one is
//! answered by an HTTP request, a DNS lookup or the page itself, here in Rust, on local and
//! production addresses alike. The checks that need outside data (search results, directories)
//! are left to the external scan.

use super::site::Site;
use super::{CheckResult, Evidence};
use crate::web::Fetched;

mod access;
mod discovery;
mod payments;
mod usability;

use access::access;
use discovery::discovery;
use payments::payments;
use usability::usability;

trait With {
    fn with(self, e: Evidence) -> Self;
    fn with_all(self, e: impl IntoIterator<Item = Evidence>) -> Self;
}

impl With for CheckResult {
    fn with(mut self, e: Evidence) -> Self {
        self.evidence.push(e);
        self
    }
    fn with_all(mut self, e: impl IntoIterator<Item = Evidence>) -> Self {
        self.evidence.extend(e);
        self
    }
}

/// Agents whose answers are compared, `(user agent string, robots token)`.
const AGENTS: &[(&str, &str)] = &[
    (
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ChatGPT-User/1.0; +https://openai.com/bot)",
        "chatgpt-user",
    ),
    (
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; Claude-User/1.0; +Claude-User@anthropic.com)",
        "claude-user",
    ),
    (
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; PerplexityBot/1.0; +https://perplexity.ai/perplexitybot)",
        "perplexitybot",
    ),
    (
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; OAI-SearchBot/1.0; +https://openai.com/searchbot)",
        "oai-searchbot",
    ),
];

/// Whether `got` is Markdown: served as text/markdown, or plain text that reads like it.
fn is_markdown(site: &Site, got: &Fetched) -> bool {
    if !got.ok() {
        return false;
    }
    let mime = got.mime();
    if mime == "text/markdown" || mime == "text/x-markdown" {
        return true;
    }
    site.real_file(got) && (mime.starts_with("text/plain") || mime.is_empty()) && {
        let t = got.text();
        t.lines().any(|l| l.starts_with("# ")) || t.contains("](")
    }
}

pub(super) async fn run(site: &Site) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let (discovery, access, usability, payments) = futures::join!(
        discovery(site),
        access(site),
        usability(site),
        payments(site)
    );
    out.extend(discovery);
    out.extend(access);
    out.extend(usability);
    out.extend(payments);
    out
}

/// The target of a `Link` header with `rel`.
pub(super) fn link_header(got: &Fetched, rel: &str) -> Option<String> {
    links_in_header(got)
        .into_iter()
        .find(|(_, rels, _)| rels.iter().any(|r| r == rel))
        .map(|(url, _, _)| url)
}

/// Every `Link` header entry: `(target, rels, type)`.
pub(super) fn links_in_header(got: &Fetched) -> Vec<(String, Vec<String>, Option<String>)> {
    let mut out = Vec::new();
    for header in got.headers_named("link") {
        for part in header.split(',') {
            let Some((url, params)) = part.trim().split_once('>') else {
                continue;
            };
            let url = url.trim().trim_start_matches('<').to_string();
            let mut rels = Vec::new();
            let mut kind = None;
            for p in params.split(';') {
                let Some((k, v)) = p.trim().split_once('=') else {
                    continue;
                };
                let v = v.trim().trim_matches('"');
                match k.trim().to_ascii_lowercase().as_str() {
                    "rel" => rels.extend(v.split_whitespace().map(|r| r.to_ascii_lowercase())),
                    "type" => kind = Some(v.to_ascii_lowercase()),
                    _ => {}
                }
            }
            let url = crate::web::Url::parse(&got.url)
                .and_then(|b| b.join(&url))
                .map(|u| u.to_string())
                .unwrap_or(url);
            out.push((url, rels, kind));
        }
    }
    out
}
