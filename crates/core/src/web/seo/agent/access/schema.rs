//! What the structured data and the links say: prices, docs, guidance for agents, the
//! Organization, the breadth of schema types, trust links and the Markdown alternate.

use std::sync::Arc;

use serde_json::Value;

use super::super::super::CheckResult;
use super::super::super::checks::{fail, json_ld, pass, seen, types_of, warn};
use super::super::super::site::Site;
use super::super::{With, is_markdown, links_in_header};
use crate::web::Fetched;

/// The structured data checks, in order; answers the docs site found at `docs.<domain>`, if any.
pub(super) async fn checks(
    site: &Site,
    out: &mut Vec<CheckResult>,
    agents_md: &Fetched,
    llms_text: &str,
) -> Option<Arc<Fetched>> {
    let h = &site.head;
    let entities = json_ld(&h.json_ld);
    let all_types: Vec<String> = entities.iter().flat_map(types_of).collect();
    let raw_ld = h.json_ld.join("\n");
    let has_offer = all_types.iter().any(|t| {
        matches!(
            t.as_str(),
            "Offer" | "AggregateOffer" | "PriceSpecification" | "UnitPriceSpecification"
        )
    }) || [
        "\"Offer\"",
        "\"AggregateOffer\"",
        "PriceSpecification",
        "\"offers\"",
        "\"priceRange\"",
    ]
    .iter()
    .any(|k| raw_ld.contains(k));
    let anchor = |words: &[&str]| {
        h.anchors.iter().find(|a| {
            let href = a.href.to_ascii_lowercase();
            let text = a.text.to_ascii_lowercase();
            words.iter().any(|w| href.contains(w) || text.contains(w))
        })
    };
    out.push(if has_offer {
        pass("pricing-info", "Prices in JSON-LD (Offer).")
    } else if let Some(a) = anchor(&["pricing", "/price", "plans"]) {
        warn(
            "pricing-info",
            1. / 3.,
            format!("A pricing link ({}), but no Offer in JSON-LD.", a.href),
        )
    } else {
        fail("pricing-info", "No prices in JSON-LD and no pricing link.")
    });
    let docs = h.anchors.iter().find(|a| {
        let href = a.href.to_ascii_lowercase();
        href.contains("://docs.")
            || href.contains("/docs")
            || href.contains("/developers")
            || href.contains("/api")
            || href.contains("/reference")
    });
    let docs_site = if docs.is_none() && !site.local {
        let url = format!("https://docs.{}/", site.host.trim_start_matches("www."));
        let got = site.get(&url).await;
        got.ok().then_some(got)
    } else {
        None
    };
    out.push(match (docs, &docs_site) {
        (Some(a), _) => pass("public-api-docs", format!("Links {}.", a.href)),
        (None, Some(g)) => pass(
            "public-api-docs",
            format!("A docs site answers at {}.", g.url),
        )
        .with(seen(g, "")),
        (None, None) => fail("public-api-docs", "No link to docs or an API reference."),
    });
    let lower = llms_text.to_ascii_lowercase();
    let guidance = [
        "when to use",
        "use this",
        "agents should",
        "for agents",
        "use cases",
        "when an agent",
        "how to use",
    ];
    out.push(if guidance.iter().any(|g| lower.contains(g)) {
        pass(
            "agent-instruction",
            "llms.txt says when and how to use the site.",
        )
    } else if site.real_file(agents_md) {
        pass("agent-instruction", "AGENTS.md gives agents guidance.")
    } else {
        fail(
            "agent-instruction",
            "No when-to-use guidance in llms.txt or AGENTS.md.",
        )
    });
    let same_as = entities.iter().find_map(|e| match &e["sameAs"] {
        Value::Array(a) if !a.is_empty() => Some(a.len()),
        Value::String(_) => Some(1),
        _ => None,
    });
    out.push(match same_as {
        Some(n) => pass(
            "json-ld-entity-linking",
            format!("sameAs links {n} profiles."),
        ),
        None if entities.is_empty() => fail("json-ld-entity-linking", "No JSON-LD."),
        None => fail("json-ld-entity-linking", "No sameAs in the JSON-LD."),
    });
    let org = entities.iter().find(|e| {
        types_of(e).iter().any(|t| {
            matches!(
                t.as_str(),
                "Organization"
                    | "Corporation"
                    | "LocalBusiness"
                    | "ProfessionalService"
                    | "NGO"
                    | "EducationalOrganization"
            )
        })
    });
    out.push(match org {
        None => fail("org-schema-completeness", "No Organization in the JSON-LD."),
        Some(o) => {
            let contact = !o["contactPoint"].is_null()
                || o["telephone"].is_string()
                || o["email"].is_string();
            let address = !o["address"].is_null();
            match (contact, address) {
                (true, true) => pass(
                    "org-schema-completeness",
                    "The Organization has contact details and an address.",
                ),
                (false, false) => warn(
                    "org-schema-completeness",
                    0.5,
                    "The Organization has no contactPoint or address.",
                ),
                (c, _) => warn(
                    "org-schema-completeness",
                    0.5,
                    format!("Missing {}.", if c { "address" } else { "contactPoint" }),
                ),
            }
        }
    });
    const EXTENDED: &[&str] = &[
        "FAQPage",
        "Service",
        "Product",
        "Offer",
        "AggregateRating",
        "Review",
        "HowTo",
        "Article",
        "BlogPosting",
        "NewsArticle",
        "Event",
        "Course",
        "Recipe",
        "SoftwareApplication",
        "BreadcrumbList",
        "VideoObject",
        "JobPosting",
        "LocalBusiness",
        "ProfessionalService",
    ];
    let extended: Vec<&String> = {
        let mut v: Vec<&String> = all_types
            .iter()
            .filter(|t| EXTENDED.contains(&t.as_str()))
            .collect();
        v.dedup();
        v
    };
    out.push(match extended.len() {
        0 => fail("schema-type-breadth", "Only basic types, or none."),
        1 | 2 => warn(
            "schema-type-breadth",
            0.5,
            format!(
                "Some extended types: {}; add FAQPage, Product or AggregateRating too.",
                extended
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        _ => pass(
            "schema-type-breadth",
            format!(
                "Extended types: {}.",
                extended
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    });
    let anchors = [
        ("About", &["about"][..]),
        ("Contact", &["contact"][..]),
        ("Privacy", &["privacy"][..]),
    ];
    let found: Vec<&str> = anchors
        .iter()
        .filter(|(_, w)| anchor(w).is_some())
        .map(|(n, _)| *n)
        .collect();
    out.push(match found.len() {
        3 => pass("trust-anchors", "Links About, Contact and Privacy."),
        0 => fail("trust-anchors", "No About, Contact or Privacy link."),
        _ => warn(
            "trust-anchors",
            found.len() as f64 / 3.,
            format!("Links only {}.", found.join(" and ")),
        ),
    });
    let md_alt = h
        .links
        .iter()
        .find(|l| l.has_rel("alternate") && l.mime.as_deref() == Some("text/markdown"))
        .map(|l| l.href.clone())
        .or_else(|| {
            links_in_header(&site.page)
                .into_iter()
                .find(|(_, r, t)| {
                    r.iter().any(|r| r == "alternate") && t.as_deref() == Some("text/markdown")
                })
                .map(|(u, _, _)| u)
        });
    out.push(match md_alt {
        None => fail("markdown-link-alternate", "No Markdown alternate link."),
        Some(url) => {
            let got = site.get(&url).await;
            if is_markdown(site, &got) {
                pass("markdown-link-alternate", format!("{url} serves Markdown."))
                    .with(seen(&got, ""))
            } else {
                warn(
                    "markdown-link-alternate",
                    0.5,
                    format!("{url} doesn't serve Markdown."),
                )
                .with(seen(&got, ""))
            }
        }
    });
    docs_site
}
