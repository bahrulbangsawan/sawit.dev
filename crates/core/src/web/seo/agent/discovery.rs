//! Discovery: can agents find the site? Its agent catalog, the AI search crawlers' robots.txt
//! policy, DNS-AID and the MCP server card's branding.

use std::sync::Arc;

use serde_json::Value;

use super::super::checks::{fail, na, pass, result, robots, seen, warn};
use super::super::robots::AI_SEARCH;
use super::super::site::Site;
use super::super::{CheckResult, Status};
use super::{With, link_header};
use crate::web::Fetched;

/// The site's agent catalog: `(where, entries)`, `legacy` when only the old path has it.
async fn catalog(site: &Site) -> Option<(Arc<Fetched>, Vec<Value>, bool)> {
    let entries = |v: &Value| -> Option<Vec<Value>> {
        ["entries", "resources", "items", "catalog"]
            .iter()
            .find_map(|k| v[*k].as_array().cloned())
    };
    let (got, v) = site.json("/.well-known/ard.json").await;
    if let Some(e) = v.as_ref().and_then(entries) {
        return Some((got, e, false));
    }
    // A rel="ard" link or Link header.
    let linked = site
        .head
        .links_rel("ard")
        .next()
        .map(|l| l.href.clone())
        .or_else(|| link_header(&site.page, "ard"));
    if let Some(url) = linked {
        let (got, v) = site.json(&url).await;
        if let Some(e) = v.as_ref().and_then(entries) {
            return Some((got, e, false));
        }
    }
    let (got, v) = site.json("/.well-known/ai-catalog.json").await;
    v.as_ref().and_then(entries).map(|e| (got, e, true))
}
pub(super) async fn discovery(site: &Site) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let cat = catalog(site).await;
    match &cat {
        Some((got, entries, false)) => {
            out.push(pass("ard-catalog", format!("A catalog with {} entries.", entries.len())).with(seen(got, "")))
        }
        Some((got, entries, true)) => out.push(
            warn("ard-catalog", 1., format!("Only the older /.well-known/ai-catalog.json ({} entries): move it to /.well-known/ard.json.", entries.len()))
                .with(seen(got, "")),
        ),
        None => {
            let got = site.get("/.well-known/ard.json").await;
            out.push(fail("ard-catalog", "No /.well-known/ard.json, rel=\"ard\" link or /.well-known/ai-catalog.json.").with(seen(&got, "")))
        }
    }
    let (ai_got, ai) = site.json("/.well-known/ai-catalog.json").await;
    out.push(if ai.is_some() {
        pass(
            "ai-catalog-published",
            "A valid catalog at /.well-known/ai-catalog.json.",
        )
        .with(seen(&ai_got, ""))
    } else {
        na(
            "ai-catalog-published",
            "No /.well-known/ai-catalog.json (bonus only).",
        )
        .with(seen(&ai_got, ""))
    });
    match &cat {
        None => {
            out.push(na("ard-entries-valid", "No catalog to check."));
            out.push(na("ard-trust-manifest", "No catalog to check."));
        }
        Some((_, entries, _)) => {
            let valid = entries
                .iter()
                .filter(|e| {
                    let s = |k: &[&str]| {
                        k.iter()
                            .any(|k| e[*k].as_str().is_some_and(|v| !v.is_empty()))
                    };
                    s(&["id", "identifier", "urn"])
                        && s(&["name", "displayName", "title"])
                        && s(&["type", "mediaType", "media_type", "kind"])
                        && s(&["url", "href", "endpoint", "target"])
                })
                .count();
            out.push(match (valid, entries.len()) {
                (_, 0) => fail("ard-entries-valid", "The catalog has no entries."),
                (v, n) if v == n => pass(
                    "ard-entries-valid",
                    format!("All {n} entries are complete."),
                ),
                (0, n) => fail(
                    "ard-entries-valid",
                    format!("None of {n} entries has an id, name, type and URL."),
                ),
                (v, n) => warn(
                    "ard-entries-valid",
                    v as f64 / n as f64,
                    format!("{v} of {n} entries are complete."),
                ),
            });
            let trusted = entries
                .iter()
                .filter(|e| {
                    e["trustManifest"]
                        .as_object()
                        .is_some_and(|m| !m.is_empty())
                })
                .count();
            out.push(if trusted > 0 {
                pass(
                    "ard-trust-manifest",
                    format!("{trusted} entries carry a trustManifest."),
                )
            } else {
                fail("ard-trust-manifest", "No entry carries a trustManifest.")
            });
        }
    }
    let (r_got, robots_txt) = robots(site).await;
    out.push(match &robots_txt {
        None => pass(
            "robots-ai-policy-quality",
            "No robots.txt: every crawler may read the site.",
        )
        .with(seen(&r_got, "")),
        Some(r) => {
            let blocked: Vec<&str> = AI_SEARCH
                .iter()
                .copied()
                .filter(|a| !r.allowed(a, "/"))
                .collect();
            if blocked.is_empty() {
                pass(
                    "robots-ai-policy-quality",
                    "The AI search crawlers may read the home page.",
                )
            } else if blocked.len() < AI_SEARCH.len() {
                warn(
                    "robots-ai-policy-quality",
                    0.5,
                    format!("robots.txt blocks {}.", blocked.join(", ")),
                )
            } else {
                fail(
                    "robots-ai-policy-quality",
                    format!(
                        "robots.txt blocks every AI search crawler ({}).",
                        blocked.join(", ")
                    ),
                )
            }
            .with(seen(&r_got, ""))
        }
    });
    out.push(dns_aid(site).await);
    let (card_got, card) = site.json("/.well-known/mcp/server-card.json").await;
    out.push(match &card {
        None => na("registry-branding", "No MCP server card."),
        Some(c) => {
            let name = c["name"].is_string() || c["serverInfo"]["name"].is_string();
            let desc = c["description"].is_string();
            let icon = c["icons"].is_array() || c["icon"].is_string() || c["iconUrl"].is_string();
            match [name, desc, icon].iter().filter(|x| **x).count() {
                3 => pass(
                    "registry-branding",
                    "The server card has a name, description and icon.",
                ),
                0 => fail(
                    "registry-branding",
                    "The server card has no name, description or icon.",
                ),
                n => warn(
                    "registry-branding",
                    n as f64 / 3.,
                    "The server card is missing its name, description or icon.",
                ),
            }
            .with(seen(&card_got, ""))
        }
    });
    out
}

/// DNS-AID: a TXT record at `_agents.<domain>`, looked up over DNS-over-HTTPS (production only).
async fn dns_aid(site: &Site) -> CheckResult {
    if site.local {
        return na("dns-aid", "A local address has no public DNS.");
    }
    let host = site.host.trim_start_matches("www.");
    let url = format!("https://cloudflare-dns.com/dns-query?name=_agents.{host}&type=TXT");
    let got = crate::web::fetch_with(
        &url,
        crate::web::Request::default()
            .header("Accept", "application/dns-json")
            .timeout(std::time::Duration::from_secs(6)),
    )
    .await;
    let answers = serde_json::from_slice::<Value>(&got.body)
        .ok()
        .and_then(|v| v["Answer"].as_array().map(|a| a.len()))
        .unwrap_or(0);
    if got.status == 0 {
        return result(
            "dns-aid",
            Status::Skipped,
            0.,
            "The DNS lookup didn't answer.",
        );
    }
    if answers > 0 {
        pass(
            "dns-aid",
            format!("{answers} TXT records at _agents.{host}."),
        )
    } else {
        fail("dns-aid", format!("No TXT record at _agents.{host}."))
    }
}
