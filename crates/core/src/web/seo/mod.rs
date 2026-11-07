//! The browser's SEO and AEO scan: everything search engines, link previews and answer engines
//! read from a page and its site, checked in Rust. The site's files are fetched here (never from
//! the page, so its CSP can't block them), the HTML read with `html5ever`, `robots.txt`, sitemaps
//! and JSON-LD parsed in Rust, image sizes read from their headers, and the score worked out here.
//! The page's rendered DOM, when the browser has it, stands in for the server's HTML for what
//! scripts add after load.
//!
//! Every check is in one table ([`catalog::CHECKS`]), keyed by id: a check never runs twice. The
//! report ([`Report`]) is what the browser's SEO panel draws and what `browser_seo` returns.

pub mod catalog;
mod checks;
pub mod robots;
mod site;
pub mod sitemap;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use catalog::{CHECKS, CheckDef};
pub use site::{Site, image_size};

/// What a check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    /// Doesn't apply to this site (no MCP server to probe, no commerce, no `hreflang`).
    Na,
    /// Not run: it needs data Sawit can't get itself, or it wasn't asked for.
    Skipped,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warning",
            Status::Fail => "fail",
            Status::Na => "n/a",
            Status::Skipped => "skipped",
        }
    }

    /// Whether it counts towards a score.
    pub fn scored(self) -> bool {
        matches!(self, Status::Pass | Status::Warn | Status::Fail)
    }
}

/// How much a check matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Required,
    Recommended,
    /// Earns points when it passes, never costs any.
    Bonus,
    /// Forward-looking: shown, left out of the scores.
    Emerging,
}

/// Which part of the report a check belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Search engines and link previews.
    Seo,
    /// Can agents find the site.
    Discovery,
    /// Can agents read it.
    Access,
    /// Can agents use it.
    Usability,
    /// Can agents pay it.
    Payments,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Seo => "Search and sharing",
            Layer::Discovery => "Discovery",
            Layer::Access => "Access",
            Layer::Usability => "Usability",
            Layer::Payments => "Payments",
        }
    }

    /// The layer's weight in the agent-readiness total (out of 100); 0 for SEO, scored apart.
    pub fn weight(self) -> f64 {
        match self {
            Layer::Seo => 0.,
            Layer::Discovery => 20.,
            Layer::Access => 30.,
            Layer::Usability => 40.,
            Layer::Payments => 10.,
        }
    }

    pub const AGENT: [Layer; 4] = [
        Layer::Discovery,
        Layer::Access,
        Layer::Usability,
        Layer::Payments,
    ];
}

/// What kind of site a scan is for: which checks apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanType {
    #[default]
    All,
    /// A content site: pages to read; API checks don't apply.
    Content,
    /// An API or app: the content-only checks don't apply.
    Api,
}

impl ScanType {
    pub fn parse(text: &str) -> Option<ScanType> {
        match text.trim().to_ascii_lowercase().as_str() {
            "all" | "" => Some(ScanType::All),
            "content" | "content_site" | "site" => Some(ScanType::Content),
            "api" | "app" | "api_or_app" => Some(ScanType::Api),
            _ => None,
        }
    }
}

/// What a check looked at: the address fetched, its status, a header or a snippet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub url: String,
    /// The HTTP status, 0 when nothing answered.
    pub status: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// One check's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub id: String,
    pub name: String,
    pub layer: Layer,
    pub tier: Tier,
    pub status: Status,
    pub score: f64,
    pub max: f64,
    /// What was found, in a sentence.
    pub details: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    /// How to fix it (empty when it passes).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub spec: String,
    /// Also listed with the search and sharing checks.
    pub seo: bool,
    /// The result came from the external scan, not from Sawit's own check.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external: bool,
}

/// What the page says about itself, for the panel's summary and the share previews.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageFacts {
    pub title: Option<String>,
    pub description: Option<String>,
    pub canonical: Option<String>,
    pub lang: Option<String>,
    pub robots: Option<String>,
    pub site_name: Option<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub og_type: Option<String>,
    pub og_url: Option<String>,
    pub twitter_card: Option<String>,
    pub twitter_title: Option<String>,
    pub twitter_description: Option<String>,
    pub twitter_image: Option<String>,
    /// The OG image as fetched: its pixel size and media type.
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub image_type: Option<String>,
    /// The icons the page names, `(rel, sizes, href)`.
    pub icons: Vec<(String, Option<String>, String)>,
    pub headings: Vec<(u8, String)>,
    pub hreflang: Vec<(String, String)>,
    /// JSON-LD `@type`s found.
    pub schema_types: Vec<String>,
    pub images: usize,
    pub images_without_alt: usize,
    pub links: usize,
    /// The share preview's host (`example.com`).
    pub host: String,
}

/// A layer's score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerScore {
    pub layer: Layer,
    pub name: String,
    pub score: f64,
    pub max: f64,
    /// 0 to 100.
    pub percent: u32,
    pub weight: f64,
}

/// The score an outside scanner gave, or why there is none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExternalScore {
    pub score: Option<u32>,
    pub grade: Option<String>,
    /// Why there is no external score (a local address, the setting off, the service down).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerScore>,
}

/// The whole scan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    pub local: bool,
    pub scan_type: ScanType,
    /// The page as rendered by the browser was read, not just the server's HTML.
    pub rendered: bool,
    pub facts: PageFacts,
    pub checks: Vec<CheckResult>,
    /// The search and sharing score, 0 to 100.
    pub seo_score: u32,
    /// Agent readiness, 0 to 100, from the layers' weighted scores.
    pub agent_score: u32,
    pub grade: String,
    /// The score for content sites: required checks 80 points, recommended 20, up to 5 more.
    pub essentials: u32,
    pub layers: Vec<LayerScore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalScore>,
    pub elapsed_ms: u64,
    /// Why the page couldn't be scanned at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Report {
    pub fn check(&self, id: &str) -> Option<&CheckResult> {
        self.checks.iter().find(|c| c.id == id)
    }

    /// How many checks have each status, among `seo` ones when `seo_only`.
    pub fn counts(&self, seo_only: bool) -> [usize; 3] {
        let mut out = [0; 3];
        for c in self.checks.iter().filter(|c| !seo_only || c.seo) {
            match c.status {
                Status::Pass => out[0] += 1,
                Status::Warn => out[1] += 1,
                Status::Fail => out[2] += 1,
                _ => {}
            }
        }
        out
    }
}

/// How a scan is run.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    pub scan_type: ScanType,
    /// Only these check ids (all when empty).
    pub checks: Vec<String>,
    /// Also ask the outside scanner (production addresses only).
    pub external: bool,
    /// The page as the browser rendered it (`document.documentElement.outerHTML`).
    pub rendered_html: Option<String>,
}

/// Scans `url`.
pub async fn scan(url: &str, options: ScanOptions) -> Report {
    checks::run(url, options).await
}

/// A prompt an agent can act on to fix `check` on `url`.
pub fn fix_prompt(url: &str, check: &CheckResult) -> String {
    let mut out = format!(
        "Fix this on {url}: “{}” is {} ({}).\n\nWhat was found: {}\n",
        check.name,
        check.status.as_str(),
        check.layer.name(),
        check.details
    );
    for e in &check.evidence {
        out.push_str(&format!(
            "- {} → {}{}\n",
            e.url,
            if e.status == 0 {
                "no answer".to_string()
            } else {
                e.status.to_string()
            },
            if e.note.is_empty() {
                String::new()
            } else {
                format!(" ({})", e.note)
            }
        ));
    }
    if !check.fix.is_empty() {
        out.push_str(&format!("\nHow to fix it: {}\n", check.fix));
    }
    if !check.spec.is_empty() {
        out.push_str(&format!("Spec: {}\n", check.spec));
    }
    out.push_str("\nMake the change in this project's code, then check it again with browser_seo.");
    out
}

/// A prompt for every failing or warning check, worst first.
pub fn fix_prompt_all(report: &Report, seo_only: bool) -> String {
    let mut out = format!(
        "Fix these SEO and agent-readiness problems on {}:\n\n",
        report.url
    );
    let mut list: Vec<&CheckResult> = report
        .checks
        .iter()
        .filter(|c| (!seo_only || c.seo) && matches!(c.status, Status::Fail | Status::Warn))
        .filter(|c| !c.external)
        .collect();
    list.sort_by_key(|c| (c.status != Status::Fail, c.tier as u8));
    for c in list {
        out.push_str(&format!(
            "- [{}] {}: {} Fix: {}\n",
            c.status.as_str(),
            c.name,
            c.details,
            c.fix
        ));
    }
    out.push_str("\nMake the changes in this project's code, then check again with browser_seo.");
    out
}

/// The report as text for an agent: the scores, then every check that ran, problems first, each
/// with its finding, evidence and fix.
pub fn report_text(report: &Report) -> String {
    let mut out = format!("SEO and agent-readiness scan of {}\n", report.url);
    if report.final_url != report.url && !report.final_url.is_empty() {
        out.push_str(&format!("Answered from: {}\n", report.final_url));
    }
    if let Some(e) = &report.error {
        out.push_str(&format!("The page couldn't be fetched: {e}\n"));
    }
    out.push_str(&format!(
        "Read: {}{}\n",
        if report.rendered {
            "the page as the browser rendered it, and the site's files"
        } else {
            "the server's HTML and the site's files"
        },
        if report.local {
            " (a local address)"
        } else {
            ""
        }
    ));
    out.push_str(&format!("Search and sharing: {}/100\n", report.seo_score));
    if report.layers.iter().any(|l| l.max > 0.) {
        out.push_str(&format!(
            "Agent readiness: {}/100 ({}), Essentials {}/100, scan type {}\n",
            report.agent_score,
            report.grade,
            report.essentials,
            match report.scan_type {
                ScanType::All => "all",
                ScanType::Content => "content",
                ScanType::Api => "api",
            }
        ));
        for l in report.layers.iter().filter(|l| l.max > 0.) {
            out.push_str(&format!(
                "  {}: {}% ({:.1} of {:.0} points, weight {:.0})\n",
                l.name, l.percent, l.score, l.max, l.weight
            ));
        }
    }
    if let Some(x) = &report.external {
        match (x.score, &x.missing) {
            (Some(s), _) => out.push_str(&format!(
                "External score: {s}/100{}\n",
                x.grade
                    .as_deref()
                    .map(|g| format!(" ({g})"))
                    .unwrap_or_default()
            )),
            (None, Some(why)) => out.push_str(&format!("External score: none ({why})\n")),
            _ => {}
        }
    }
    let [p, w, f] = report.counts(false);
    out.push_str(&format!("{p} passed, {w} warnings, {f} failed\n"));
    let mut list: Vec<&CheckResult> = report
        .checks
        .iter()
        .filter(|c| c.status != Status::Skipped)
        .collect();
    list.sort_by_key(|c| match c.status {
        Status::Fail => 0,
        Status::Warn => 1,
        Status::Pass => 2,
        _ => 3,
    });
    let mut section = None;
    for c in list {
        let group = match c.status {
            Status::Fail | Status::Warn => "To fix",
            Status::Pass => "Passed",
            _ => "Not applicable",
        };
        if section != Some(group) {
            out.push_str(&format!("\n{group}:\n"));
            section = Some(group);
        }
        out.push_str(&format!(
            "- [{}] {} ({}, {}): {}{}\n",
            c.status.as_str(),
            c.name,
            c.id,
            c.layer.name(),
            c.details,
            if c.status.scored() {
                format!(" [{:.1}/{:.0}]", c.score, c.max)
            } else {
                String::new()
            }
        ));
        if matches!(c.status, Status::Fail | Status::Warn) {
            for e in c.evidence.iter().take(5) {
                out.push_str(&format!(
                    "    {} → {}{}\n",
                    e.url,
                    if e.status == 0 {
                        "no answer".to_string()
                    } else {
                        e.status.to_string()
                    },
                    if e.note.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", e.note)
                    }
                ));
            }
            if !c.fix.is_empty() {
                out.push_str(&format!("    Fix: {}\n", c.fix));
            }
        }
    }
    out
}
