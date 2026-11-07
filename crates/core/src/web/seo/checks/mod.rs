//! Running the checks: the site loaded once ([`Site`]), each check reading what it needs from it,
//! every result scored, and the scores summed up per layer.

use std::time::Instant;

use serde_json::Value;

use super::catalog::{self, Applies, CheckDef};
use super::robots::Robots;
use super::site::{Site, image_size};
use super::sitemap::Sitemap;
use super::{
    CheckResult, Evidence, Layer, LayerScore, PageFacts, Report, ScanOptions, ScanType, Status,
    Tier,
};
use crate::web::Fetched;
use crate::web::favicon;
mod files;
mod search;
mod share;

use search::seo_checks;

/// A result for check `id`: `frac` of its points (1 for a pass, 0 for a fail).
pub(super) fn result(
    id: &str,
    status: Status,
    frac: f64,
    details: impl Into<String>,
) -> CheckResult {
    let def = catalog::def(id).unwrap_or_else(|| panic!("no check {id}"));
    let max = f64::from(def.max);
    let frac = match status {
        Status::Pass => frac.clamp(0., 1.),
        Status::Warn => frac.clamp(0., 1.),
        _ => 0.,
    };
    CheckResult {
        id: def.id.into(),
        name: def.name.into(),
        layer: def.layer,
        tier: def.tier,
        status,
        score: (max * frac * 10.).round() / 10.,
        max,
        details: details.into(),
        evidence: Vec::new(),
        fix: if status == Status::Pass || !status.scored() {
            String::new()
        } else {
            def.fix.into()
        },
        spec: def.spec.into(),
        seo: def.seo,
        external: false,
    }
}

pub(super) fn pass(id: &str, details: impl Into<String>) -> CheckResult {
    result(id, Status::Pass, 1., details)
}
pub(super) fn warn(id: &str, frac: f64, details: impl Into<String>) -> CheckResult {
    result(id, Status::Warn, frac, details)
}
pub(super) fn fail(id: &str, details: impl Into<String>) -> CheckResult {
    result(id, Status::Fail, 0., details)
}
pub(super) fn na(id: &str, details: impl Into<String>) -> CheckResult {
    result(id, Status::Na, 0., details)
}

/// Evidence from a fetch.
pub(super) fn seen(got: &Fetched, note: impl Into<String>) -> Evidence {
    let mut note = note.into();
    if note.is_empty()
        && let Some(e) = &got.error
    {
        note = e.clone();
    }
    Evidence {
        url: got.url.clone(),
        status: got.status,
        note,
    }
}

trait WithEvidence {
    fn with(self, evidence: Evidence) -> Self;
}

impl WithEvidence for CheckResult {
    fn with(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }
}

/// The first `n` characters of `text`, for evidence.
pub(super) fn snippet(text: &str, n: usize) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= n {
        text
    } else {
        format!("{}…", text.chars().take(n).collect::<String>())
    }
}

/// Whether check `def` runs for `options`.
fn wanted(def: &CheckDef, options: &ScanOptions) -> bool {
    let kind = matches!(
        (options.scan_type, def.applies),
        (_, Applies::All)
            | (ScanType::All, _)
            | (ScanType::Content, Applies::Content)
            | (ScanType::Api, Applies::Api)
    );
    kind && (options.checks.is_empty() || options.checks.iter().any(|c| c == def.id))
}

pub(super) async fn run(url: &str, options: ScanOptions) -> Report {
    let started = Instant::now();
    let site = Site::load(url, options.rendered_html.clone()).await;
    let mut report = Report {
        url: url.to_string(),
        final_url: site.page.url.clone(),
        status: site.page.status,
        local: site.local,
        scan_type: options.scan_type,
        rendered: site.rendered,
        ..Default::default()
    };
    if site.page.status == 0 {
        report.error = Some(
            site.page
                .error
                .clone()
                .unwrap_or_else(|| "The page didn't answer.".into()),
        );
    }
    let facts = facts(&site).await;
    let (mut results, agent) = futures::join!(seo_checks(&site, &facts), super::agent::run(&site));
    results.extend(agent);
    if options.external {
        let (score, theirs) = super::external::scan(&site.page.url).await;
        report.external = Some(score);
        results.extend(theirs);
    }
    results.retain(|r| catalog::def(&r.id).is_some_and(|d| wanted(d, &options)));
    // Checks that weren't asked for, or that only the external scan answers, are listed as such.
    for def in catalog::CHECKS {
        if results.iter().any(|r| r.id == def.id) {
            continue;
        }
        let reason = if def.external {
            "Needs outside data (search results, directories): only the external scan answers it."
        } else if !wanted(def, &options) {
            "Not part of this scan."
        } else {
            continue;
        };
        results.push(result(def.id, Status::Skipped, 0., reason));
    }
    order(&mut results);
    report.facts = facts;
    report.checks = results;
    score(&mut report);
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report
}

/// The checks in the table's order.
fn order(results: &mut [CheckResult]) {
    let place = |id: &str| {
        catalog::CHECKS
            .iter()
            .position(|c| c.id == id)
            .unwrap_or(usize::MAX)
    };
    results.sort_by_key(|r| place(&r.id));
}

/// What the page says about itself, the OG image's size fetched.
async fn facts(site: &Site) -> PageFacts {
    let h = &site.head;
    let meta = |k: &str| h.meta(k).filter(|v| !v.is_empty()).map(String::from);
    let mut f = PageFacts {
        title: h.title.clone().filter(|t| !t.is_empty()),
        description: meta("description"),
        canonical: h.canonical().map(String::from),
        lang: h.lang.clone().filter(|l| !l.is_empty()),
        robots: meta("robots"),
        site_name: meta("og:site_name"),
        og_title: meta("og:title"),
        og_description: meta("og:description"),
        og_image: meta("og:image")
            .or_else(|| meta("og:image:url"))
            .or_else(|| meta("og:image:secure_url"))
            .map(|i| absolute(site, &i)),
        og_type: meta("og:type"),
        og_url: meta("og:url"),
        twitter_card: meta("twitter:card"),
        twitter_title: meta("twitter:title"),
        twitter_description: meta("twitter:description"),
        twitter_image: meta("twitter:image")
            .or_else(|| meta("twitter:image:src"))
            .map(|i| absolute(site, &i)),
        icons: h
            .links
            .iter()
            .filter(|l| favicon::is_icon(l) || l.has_rel("mask-icon") || l.has_rel("manifest"))
            .map(|l| (l.rel.join(" "), l.sizes.clone(), l.href.clone()))
            .collect(),
        headings: h
            .headings
            .iter()
            .map(|x| (x.level, x.text.clone()))
            .collect(),
        hreflang: h
            .links
            .iter()
            .filter(|l| l.has_rel("alternate"))
            .filter_map(|l| Some((l.hreflang.clone()?, l.href.clone())))
            .collect(),
        schema_types: schema_types(&h.json_ld),
        images: h.images.len(),
        images_without_alt: h.images.iter().filter(|i| i.alt.is_none()).count(),
        links: h.anchors.len(),
        host: site.host.trim_start_matches("www.").to_string(),
        ..Default::default()
    };
    if let Some(image) = f.og_image.clone().or_else(|| f.twitter_image.clone()) {
        let got = site.get_with(&image, &[("Accept", "image/*")]).await;
        if got.ok()
            && let Some((w, h, mime)) = image_size(&got.body)
        {
            f.image_width = Some(w);
            f.image_height = Some(h);
            f.image_type = Some(mime.to_string());
        }
    }
    f
}

fn absolute(site: &Site, href: &str) -> String {
    crate::web::Url::parse(&site.head.base)
        .and_then(|b| b.join(href.trim()))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| href.to_string())
}

/// The JSON-LD blocks, parsed (bad ones left out).
pub(super) fn json_ld(blocks: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for block in blocks {
        let Ok(v) = serde_json::from_str::<Value>(block.trim()) else {
            continue;
        };
        flatten_ld(v, &mut out);
    }
    out
}

fn flatten_ld(v: Value, out: &mut Vec<Value>) {
    match v {
        Value::Array(items) => items.into_iter().for_each(|i| flatten_ld(i, out)),
        Value::Object(mut map) => {
            if let Some(graph) = map.remove("@graph") {
                flatten_ld(graph, out);
            }
            if map.contains_key("@type") {
                out.push(Value::Object(map));
            }
        }
        _ => {}
    }
}

/// The `@type`s of an entity (a string or a list).
pub(super) fn types_of(entity: &Value) -> Vec<String> {
    match &entity["@type"] {
        Value::String(t) => vec![t.clone()],
        Value::Array(ts) => ts
            .iter()
            .filter_map(|t| t.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn schema_types(blocks: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in json_ld(blocks) {
        for t in types_of(&e) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// The parsed `robots.txt`, if the site serves one.
pub(super) async fn robots(site: &Site) -> (std::sync::Arc<Fetched>, Option<Robots>) {
    let got = site.get("/robots.txt").await;
    let robots = site.real_file(&got).then(|| Robots::parse(&got.text()));
    (got, robots)
}

/// The site's sitemap: the one robots.txt names, else `/sitemap.xml`, else `/sitemap_index.xml`.
pub(super) async fn find_sitemap(
    site: &Site,
) -> (Option<std::sync::Arc<Fetched>>, Option<Sitemap>) {
    let (_, robots) = robots(site).await;
    let mut tried = Vec::new();
    let named = robots.map(|r| r.sitemaps).unwrap_or_default();
    for path in named
        .iter()
        .map(String::as_str)
        .chain(["/sitemap.xml", "/sitemap_index.xml"])
        .take(4)
    {
        let got = site.get(path).await;
        if got.ok()
            && let Some(map) = Sitemap::parse(&got.text())
        {
            return (Some(got), Some(map));
        }
        tried.push(got);
    }
    (tried.into_iter().next(), None)
}
/// Points earned and possible: every required and recommended check counts in full; a bonus
/// check counts (in both) only when it passes or warns, so it can lift a score and never lowers
/// one.
fn points(checks: &[&CheckResult]) -> (f64, f64) {
    checks
        .iter()
        .filter(|c| c.tier != Tier::Bonus || matches!(c.status, Status::Pass | Status::Warn))
        .fold((0., 0.), |(s, m), c| (s + c.score, m + c.max))
}

/// Works out the scores: the search and sharing score from the `seo` checks, each agent layer's
/// score and the weighted total, the content sites' "Essentials" score and the grade.
pub(super) fn score(report: &mut Report) {
    let counted = |c: &&CheckResult| c.status.scored() && c.tier != Tier::Emerging;
    // Search and sharing: points earned over points possible, bonus checks adding only.
    let seo: Vec<&CheckResult> = report
        .checks
        .iter()
        .filter(|c| c.seo)
        .filter(counted)
        .collect();
    let (earned, possible) = points(&seo);
    report.seo_score = percent(earned, possible);

    let mut layers = Vec::new();
    let (mut weighted, mut weights) = (0., 0.);
    for layer in Layer::AGENT {
        let checks: Vec<&CheckResult> = report
            .checks
            .iter()
            .filter(|c| c.layer == layer)
            .filter(counted)
            .collect();
        let (score, max) = points(&checks);
        // Payments count when any one protocol is supported (all of them are bonus checks).
        let (score, max) = if layer == Layer::Payments {
            let applies = report
                .checks
                .iter()
                .any(|c| c.layer == layer && c.status.scored());
            let any = report
                .checks
                .iter()
                .any(|c| c.layer == layer && c.status == Status::Pass);
            match (applies, any) {
                (false, _) => (0., 0.),
                (true, true) => (1., 1.),
                (true, false) => (0., 1.),
            }
        } else {
            (score, max)
        };
        if max > 0. {
            weighted += layer.weight() * (score / max).min(1.);
            weights += layer.weight();
        }
        layers.push(LayerScore {
            layer,
            name: layer.name().into(),
            score,
            max,
            percent: percent(score, max),
            weight: layer.weight(),
        });
    }
    report.agent_score = if weights > 0. {
        (weighted / weights * 100.).round() as u32
    } else {
        0
    };
    report.layers = layers;
    report.grade = grade(report.agent_score).into();

    // Essentials: required 80, recommended 20, up to 5 more from bonus and emerging checks.
    let part = |tier: Tier| {
        let list: Vec<&CheckResult> = report
            .checks
            .iter()
            .filter(|c| c.tier == tier && c.status.scored() && c.layer != Layer::Seo)
            .collect();
        let max: f64 = list.iter().map(|c| c.max).sum();
        (list.iter().map(|c| c.score).sum::<f64>(), max)
    };
    let (req, req_max) = part(Tier::Required);
    let (rec, rec_max) = part(Tier::Recommended);
    let (bonus_a, bonus_a_max) = part(Tier::Bonus);
    let (bonus_b, bonus_b_max) = part(Tier::Emerging);
    let ratio = |s: f64, m: f64| if m > 0. { (s / m).min(1.) } else { 1. };
    let extra = if bonus_a_max + bonus_b_max > 0. {
        5. * (bonus_a + bonus_b) / (bonus_a_max + bonus_b_max)
    } else {
        0.
    };
    report.essentials = if req_max + rec_max > 0. {
        (80. * ratio(req, req_max) + 20. * ratio(rec, rec_max) + extra)
            .round()
            .min(100.) as u32
    } else {
        0
    };
}

pub(super) fn percent(score: f64, max: f64) -> u32 {
    if max <= 0. {
        return 0;
    }
    ((score / max).min(1.) * 100.).round() as u32
}

/// A grade from A+ to F.
pub fn grade(score: u32) -> &'static str {
    match score {
        95.. => "A+",
        90..=94 => "A",
        75..=89 => "B",
        45..=74 => "C",
        30..=44 => "D",
        _ => "F",
    }
}
