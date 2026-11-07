//! Running the checks: the site loaded once ([`Site`]), each check reading what it needs from it,
//! every result scored, and the scores summed up per layer.

use std::time::Instant;

use futures::future::join_all;
use serde_json::Value;

use super::catalog::{self, Applies, CheckDef};
use super::robots::Robots;
use super::site::{Site, image_size};
use super::sitemap::{self, Sitemap};
use super::{
    CheckResult, Evidence, Layer, LayerScore, PageFacts, Report, ScanOptions, ScanType, Status,
    Tier,
};
use crate::web::Fetched;
use crate::web::favicon;

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
    let mut results = seo_checks(&site, &facts).await;
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

async fn seo_checks(site: &Site, f: &PageFacts) -> Vec<CheckResult> {
    let h = &site.head;
    let page_ev = seen(&site.page, "");
    let mut out = Vec::new();

    // Title and description.
    out.push(match &f.title {
        None => fail("title", "The page has no <title>."),
        Some(t) => {
            let n = t.chars().count();
            if (10..=60).contains(&n) {
                pass("title", format!("“{t}” ({n} characters)."))
            } else if n < 10 {
                warn(
                    "title",
                    0.5,
                    format!("“{t}” is short ({n} characters; aim for 10 to 60)."),
                )
            } else {
                warn(
                    "title",
                    0.5,
                    format!(
                        "“{}” is long ({n} characters; results cut it near 60).",
                        snippet(t, 70)
                    ),
                )
            }
        }
    });
    out.push(match &f.description {
        None => fail(
            "meta-description",
            "No meta description: search results make one up from the page.",
        ),
        Some(d) => {
            let n = d.chars().count();
            if (50..=160).contains(&n) {
                pass("meta-description", format!("{n} characters."))
            } else if n < 50 {
                warn(
                    "meta-description",
                    0.5,
                    format!("Short ({n} characters; aim for 50 to 160)."),
                )
            } else {
                warn(
                    "meta-description",
                    0.5,
                    format!("Long ({n} characters; results cut it near 160)."),
                )
            }
        }
    });
    out.push(match &f.canonical {
        None => fail("canonical", "No canonical link."),
        Some(c) => {
            let same = crate::web::Url::parse(c)
                .ok()
                .and_then(|u| {
                    u.host_str()
                        .map(|h| h.trim_start_matches("www.").to_string())
                })
                .is_some_and(|host| host == f.host);
            if !c.starts_with("http") {
                warn(
                    "canonical",
                    0.5,
                    format!("“{c}” isn't an absolute address."),
                )
            } else if !same {
                warn("canonical", 0.5, format!("Points to another site: {c}."))
            } else {
                pass("canonical", c.clone())
            }
        }
    });

    // Robots meta and header.
    let mut hidden = Vec::new();
    for key in ["robots", "googlebot", "bingbot"] {
        for v in h.metas_named(key) {
            if v.to_ascii_lowercase().contains("noindex") || v.to_ascii_lowercase().contains("none")
            {
                hidden.push(format!("<meta name=\"{key}\" content=\"{v}\">"));
            }
        }
    }
    for v in site.page.headers_named("x-robots-tag") {
        if v.to_ascii_lowercase().contains("noindex") {
            hidden.push(format!("X-Robots-Tag: {v}"));
        }
    }
    out.push(if hidden.is_empty() {
        pass(
            "noindex-directives",
            "No noindex in the meta tags or the X-Robots-Tag header.",
        )
    } else {
        fail(
            "noindex-directives",
            format!("Hidden from search by {}.", hidden.join(", ")),
        )
        .with(page_ev.clone())
    });

    let (robots_got, robots) = robots(site).await;
    out.push(match &robots {
        None => fail("robots-txt", "No /robots.txt.").with(seen(&robots_got, "")),
        Some(r) => {
            let blocked = !r.allowed("*", "/") && !r.allowed("googlebot", "/");
            if blocked {
                fail(
                    "robots-txt",
                    "robots.txt blocks the whole site (Disallow: /).",
                )
            } else if r.sitemaps.is_empty() {
                warn(
                    "robots-txt",
                    0.5,
                    format!("{} groups, but no Sitemap: line.", r.groups.len()),
                )
            } else {
                pass(
                    "robots-txt",
                    format!(
                        "{} groups, {} sitemaps named.",
                        r.groups.len(),
                        r.sitemaps.len()
                    ),
                )
            }
            .with(seen(&robots_got, ""))
        }
    });

    // hreflang.
    out.push(if f.hreflang.is_empty() {
        na(
            "hreflang",
            "The page names no language alternates (fine for a site in one language).",
        )
    } else {
        let bad: Vec<&str> = f
            .hreflang
            .iter()
            .filter(|(code, href)| !valid_lang(code) || !href.starts_with("http"))
            .map(|(c, _)| c.as_str())
            .collect();
        let has_default = f
            .hreflang
            .iter()
            .any(|(c, _)| c.eq_ignore_ascii_case("x-default"));
        let page = site.page.url.trim_end_matches('/');
        let has_self = f
            .hreflang
            .iter()
            .any(|(_, u)| u.trim_end_matches('/') == page);
        if !bad.is_empty() {
            fail(
                "hreflang",
                format!("Bad codes or relative addresses: {}.", bad.join(", ")),
            )
        } else if !has_default || !has_self {
            warn(
                "hreflang",
                0.5,
                format!(
                    "{} alternates; missing {}.",
                    f.hreflang.len(),
                    [
                        (!has_default).then_some("x-default"),
                        (!has_self).then_some("the page itself")
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" and ")
                ),
            )
        } else {
            pass(
                "hreflang",
                format!("{} alternates, x-default among them.", f.hreflang.len()),
            )
        }
    });

    // Headings.
    let h1s = h.headings.iter().filter(|x| x.level == 1).count();
    let skip = h
        .headings
        .windows(2)
        .find(|w| w[1].level > w[0].level + 1)
        .map(|w| (w[0].level, w[1].level));
    out.push(match (h1s, skip) {
        (0, _) => fail("headings", "No <h1>."),
        (1, None) => pass(
            "headings",
            format!("One <h1>, {} headings, no level skipped.", h.headings.len()),
        ),
        (1, Some((a, b))) => warn(
            "headings",
            0.5,
            format!("h{a} is followed by h{b}: a level is skipped."),
        ),
        (n, _) => warn(
            "headings",
            0.5,
            format!("{n} <h1>s: keep one for the page's subject."),
        ),
    });

    // Icons, the touch icon, the manifest, the OG image and llms files: fetched together.
    let icon_links: Vec<_> = h
        .links
        .iter()
        .filter(|l| l.has_rel("icon"))
        .cloned()
        .collect();
    let touch: Vec<_> = h
        .links
        .iter()
        .filter(|l| l.has_rel("apple-touch-icon") || l.has_rel("apple-touch-icon-precomposed"))
        .cloned()
        .collect();
    let manifest = h.links_rel("manifest").next().map(|l| l.href.clone());
    let favicon_urls = favicon::candidates(&icon_links, &site.page.url);
    let favicon_check = async {
        for url in favicon_urls.iter().take(4) {
            if let Some(icon) = favicon::load_icon(url).await {
                return Some((url.clone(), icon.width, icon.height));
            }
        }
        None
    };
    let touch_check = async {
        let url = touch.first().map(|l| l.href.clone())?;
        let icon = favicon::load_icon(&url).await;
        Some((url, icon.map(|i| (i.width, i.height))))
    };
    let manifest_check = async {
        let url = manifest?;
        Some(site.json(&url).await)
    };
    let (fav, touch_result, manifest_result) =
        futures::join!(favicon_check, touch_check, manifest_check);
    out.push(match fav {
        Some((url, ..)) => pass("favicon", format!("Loads from {url}.")),
        None => fail(
            "favicon",
            "No icon loads: neither the <link rel=\"icon\"> ones nor /favicon.ico.",
        ),
    });
    out.push(match touch_result {
        None => fail("apple-touch-icon", "No apple-touch-icon link."),
        Some((url, Some(_))) => pass("apple-touch-icon", format!("Loads from {url}.")),
        Some((url, None)) => fail(
            "apple-touch-icon",
            format!("{url} doesn't load as a picture."),
        ),
    });
    out.push(match manifest_result {
        None => fail("web-manifest", "No <link rel=\"manifest\">."),
        Some((got, Some(m))) => {
            let icons = m["icons"].as_array().map_or(0, Vec::len);
            let name = m["name"].is_string() || m["short_name"].is_string();
            if name && icons > 0 {
                pass("web-manifest", format!("Named, with {icons} icons."))
            } else {
                warn("web-manifest", 0.5, "The manifest has no name or no icons.")
            }
            .with(seen(&got, ""))
        }
        Some((got, None)) => {
            fail("web-manifest", "The manifest doesn't load as JSON.").with(seen(&got, ""))
        }
    });

    // Open Graph.
    let og = [
        ("og:title", &f.og_title),
        ("og:description", &f.og_description),
        ("og:image", &f.og_image),
        ("og:url", &f.og_url),
        ("og:type", &f.og_type),
    ];
    let missing: Vec<&str> = og
        .iter()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| *k)
        .collect();
    out.push(match missing.len() {
        0 => pass("open-graph", "All five set."),
        5 => fail(
            "open-graph",
            "No Open Graph tags: shared links show no title or picture.",
        ),
        n => warn(
            "open-graph",
            (5 - n) as f64 / 5.,
            format!("Missing {}.", missing.join(", ")),
        ),
    });
    out.push(match (&f.og_image, f.image_width, f.image_height) {
        (None, ..) => fail("og-image", "No og:image: shared links show no picture."),
        (Some(url), Some(w), Some(hh)) => if w >= 1200 && hh >= 630 {
            pass(
                "og-image",
                format!("{w}×{hh} {}.", f.image_type.as_deref().unwrap_or("")),
            )
        } else if w >= 600 {
            warn(
                "og-image",
                0.5,
                format!("{w}×{hh}: smaller than 1200×630, previews may be small or blurry."),
            )
        } else {
            warn(
                "og-image",
                0.25,
                format!("{w}×{hh}: too small for a large preview."),
            )
        }
        .with(Evidence {
            url: url.clone(),
            status: 200,
            note: String::new(),
        }),
        (Some(url), ..) => {
            let got = site.get_with(url, &[("Accept", "image/*")]).await;
            fail("og-image", "The og:image doesn't load as a picture.").with(seen(&got, ""))
        }
    });
    out.push(match &f.twitter_card {
        Some(c) => pass("twitter-card", format!("twitter:card is {c}.")),
        None if f.og_image.is_some() => warn(
            "twitter-card",
            0.5,
            "No twitter:card: X shows a small preview from Open Graph.",
        ),
        None => fail("twitter-card", "No twitter:card."),
    });

    out.push(json_ld_check(site));

    // Sitemaps and llms files.
    let (map_got, map) = find_sitemap(site).await;
    out.push(match (&map_got, &map) {
        (Some(got), Some(m)) => {
            let what = if m.index { "sitemap index" } else { "sitemap" };
            if m.entries.is_empty() {
                warn("sitemap", 0.5, format!("The {what} is empty.")).with(seen(got, ""))
            } else {
                pass(
                    "sitemap",
                    format!("A {what} with {} entries.", m.entries.len()),
                )
                .with(seen(got, ""))
            }
        }
        (got, _) => {
            let r = fail(
                "sitemap",
                "No valid sitemap at /sitemap.xml or where robots.txt says.",
            );
            match got {
                Some(g) => r.with(seen(g, "")),
                None => r,
            }
        }
    });
    let (smd, llms, llms_full, full_llms) = futures::join!(
        site.get("/sitemap.md"),
        site.get("/llms.txt"),
        site.get("/llms-full.txt"),
        site.get("/full-llms.txt"),
    );
    out.push(if site.real_file(&smd) && smd.text().contains("](") {
        pass(
            "sitemap-md",
            format!("{} links.", sitemap::markdown_links(&smd.text()).len()),
        )
        .with(seen(&smd, ""))
    } else {
        fail("sitemap-md", "No /sitemap.md.").with(seen(&smd, ""))
    });
    let llms_ok = site.real_file(&llms);
    out.push(if llms_ok {
        pass(
            "llms-txt-exists",
            format!("{} characters.", llms.body.len()),
        )
        .with(seen(&llms, ""))
    } else {
        fail("llms-txt-exists", "No /llms.txt.").with(seen(&llms, ""))
    });
    out.push(if llms_ok {
        let shape = sitemap::llms_shape(&llms.text());
        let parts = [shape.h1, shape.summary, shape.sections > 0, shape.links > 0];
        let have = parts.iter().filter(|p| **p).count();
        let details = format!(
            "{} lines, {} sections, {} links{}{}.",
            shape.lines,
            shape.sections,
            shape.links,
            if shape.h1 { "" } else { ", no # H1 first" },
            if shape.summary { "" } else { ", no > summary" }
        );
        match have {
            4 => pass("llms-txt-formatting", details),
            0 => fail("llms-txt-formatting", details),
            n => warn("llms-txt-formatting", n as f64 / 4., details),
        }
    } else {
        na("llms-txt-formatting", "No llms.txt to read.")
    });
    out.push(if site.real_file(&llms_full) {
        pass(
            "llms-full-txt",
            format!("/llms-full.txt, {} characters.", llms_full.body.len()),
        )
        .with(seen(&llms_full, ""))
    } else if site.real_file(&full_llms) {
        warn(
            "llms-full-txt",
            0.5,
            "Served as /full-llms.txt: agents look for /llms-full.txt.",
        )
        .with(seen(&full_llms, ""))
    } else {
        fail("llms-full-txt", "No /llms-full.txt (nor /full-llms.txt).").with(seen(&llms_full, ""))
    });

    // Images and links.
    out.push(if h.images.is_empty() {
        na("image-alt", "The page has no images.")
    } else {
        let without = f.images_without_alt;
        let share = 1. - without as f64 / h.images.len() as f64;
        if without == 0 {
            pass(
                "image-alt",
                format!("All {} images have alt text.", h.images.len()),
            )
        } else {
            let mut r = if share >= 0.5 {
                warn(
                    "image-alt",
                    share,
                    format!("{without} of {} images have no alt.", h.images.len()),
                )
            } else {
                fail(
                    "image-alt",
                    format!("{without} of {} images have no alt.", h.images.len()),
                )
            };
            for i in h.images.iter().filter(|i| i.alt.is_none()).take(5) {
                r = r.with(Evidence {
                    url: i.src.clone(),
                    status: 0,
                    note: "no alt".into(),
                });
            }
            r
        }
    });
    out.push(broken_links(site).await);

    out.push(if site.local {
        na("https", "A local address: HTTPS isn't needed.")
    } else if site.page.url.starts_with("https://") {
        pass("https", "Served over HTTPS.")
    } else {
        fail("https", "Served over plain HTTP.")
    });
    out.push(match h.meta("viewport") {
        Some(v) if v.contains("width=device-width") => pass("viewport", v.to_string()),
        Some(v) => warn(
            "viewport",
            0.5,
            format!("“{v}” doesn't set width=device-width."),
        ),
        None => fail(
            "viewport",
            "No viewport meta tag: phones show the desktop layout zoomed out.",
        ),
    });
    let missing: Vec<&str> = [
        (f.canonical.is_none(), "canonical"),
        (f.lang.is_none(), "lang"),
        (f.og_image.is_none(), "og:image"),
        (f.og_type.is_none(), "og:type"),
    ]
    .into_iter()
    .filter(|(m, _)| *m)
    .map(|(_, n)| n)
    .collect();
    out.push(match missing.len() {
        0 => pass(
            "metadata-completeness",
            "Canonical, lang, og:image and og:type all present.",
        ),
        4 => fail(
            "metadata-completeness",
            "None of canonical, lang, og:image, og:type.",
        ),
        n => warn(
            "metadata-completeness",
            (4 - n) as f64 / 4.,
            format!("Missing {}.", missing.join(", ")),
        ),
    });
    out
}

/// A language code as hreflang takes it: `en`, `en-GB`, `zh-Hant`, `x-default`.
fn valid_lang(code: &str) -> bool {
    if code.eq_ignore_ascii_case("x-default") {
        return true;
    }
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or("");
    (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn json_ld_check(site: &Site) -> CheckResult {
    let blocks = &site.head.json_ld;
    if blocks.is_empty() {
        return fail("json-ld", "No JSON-LD on the page.");
    }
    let bad = blocks
        .iter()
        .filter(|b| serde_json::from_str::<Value>(b.trim()).is_err())
        .count();
    let entities = json_ld(blocks);
    let identity = entities.iter().find(|e| {
        types_of(e).iter().any(|t| {
            matches!(
                t.as_str(),
                "Organization"
                    | "Corporation"
                    | "LocalBusiness"
                    | "Person"
                    | "WebSite"
                    | "SoftwareApplication"
                    | "Product"
                    | "ProfessionalService"
            )
        })
    });
    let types: Vec<String> = entities.iter().flat_map(types_of).collect();
    match identity {
        _ if entities.is_empty() => fail(
            "json-ld",
            format!("{} blocks, none parse with a @type.", blocks.len()),
        ),
        Some(e) => {
            let name = e["name"].is_string();
            let description = e["description"].is_string();
            let t = types_of(e).join(", ");
            if name && description && bad == 0 {
                pass(
                    "json-ld",
                    format!("{t} with name and description ({}).", types.join(", ")),
                )
            } else {
                let mut gaps = Vec::new();
                if !name {
                    gaps.push("name");
                }
                if !description {
                    gaps.push("description");
                }
                let mut details = format!("{t} found");
                if !gaps.is_empty() {
                    details.push_str(&format!(", missing {}", gaps.join(" and ")));
                }
                if bad > 0 {
                    details.push_str(&format!("; {bad} blocks don't parse"));
                }
                warn("json-ld", 0.5, format!("{details}."))
            }
        }
        None => warn(
            "json-ld",
            0.25,
            format!(
                "Types {}: none says who is behind the site (Organization, Person, WebSite).",
                types.join(", ")
            ),
        ),
    }
}

/// The page's links, up to 30, each fetched once (HEAD, then GET when HEAD isn't allowed).
async fn broken_links(site: &Site) -> CheckResult {
    let mut urls: Vec<String> = Vec::new();
    for a in &site.head.anchors {
        let href = a.href.split('#').next().unwrap_or("").to_string();
        if !(href.starts_with("http://") || href.starts_with("https://")) || urls.contains(&href) {
            continue;
        }
        urls.push(href);
        if urls.len() == 30 {
            break;
        }
    }
    if urls.is_empty() {
        return na("broken-links", "The page has no links to check.");
    }
    let checks = urls.iter().map(|u| async move {
        let head = site.request("HEAD", u, &[], None).await;
        if matches!(head.status, 405 | 403 | 501) || head.status == 0 {
            site.get(u).await
        } else {
            head
        }
    });
    let got = join_all(checks).await;
    let broken: Vec<&std::sync::Arc<Fetched>> = got
        .iter()
        .filter(|g| g.status == 0 || g.status >= 400)
        .collect();
    if broken.is_empty() {
        return pass(
            "broken-links",
            format!("All {} links checked answer.", got.len()),
        );
    }
    let share = 1. - broken.len() as f64 / got.len() as f64;
    let mut r = if broken.len() <= 2 {
        warn(
            "broken-links",
            share,
            format!("{} of {} links are broken.", broken.len(), got.len()),
        )
    } else {
        fail(
            "broken-links",
            format!("{} of {} links are broken.", broken.len(), got.len()),
        )
    };
    for g in broken.into_iter().take(8) {
        r = r.with(Evidence {
            url: g.requested.clone(),
            status: g.status,
            note: g.error.clone().unwrap_or_default(),
        });
    }
    r
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
    let earned: f64 = seo.iter().map(|c| c.score).sum();
    let possible: f64 = seo
        .iter()
        .filter(|c| c.tier != Tier::Bonus)
        .map(|c| c.max)
        .sum();
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
        let score: f64 = checks.iter().map(|c| c.score).sum();
        let max: f64 = checks
            .iter()
            .filter(|c| c.tier != Tier::Bonus)
            .map(|c| c.max)
            .sum();
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
