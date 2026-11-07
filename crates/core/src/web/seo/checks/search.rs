//! The search and sharing checks, in the table's order: the title, description and canonical,
//! the robots directives, hreflang and headings here; the icons and previews ([`super::share`])
//! and the sitemaps, llms files, images and links ([`super::files`]) in their own files.

use super::super::site::Site;
use super::super::{CheckResult, PageFacts};
use super::{WithEvidence, fail, na, pass, robots, seen, snippet, warn};

pub(super) async fn seo_checks(site: &Site, f: &PageFacts) -> Vec<CheckResult> {
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

    super::share::checks(site, f, &mut out).await;
    super::files::checks(site, f, &mut out).await;
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
