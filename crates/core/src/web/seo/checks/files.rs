//! The files search engines and agents read beside the page (sitemaps, llms files), its images
//! and links, HTTPS, the viewport and the metadata's completeness.

use futures::future::join_all;

use super::super::site::Site;
use super::super::sitemap;
use super::super::{CheckResult, Evidence, PageFacts};
use super::{WithEvidence, fail, find_sitemap, na, pass, seen, warn};
use crate::web::Fetched;

/// The sitemap, llms file, image, link and metadata checks, in order.
pub(super) async fn checks(site: &Site, f: &PageFacts, out: &mut Vec<CheckResult>) {
    let h = &site.head;
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
