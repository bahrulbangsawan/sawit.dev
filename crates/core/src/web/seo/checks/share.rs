//! How the page looks when shared or saved: its icons, the touch icon, the manifest, Open Graph,
//! the OG image, the Twitter card and the JSON-LD.

use serde_json::Value;

use super::super::site::Site;
use super::super::{CheckResult, Evidence, PageFacts};
use super::{WithEvidence, fail, json_ld, pass, seen, types_of, warn};
use crate::web::favicon;

/// The icon, preview and JSON-LD checks, in order.
pub(super) async fn checks(site: &Site, f: &PageFacts, out: &mut Vec<CheckResult>) {
    let h = &site.head;
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
        (Some(url), Some(w), Some(hh)) => if f.image_type.as_deref() == Some("image/avif") {
                warn(
                    "og-image",
                    0.5,
                    format!("{w}×{hh} AVIF: WhatsApp, Facebook and LinkedIn don't show AVIF share images; use PNG or JPEG."),
                )
            } else if w >= 1200 && hh >= 630 {
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
