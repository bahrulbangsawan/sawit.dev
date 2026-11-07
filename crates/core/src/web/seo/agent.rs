//! The agent-readiness checks: can agents find the site, read it, use it and pay it. Each one is
//! answered by an HTTP request, a DNS lookup or the page itself, here in Rust, on local and
//! production addresses alike. The checks that need outside data (search results, directories)
//! are left to the external scan.

use std::sync::Arc;

use futures::future::join_all;
use serde_json::Value;

use super::checks::{
    fail, find_sitemap, json_ld, na, pass, result, robots, seen, snippet, types_of, warn,
};
use super::mcp::{self, Probe};
use super::openapi;
use super::robots::{AI_SEARCH, AI_USER};
use super::site::Site;
use super::sitemap::{self, Sitemap};
use super::{CheckResult, Evidence, Status};
use crate::web::Fetched;

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

// --- Discovery ---------------------------------------------------------------------------------

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

async fn discovery(site: &Site) -> Vec<CheckResult> {
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

// --- Access ------------------------------------------------------------------------------------

async fn access(site: &Site) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let h = &site.head;
    let server = &site.server;

    let (_, map): (_, Option<Sitemap>) = find_sitemap(site).await;
    out.push(match &map {
        None => na("sitemap-lastmod", "No sitemap."),
        Some(m) if m.entries.is_empty() => na("sitemap-lastmod", "The sitemap is empty."),
        Some(m) => {
            let share = m.lastmod_share();
            let pct = (share * 100.).round();
            if share >= 0.8 {
                pass(
                    "sitemap-lastmod",
                    format!("{pct}% of entries carry lastmod."),
                )
            } else if share > 0. {
                warn(
                    "sitemap-lastmod",
                    share,
                    format!("{pct}% of entries carry lastmod."),
                )
            } else {
                fail("sitemap-lastmod", "No entry carries lastmod.")
            }
        }
    });

    // Content without JavaScript: the server's HTML.
    let chars = server.text.chars().count();
    let ratio = if server.bytes > 0 {
        server.text.len() as f64 / server.bytes as f64
    } else {
        0.
    };
    let h1 = server.headings.iter().any(|x| x.level == 1);
    out.push(if chars >= 500 && h1 && ratio >= 0.05 {
        pass("content-no-js", format!("{chars} characters and an <h1> before scripts run."))
    } else if chars >= 200 {
        warn(
            "content-no-js",
            2. / 3.,
            format!(
                "{chars} characters{}, text is {:.1}% of the HTML (aim for 5%).",
                if h1 { " with an <h1>" } else { ", no <h1>" },
                ratio * 100.
            ),
        )
    } else {
        fail("content-no-js", format!("Only {chars} characters before scripts run: agents that don't run JavaScript see an empty page."))
    }
    .with(seen(&site.page, "")));

    // The agents' user agents.
    let page = site.page.url.clone();
    let agent_gets = join_all(AGENTS.iter().map(|(ua, token)| {
        let page = page.clone();
        async move {
            (
                *token,
                site.get_with(&page, &[("User-Agent", ua), ("Accept", "text/html")])
                    .await,
            )
        }
    }))
    .await;
    let reached: Vec<&str> = agent_gets
        .iter()
        .filter(|(_, g)| g.ok())
        .map(|(t, _)| *t)
        .collect();
    let evidence: Vec<Evidence> = agent_gets.iter().map(|(t, g)| seen(g, *t)).collect();
    out.push(
        match reached.len() {
            n if n == AGENTS.len() => {
                pass("bot-detection", format!("All {n} agents get the page."))
            }
            0 => fail("bot-detection", "No agent's user agent gets the page."),
            n => warn(
                "bot-detection",
                n as f64 / AGENTS.len() as f64,
                format!("{n} of {} agents get the page.", AGENTS.len()),
            ),
        }
        .with_all(evidence.clone()),
    );
    let (_, robots_txt) = robots(site).await;
    let path = crate::web::Url::parse(&page)
        .map(|u| u.path().to_string())
        .unwrap_or_else(|_| "/".into());
    let both: Vec<&str> = agent_gets
        .iter()
        .filter(|(t, g)| g.ok() && robots_txt.as_ref().is_none_or(|r| r.allowed(t, &path)))
        .map(|(t, _)| *t)
        .collect();
    out.push(match both.len() {
        n if n == AGENTS.len() => pass(
            "agent-crawler-reachability",
            "Every agent tested gets the page and is allowed by robots.txt.",
        ),
        0 => fail(
            "agent-crawler-reachability",
            "No agent tested both gets the page and is allowed.",
        ),
        n => warn(
            "agent-crawler-reachability",
            n as f64 / AGENTS.len() as f64,
            format!("Reachable to {}.", both.join(", ")),
        ),
    });
    out.push(match &robots_txt {
        None => pass(
            "robots-agent-user-policy",
            "No robots.txt: nothing blocks them.",
        ),
        Some(r) => {
            let blocked: Vec<&str> = AI_USER
                .iter()
                .copied()
                .filter(|a| !r.allowed(a, &path))
                .collect();
            if blocked.is_empty() {
                pass(
                    "robots-agent-user-policy",
                    "The agents that fetch for a person may read the page.",
                )
            } else {
                fail(
                    "robots-agent-user-policy",
                    format!("robots.txt blocks {}.", blocked.join(", ")),
                )
            }
        }
    });

    // Discovery files.
    let (skills_got, skills) = site.json("/.well-known/agent-skills/index.json").await;
    let agents_md = site.get("/AGENTS.md").await;
    let agents_md = if site.real_file(&agents_md) {
        agents_md
    } else {
        site.get("/agents.md").await
    };
    out.push(if let Some(s) = &skills {
        let n = s["skills"].as_array().map_or(0, Vec::len);
        pass(
            "agent-discovery-file",
            format!("An agent skills index with {n} skills."),
        )
        .with(seen(&skills_got, ""))
    } else if site.real_file(&agents_md) {
        pass("agent-discovery-file", "AGENTS.md answers.").with(seen(&agents_md, ""))
    } else {
        fail(
            "agent-discovery-file",
            "No agent skills index and no AGENTS.md.",
        )
        .with(seen(&skills_got, ""))
    });
    out.push(match &skills {
        None => na("agent-skills-index-v2", "No skills index."),
        Some(s) => {
            let version = s["version"].as_str().or(s["$schema"].as_str()).is_some();
            let list = s["skills"].as_array().cloned().unwrap_or_default();
            let complete = list
                .iter()
                .filter(|k| {
                    k["name"].is_string()
                        && (k["url"].is_string() || k["path"].is_string())
                        && (k["sha256"].is_string() || k["digest"].is_string())
                })
                .count();
            if version && !list.is_empty() && complete == list.len() {
                pass(
                    "agent-skills-index-v2",
                    format!("Versioned, {complete} skills with name, URL and digest."),
                )
            } else {
                warn(
                    "agent-skills-index-v2",
                    0.5,
                    format!(
                        "{complete} of {} skills complete{}.",
                        list.len(),
                        if version { "" } else { ", no version" }
                    ),
                )
            }
        }
    });
    let (card_got, card) = site.json("/.well-known/agent-card.json").await;
    let card = match card {
        Some(c) => Some((card_got, c)),
        None => {
            let (g, c) = site.json("/.well-known/agent.json").await;
            c.map(|c| (g, c))
        }
    };
    out.push(match &card {
        Some((g, c)) if c["name"].is_string() => pass(
            "a2a-agent-card",
            format!("Agent card “{}”.", c["name"].as_str().unwrap_or("")),
        )
        .with(seen(g, "")),
        Some((g, _)) => {
            warn("a2a-agent-card", 0.5, "The agent card has no name.").with(seen(g, ""))
        }
        None => fail("a2a-agent-card", "No /.well-known/agent-card.json."),
    });
    let pricing_md = site.get("/pricing.md").await;
    out.push(if is_markdown(site, &pricing_md) {
        pass("pricing-md", "/pricing.md answers.").with(seen(&pricing_md, ""))
    } else {
        fail("pricing-md", "No /pricing.md.").with(seen(&pricing_md, ""))
    });
    out.push(
        match robots_txt
            .as_ref()
            .and_then(|r| r.directive("schemamap").map(String::from))
        {
            Some(v) => pass("nlweb-schema-feeds", format!("Schemamap: {v}")),
            None => fail("nlweb-schema-feeds", "No Schemamap: line in robots.txt."),
        },
    );
    let (mcp_json_got, mcp_json) = site.json("/.well-known/mcp.json").await;
    let (card_got, server_card) = site.json("/.well-known/mcp/server-card.json").await;
    out.push(if mcp_json.is_some() {
        pass("mcp-well-known-discovery", "/.well-known/mcp.json answers.")
            .with(seen(&mcp_json_got, ""))
    } else if server_card.is_some() {
        pass("mcp-well-known-discovery", "The MCP server card answers.").with(seen(&card_got, ""))
    } else {
        fail(
            "mcp-well-known-discovery",
            "No /.well-known/mcp.json or server card.",
        )
        .with(seen(&mcp_json_got, ""))
    });
    // ?mode=agent.
    let agent_view_url = {
        let mut u = crate::web::Url::parse(&page).ok();
        if let Some(u) = u.as_mut() {
            u.query_pairs_mut().append_pair("mode", "agent");
        }
        u.map(|u| u.to_string()).unwrap_or_default()
    };
    let agent_view = site.get(&agent_view_url).await;
    out.push(if agent_view.ok() && agent_view.body != site.page.body {
        let t = agent_view.text().to_ascii_lowercase();
        let signals = ["llms.txt", "openapi", "mcp", "api", "agent"]
            .iter()
            .filter(|s| t.contains(*s))
            .count();
        if is_markdown(site, &agent_view) || signals >= 3 {
            pass(
                "agent-mode-view",
                format!("?mode=agent answers an agent view ({signals} signals)."),
            )
        } else {
            warn(
                "agent-mode-view",
                0.5,
                "?mode=agent answers something else, with little for agents.",
            )
        }
        .with(seen(&agent_view, ""))
    } else {
        fail("agent-mode-view", "?mode=agent answers the same page.")
    });
    let wanted = [
        "describedby",
        "service-desc",
        "service-doc",
        "api-catalog",
        "alternate",
    ];
    let found: Vec<String> = links_in_header(&site.page)
        .into_iter()
        .flat_map(|(_, rels, _)| rels)
        .filter(|r| wanted.contains(&r.as_str()))
        .collect();
    out.push(
        if found.is_empty() {
            fail("link-headers-discovery", "No discovery Link header.")
        } else {
            pass(
                "link-headers-discovery",
                format!("Link header advertises {}.", found.join(", ")),
            )
        }
        .with(seen(
            &site.page,
            site.page
                .header("link")
                .map(|l| snippet(l, 120))
                .unwrap_or_default(),
        )),
    );
    let index_md = site.get("/index.md").await;
    out.push(if is_markdown(site, &index_md) {
        pass("markdown-url-fallback", "/index.md answers Markdown.").with(seen(&index_md, ""))
    } else {
        fail(
            "markdown-url-fallback",
            "/index.md doesn't answer Markdown.",
        )
        .with(seen(&index_md, ""))
    });
    let sections = join_all(
        ["/docs/llms.txt", "/api/llms.txt", "/blog/llms.txt"]
            .iter()
            .map(|p| site.get(p)),
    )
    .await;
    let found: Vec<&Arc<Fetched>> = sections.iter().filter(|g| site.real_file(g)).collect();
    out.push(match found.first() {
        Some(g) => pass(
            "modular-llms-txt",
            format!("{} section llms.txt files.", found.len()),
        )
        .with(seen(g, "")),
        None => fail(
            "modular-llms-txt",
            "No section llms.txt (/docs, /api, /blog).",
        ),
    });

    let llms = site.get("/llms.txt").await;
    let llms_ok = site.real_file(&llms);
    let llms_text = if llms_ok { llms.text() } else { String::new() };
    out.push(if !llms_ok {
        na("llms-txt-links-resolve", "No llms.txt.")
    } else {
        let base = crate::web::Url::parse(&llms.url).ok();
        let links: Vec<String> = sitemap::markdown_links(&llms_text)
            .into_iter()
            .filter_map(|(_, t)| base.as_ref()?.join(&t).ok().map(|u| u.to_string()))
            .filter(|u| u.starts_with("http"))
            .take(5)
            .collect();
        if links.is_empty() {
            na("llms-txt-links-resolve", "llms.txt has no links.")
        } else {
            let got = join_all(links.iter().map(|u| site.get(u))).await;
            let bad: Vec<&Arc<Fetched>> = got
                .iter()
                .filter(|g| g.status == 0 || g.status >= 400)
                .collect();
            if bad.is_empty() {
                pass(
                    "llms-txt-links-resolve",
                    format!("All {} links tried load.", got.len()),
                )
            } else {
                warn(
                    "llms-txt-links-resolve",
                    1. - bad.len() as f64 / got.len() as f64,
                    format!("{} of {} links don't load.", bad.len(), got.len()),
                )
                .with_all(bad.iter().map(|g| seen(g, "")))
            }
        }
    });

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
    } else if site.real_file(&agents_md) {
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
    // Markdown the site serves: by negotiation, at /index.md, or llms.txt.
    let negotiated = site.get_with(&page, &[("Accept", "text/markdown")]).await;
    let served_md: Option<String> = if negotiated.mime() == "text/markdown" {
        Some(negotiated.text())
    } else if is_markdown(site, &index_md) {
        Some(index_md.text())
    } else {
        None
    };
    out.push(match &served_md {
        None => na(
            "markdown-frontmatter",
            "No Markdown pages served (only llms.txt, if any).",
        ),
        Some(t) if t.trim_start().starts_with("---") && t.contains("title:") => {
            pass("markdown-frontmatter", "Front matter with a title.")
        }
        Some(_) => fail(
            "markdown-frontmatter",
            "Served Markdown has no front matter.",
        ),
    });
    let meta_refresh = h.metas.iter().any(|m| m.key == "refresh");
    let cross = site.page.redirects.iter().any(|(_, to)| {
        crate::web::Url::parse(to)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_string())
            })
            .is_some_and(|h| h != site.host.trim_start_matches("www."))
    });
    let hops = site.page.redirects.len();
    out.push(
        if meta_refresh {
            fail(
                "redirect-hygiene",
                "The page redirects with a meta refresh.",
            )
        } else if cross || hops > 2 {
            warn(
                "redirect-hygiene",
                0.5,
                format!(
                    "{hops} redirects{}.",
                    if cross { ", one to another site" } else { "" }
                ),
            )
        } else {
            pass(
                "redirect-hygiene",
                format!("{hops} redirects, no meta refresh."),
            )
        }
        .with_all(site.page.redirects.iter().map(|(s, to)| Evidence {
            url: to.clone(),
            status: *s,
            note: "redirect".into(),
        })),
    );
    let tokens = h.text.chars().count() / 4;
    out.push(if tokens <= 25_000 {
        pass(
            "page-token-budget",
            format!("About {tokens} tokens of text."),
        )
    } else if tokens <= 50_000 {
        warn(
            "page-token-budget",
            0.5,
            format!("About {tokens} tokens: long for an agent's context."),
        )
    } else {
        fail(
            "page-token-budget",
            format!("About {tokens} tokens: too long for most agents."),
        )
    });
    let md_docs: Vec<&str> = [served_md.as_deref(), llms_ok.then_some(llms_text.as_str())]
        .into_iter()
        .flatten()
        .collect();
    out.push(if md_docs.is_empty() {
        na("code-fence-validity", "No Markdown to read.")
    } else if md_docs.iter().all(|t| t.matches("```").count() % 2 == 0) {
        pass(
            "code-fence-validity",
            format!("Balanced across {} documents.", md_docs.len()),
        )
    } else {
        fail("code-fence-validity", "An unclosed ``` code fence.")
    });
    let login = site.page.url.to_ascii_lowercase();
    out.push(if matches!(site.page.status, 401 | 403) {
        fail(
            "docs-auth-gate",
            format!("The page answers {}.", site.page.status),
        )
    } else if ["/login", "/signin", "/sign-in", "/auth"]
        .iter()
        .any(|p| login.contains(p))
    {
        fail("docs-auth-gate", "The page redirects to a sign-in page.")
    } else {
        pass("docs-auth-gate", "Readable without signing in.")
    });

    let spec = openapi::find(site).await;
    out.push(match &spec {
        Some((got, v)) => pass(
            "openapi-spec",
            format!(
                "{} at {} (version {}).",
                if v["swagger"].is_string() {
                    "Swagger"
                } else {
                    "OpenAPI"
                },
                got.url,
                v["openapi"]
                    .as_str()
                    .or(v["swagger"].as_str())
                    .unwrap_or("?")
            ),
        )
        .with(seen(got, "")),
        None => fail("openapi-spec", "No OpenAPI or Swagger document found."),
    });
    let portal = h.anchors.iter().find(|a| {
        let href = a.href.to_ascii_lowercase();
        href.contains("://docs.")
            || href.contains("/developers")
            || href.ends_with("/docs")
            || href.contains("/docs/")
    });
    out.push(match portal {
        Some(a) => {
            let got = site.get(&a.href).await;
            if got.ok() {
                pass("developer-portal", format!("{} answers.", a.href)).with(seen(&got, ""))
            } else {
                warn(
                    "developer-portal",
                    0.5,
                    format!("{} is linked but doesn't answer.", a.href),
                )
                .with(seen(&got, ""))
            }
        }
        None => match &docs_site {
            Some(g) => pass(
                "developer-portal",
                format!("A docs portal answers at {}.", g.url),
            )
            .with(seen(g, "")),
            None => fail("developer-portal", "No developer or docs portal linked."),
        },
    });
    let (cat_got, api_cat) = site.json("/.well-known/api-catalog").await;
    out.push(match api_cat {
        Some(v) if v["linkset"].is_array() => pass(
            "api-catalog-rfc9727",
            format!(
                "A linkset with {} entries.",
                v["linkset"].as_array().map_or(0, Vec::len)
            ),
        )
        .with(seen(&cat_got, "")),
        Some(_) => warn(
            "api-catalog-rfc9727",
            0.5,
            "/.well-known/api-catalog isn't a linkset.",
        )
        .with(seen(&cat_got, "")),
        None => {
            fail("api-catalog-rfc9727", "No /.well-known/api-catalog.").with(seen(&cat_got, ""))
        }
    });
    let vary = negotiated
        .header("vary")
        .is_some_and(|v| v.to_ascii_lowercase().contains("accept"));
    let md = negotiated.mime() == "text/markdown";
    out.push(
        if md {
            pass(
                "markdown-negotiation",
                "Accept: text/markdown gets Markdown.",
            )
        } else {
            fail(
                "markdown-negotiation",
                format!(
                    "Accept: text/markdown got {}.",
                    if negotiated.mime().is_empty() {
                        "nothing".to_string()
                    } else {
                        negotiated.mime()
                    }
                ),
            )
        }
        .with(seen(&negotiated, "")),
    );
    out.push(match (md, vary) {
        (true, true) => pass(
            "markdown-negotiation-vary",
            "Markdown and HTML on one address, with Vary: Accept.",
        ),
        (true, false) => warn(
            "markdown-negotiation-vary",
            0.5,
            "Markdown by Accept, but no Vary: Accept.",
        ),
        _ => fail("markdown-negotiation-vary", "No Markdown by Accept."),
    });
    let ua_md = site
        .get_with(
            &page,
            &[
                (
                    "User-Agent",
                    "Mozilla/5.0 (compatible; GPTBot/1.2; +https://openai.com/gptbot)",
                ),
                ("Accept", "text/html"),
            ],
        )
        .await;
    out.push(if ua_md.mime() == "text/markdown" {
        pass(
            "agent-ua-markdown",
            "An AI agent's user agent gets Markdown.",
        )
    } else {
        fail(
            "agent-ua-markdown",
            "AI agents get HTML like everyone else (optional).",
        )
    });
    out
}

// --- Usability ---------------------------------------------------------------------------------

async fn usability(site: &Site) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let probe: Option<Probe> = mcp::probe(site).await;
    out.extend(mcp::checks(site, probe.as_ref()).await);

    let spec = openapi::find(site).await;
    let api = site
        .get_with("/api", &[("Accept", "application/json")])
        .await;
    let api_json = api.mime().contains("json");
    out.push(match (&spec, api_json) {
        (Some((g, _)), _) => pass("public-api", format!("Described by {}.", g.url)),
        (None, true) => warn(
            "public-api",
            4. / 7.,
            "/api answers JSON, but no OpenAPI document describes it.",
        )
        .with(seen(&api, "")),
        (None, false) => fail(
            "public-api",
            "No API found (no OpenAPI document, /api doesn't answer JSON).",
        )
        .with(seen(&api, "")),
    });
    // The site's origin first, then the MCP server's and mcp.<domain>.
    let mut origins = vec![site.origin.clone()];
    if let Some(o) = probe.as_ref().and_then(|p| crate::web::origin(&p.endpoint)) {
        origins.push(o);
    }
    if !site.local {
        origins.push(format!(
            "https://mcp.{}",
            site.host.trim_start_matches("www.")
        ));
    }
    origins.dedup();
    let first = |path: &'static str| {
        let origins = origins.clone();
        async move {
            let mut last = None;
            for o in &origins {
                let (g, v) = site.json(&format!("{o}{path}")).await;
                if v.is_some() {
                    return (g, v);
                }
                last.get_or_insert(g);
            }
            (last.unwrap_or_default(), None)
        }
    };
    let (as_got, as_meta) = first("/.well-known/oauth-authorization-server").await;
    let (oidc_got, oidc) = first("/.well-known/openid-configuration").await;
    let auth_server = as_meta.clone().or(oidc.clone());
    out.push(match (&as_meta, &oidc) {
        (Some(v), _) => pass(
            "oauth-support",
            format!(
                "Authorization server metadata (issuer {}).",
                v["issuer"].as_str().unwrap_or("?")
            ),
        )
        .with(seen(&as_got, "")),
        (None, Some(v)) => pass(
            "oauth-support",
            format!(
                "OpenID configuration (issuer {}).",
                v["issuer"].as_str().unwrap_or("?")
            ),
        )
        .with(seen(&oidc_got, "")),
        _ => fail("oauth-support", "No OAuth or OpenID metadata.").with(seen(&as_got, "")),
    });
    let scopes = auth_server
        .as_ref()
        .and_then(|v| v["scopes_supported"].as_array().map(|a| a.len()))
        .filter(|n| *n > 0)
        .or_else(|| {
            spec.as_ref()
                .and_then(|(_, v)| openapi::scope_count(v))
                .filter(|n| *n > 0)
        });
    out.push(match scopes {
        Some(n) => pass("scoped-permissions", format!("{n} scopes declared.")),
        None => fail("scoped-permissions", "No scopes declared."),
    });
    let (prm_got, prm) = first("/.well-known/oauth-protected-resource").await;
    out.push(match &prm {
        Some(v) if v["resource"].is_string() => pass(
            "oauth-protected-resource",
            format!("Resource {}.", v["resource"].as_str().unwrap_or("")),
        )
        .with(seen(&prm_got, "")),
        Some(_) => warn(
            "oauth-protected-resource",
            0.5,
            "The metadata names no resource.",
        )
        .with(seen(&prm_got, "")),
        None => fail(
            "oauth-protected-resource",
            "No /.well-known/oauth-protected-resource.",
        )
        .with(seen(&prm_got, "")),
    });
    let auth_md = site.get("/auth.md").await;
    let auth_md_ok = site.real_file(&auth_md);
    out.push(if auth_md_ok {
        pass(
            "auth-md-exists",
            format!("/auth.md, {} characters.", auth_md.body.len()),
        )
        .with(seen(&auth_md, ""))
    } else {
        fail("auth-md-exists", "No /auth.md.").with(seen(&auth_md, ""))
    });
    out.push(if !auth_md_ok {
        fail("auth-md-structure", "No auth.md.")
    } else {
        let sections = auth_md
            .text()
            .lines()
            .filter(|l| l.starts_with("## "))
            .count();
        match sections {
            0 => fail("auth-md-structure", "auth.md has no ## sections."),
            1..=2 => warn("auth-md-structure", 0.5, format!("{sections} sections.")),
            n => pass("auth-md-structure", format!("{n} sections.")),
        }
    });
    let probes = join_all(
        ["/api", "/mcp", "/api/v1"]
            .iter()
            .map(|p| site.get_with(p, &[("Accept", "application/json")])),
    )
    .await;
    let hinted = probes.iter().find(|g| {
        g.status == 401
            && g.header("www-authenticate")
                .is_some_and(|w| w.to_ascii_lowercase().contains("resource_metadata"))
    });
    out.push(match hinted {
        Some(g) => pass(
            "agent-auth-www-authenticate",
            "A 401 names its resource metadata.",
        )
        .with(seen(g, g.header("www-authenticate").unwrap_or(""))),
        None => fail(
            "agent-auth-www-authenticate",
            "No 401 with WWW-Authenticate resource_metadata at /api, /mcp or /api/v1.",
        ),
    });
    out.push(match (prm.is_some(), auth_server.is_some()) {
        (true, true) => pass(
            "agent-auth-discovery-metadata",
            "Protected-resource and authorization-server metadata both answer.",
        ),
        (false, false) => fail(
            "agent-auth-discovery-metadata",
            "Neither metadata document answers.",
        ),
        _ => warn(
            "agent-auth-discovery-metadata",
            1. / 3.,
            "Only one of the two metadata documents answers.",
        ),
    });
    let wba = site
        .get("/.well-known/http-message-signatures-directory")
        .await;
    out.push(if wba.ok() && !wba.mime().contains("html") {
        pass(
            "web-bot-auth-directory",
            "The signatures directory answers.",
        )
        .with(seen(&wba, ""))
    } else {
        fail(
            "web-bot-auth-directory",
            "No /.well-known/http-message-signatures-directory.",
        )
        .with(seen(&wba, ""))
    });
    let scripts = format!(
        "{}\n{}",
        site.head.inline_scripts, site.server.inline_scripts
    );
    let imperative = [
        "navigator.modelContext",
        "document.modelContext",
        "modelContext.registerTool",
        "registerTool(",
    ]
    .iter()
    .any(|s| scripts.contains(s));
    let declared = site.head.tool_forms + site.server.tool_forms;
    out.push(if imperative {
        pass(
            "webmcp",
            "The page registers tools for agents (modelContext).",
        )
    } else if declared > 0 {
        pass(
            "webmcp",
            format!("{declared} forms declare tools for agents."),
        )
    } else {
        fail("webmcp", "The page declares no tools for agents.")
    });
    let mut missing = site
        .get_with(
            "/api/sawit-probe-not-a-path",
            &[("Accept", "application/json")],
        )
        .await;
    if !((400..500).contains(&missing.status) && missing.mime().contains("json")) && !site.local {
        let other = site
            .get_with(
                &format!(
                    "https://api.{}/api/v1/sawit-probe-not-a-path",
                    site.host.trim_start_matches("www.")
                ),
                &[("Accept", "application/json")],
            )
            .await;
        if (400..500).contains(&other.status) && other.mime().contains("json") {
            missing = other;
        }
    }
    out.push(
        if (400..500).contains(&missing.status) && missing.mime().contains("json") {
            pass(
                "json-error-responses",
                format!("A missing API path answers {} with JSON.", missing.status),
            )
        } else {
            fail(
                "json-error-responses",
                format!(
                    "A missing API path answers {} {}.",
                    missing.status,
                    if missing.mime().is_empty() {
                        "with nothing".into()
                    } else {
                        missing.mime()
                    }
                ),
            )
        }
        .with(seen(&missing, "")),
    );
    let api_answers: Vec<&Arc<Fetched>> = probes
        .iter()
        .chain(std::iter::once(&api))
        .filter(|g| g.status > 0)
        .collect();
    let limited = api_answers.iter().find(|g| {
        g.headers
            .iter()
            .any(|(n, _)| n.starts_with("ratelimit") || n.starts_with("x-ratelimit"))
    });
    out.push(match limited {
        Some(g) => {
            pass("rate-limit-headers", "API answers carry RateLimit headers.").with(seen(g, ""))
        }
        None if (api_json && api.ok()) || spec.is_some() => fail(
            "rate-limit-headers",
            "No RateLimit headers on the API's answers.",
        ),
        None => na("rate-limit-headers", "No API found."),
    });
    let not_found = site.get("/sawit-probe-not-a-page-404").await;
    out.push(
        match not_found.status {
            404 | 410 if !not_found.mime().contains("html") => pass(
                "agent-friendly-404",
                "A missing page answers 404 with text or Markdown.",
            ),
            404 | 410 => warn(
                "agent-friendly-404",
                0.5,
                "A missing page answers 404, as an HTML page with no guidance for agents.",
            ),
            200..=299 => fail(
                "agent-friendly-404",
                format!("A missing page answers {} (a soft 404).", not_found.status),
            ),
            s => warn(
                "agent-friendly-404",
                0.5,
                format!("A missing page answers {s}."),
            ),
        }
        .with(seen(&not_found, "")),
    );

    // The accessibility tree, from the server's HTML.
    let s = &site.server;
    let main = s.landmarks.iter().any(|l| l == "main");
    let landmarks = {
        let mut l: Vec<&String> = s.landmarks.iter().collect();
        l.sort();
        l.dedup();
        l.len()
    };
    let h1 = s.headings.iter().filter(|x| x.level == 1).count();
    let jump = s.headings.windows(2).any(|w| w[1].level > w[0].level + 1);
    let parts = [main, landmarks >= 3, h1 == 1, !jump];
    let have = parts.iter().filter(|p| **p).count();
    out.push(match have {
        4 => pass(
            "ax-document-structure",
            format!("<main>, {landmarks} landmarks, one <h1>, no heading jumps."),
        ),
        0 => fail(
            "ax-document-structure",
            "No <main>, landmarks or a single <h1>.",
        ),
        n => warn(
            "ax-document-structure",
            n as f64 / 4.,
            format!(
                "main={main}, {landmarks} landmarks, {h1} h1{}.",
                if jump { ", a heading jump" } else { "" }
            ),
        ),
    });
    let native = s.controls.len() + s.anchors.len();
    out.push(if native + s.fake_buttons == 0 {
        na("ax-native-controls", "No controls on the page.")
    } else {
        let share = native as f64 / (native + s.fake_buttons) as f64;
        if s.fake_buttons == 0 {
            pass(
                "ax-native-controls",
                format!("{native} native controls, no clickable divs."),
            )
        } else if share >= 0.9 {
            warn(
                "ax-native-controls",
                share,
                format!(
                    "{} clickable divs beside {native} native controls.",
                    s.fake_buttons
                ),
            )
        } else {
            fail(
                "ax-native-controls",
                format!(
                    "{} clickable divs beside {native} native controls.",
                    s.fake_buttons
                ),
            )
        }
    });
    let named = s.controls.iter().filter(|c| c.named).count()
        + s.anchors
            .iter()
            .filter(|a| !a.text.is_empty() || a.label.is_some())
            .count();
    out.push(if native == 0 {
        na("ax-accessible-names", "No controls on the page.")
    } else {
        let share = named as f64 / native as f64;
        if share >= 0.98 {
            pass(
                "ax-accessible-names",
                format!("{named} of {native} controls are named."),
            )
        } else if share >= 0.8 {
            warn(
                "ax-accessible-names",
                share,
                format!("{named} of {native} controls are named."),
            )
        } else {
            fail(
                "ax-accessible-names",
                format!("{named} of {native} controls are named."),
            )
        }
    });
    let fields: Vec<_> = s
        .controls
        .iter()
        .filter(|c| !matches!(c.kind.as_str(), "button" | "submit" | "reset" | "image"))
        .collect();
    out.push(if fields.is_empty() {
        na("ax-form-labeling", "No form fields.")
    } else {
        let labelled = fields.iter().filter(|c| c.named).count();
        if labelled == fields.len() {
            pass(
                "ax-form-labeling",
                format!("All {labelled} fields have labels."),
            )
        } else {
            warn(
                "ax-form-labeling",
                labelled as f64 / fields.len() as f64,
                format!("{labelled} of {} fields have labels.", fields.len()),
            )
        }
    });
    let raw = site.page.text().to_ascii_lowercase();
    let injection = [
        "ignore previous instructions",
        "ignore all previous",
        "disregard previous instructions",
        "ignore your instructions",
        "you are now",
        "system prompt:",
    ]
    .iter()
    .find(|p| raw.contains(*p));
    out.push(match injection {
        Some(p) => fail("ax-tree-injection-safe", format!("The page holds “{p}”.")),
        None => pass(
            "ax-tree-injection-safe",
            "No hidden instructions to agents found.",
        ),
    });
    let ask = site
        .get_with(
            "/ask?query=what%20is%20this%20site",
            &[("Accept", "application/json")],
        )
        .await;
    out.push(match serde_json::from_slice::<Value>(&ask.body) {
        Ok(v) if ask.ok() && (v["results"].is_array() || v["query_id"].is_string()) => {
            pass("nlweb-ask", "/ask answers JSON results.")
        }
        _ => fail("nlweb-ask", "No /ask endpoint."),
    });
    let llms = site.get("/llms.txt").await;
    let lt = if site.real_file(&llms) {
        llms.text().to_ascii_lowercase()
    } else {
        String::new()
    };
    let install = [
        "npx ",
        "brew install",
        "pip install",
        "cargo install",
        "npm install -g",
        "uvx ",
    ]
    .iter()
    .find(|w| lt.contains(*w));
    let mentioned = lt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "cli");
    out.push(match (install, mentioned) {
        (Some(w), _) => pass(
            "cli-tool",
            format!("llms.txt shows how to install a CLI (“{}”).", w.trim()),
        ),
        (None, true) => warn(
            "cli-tool",
            2. / 3.,
            "llms.txt mentions a CLI, but not how to install it.",
        ),
        _ => fail("cli-tool", "No CLI mentioned in llms.txt."),
    });
    out.extend(openapi::checks(spec.as_ref()));
    let gql = site
        .request(
            "POST",
            "/graphql",
            &[
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
            ],
            Some(r#"{"query":"{__schema{types{name description}}}"}"#),
        )
        .await;
    out.push(match serde_json::from_slice::<Value>(&gql.body) {
        Ok(v) if gql.ok() && v["data"]["__schema"].is_object() => {
            let types = v["data"]["__schema"]["types"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let own: Vec<&Value> = types
                .iter()
                .filter(|t| !t["name"].as_str().unwrap_or("_").starts_with("__"))
                .collect();
            let described = own
                .iter()
                .filter(|t| t["description"].as_str().is_some_and(|d| !d.is_empty()))
                .count();
            let share = if own.is_empty() {
                0.
            } else {
                described as f64 / own.len() as f64
            };
            if share >= 0.5 {
                pass(
                    "graphql-endpoint",
                    format!(
                        "/graphql answers; {described} of {} types described.",
                        own.len()
                    ),
                )
            } else {
                warn(
                    "graphql-endpoint",
                    0.5,
                    format!(
                        "/graphql answers; only {described} of {} types described.",
                        own.len()
                    ),
                )
            }
        }
        _ if gql.status == 400 || gql.status == 401 || gql.status == 403 => warn(
            "graphql-endpoint",
            0.5,
            "/graphql answers but doesn't allow introspection.",
        ),
        _ => na("graphql-endpoint", "No GraphQL endpoint at /graphql."),
    });
    out.push(match &probe {
        Some(p)
            if p.resources
                .iter()
                .any(|r| r["uri"].as_str().is_some_and(|u| u.starts_with("ui://"))) =>
        {
            pass("mcp-app-registry", "The MCP server offers ui:// resources.")
        }
        Some(_) => na(
            "mcp-app-registry",
            "The MCP server offers no UI resources (fine for a data service).",
        ),
        None => na("mcp-app-registry", "No MCP server."),
    });
    out
}

// --- Payments ----------------------------------------------------------------------------------

async fn payments(site: &Site) -> Vec<CheckResult> {
    let h = &site.head;
    let types: Vec<String> = json_ld(&h.json_ld).iter().flat_map(types_of).collect();
    let commerce = types.iter().any(|t| {
        matches!(
            t.as_str(),
            "Product" | "Offer" | "AggregateOffer" | "Store" | "OnlineStore"
        )
    }) || h.anchors.iter().any(|a| {
        let href = a.href.to_ascii_lowercase();
        ["/cart", "/checkout"].iter().any(|w| href.contains(w))
    });
    let (x402, mpp, ucp, acp) = futures::join!(
        site.get("/.well-known/x402"),
        site.get("/.well-known/mpp"),
        site.json("/.well-known/ucp"),
        site.json("/.well-known/acp.json"),
    );
    let real = |g: &Fetched| g.ok() && !g.mime().contains("html");
    let any = real(&x402) || real(&mpp) || ucp.1.is_some() || acp.1.is_some();
    if !commerce && !any {
        let why = "No commerce on the site: agent payments are optional here.";
        return vec![
            na("x402-support", why),
            na("mpp-support", why),
            na("ucp-support", why),
            na("acp-support", why),
        ];
    }
    let one = |id: &str, ok: bool, got: &Fetched, what: &str| {
        if ok {
            pass(id, format!("{what} answers.")).with(seen(got, ""))
        } else {
            fail(id, format!("No {what}.")).with(seen(got, ""))
        }
    };
    vec![
        one("x402-support", real(&x402), &x402, "/.well-known/x402"),
        one("mpp-support", real(&mpp), &mpp, "/.well-known/mpp"),
        one("ucp-support", ucp.1.is_some(), &ucp.0, "/.well-known/ucp"),
        one(
            "acp-support",
            acp.1.is_some(),
            &acp.0,
            "/.well-known/acp.json",
        ),
    ]
}
