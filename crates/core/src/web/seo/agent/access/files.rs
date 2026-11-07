//! The discovery files agents look for: the skills index, AGENTS.md, the agent card,
//! /pricing.md, the MCP discovery documents, the agent view, the discovery Link headers,
//! /index.md and llms.txt.

use std::sync::Arc;

use futures::future::join_all;

use super::super::super::CheckResult;
use super::super::super::checks::{fail, na, pass, seen, snippet, warn};
use super::super::super::robots::Robots;
use super::super::super::site::Site;
use super::super::super::sitemap;
use super::super::{With, is_markdown, links_in_header};
use crate::web::Fetched;

/// The discovery file checks, in order; answers `(AGENTS.md, /index.md, whether llms.txt is
/// served, its text)` for the checks after them.
pub(super) async fn checks(
    site: &Site,
    out: &mut Vec<CheckResult>,
    page: &str,
    robots_txt: &Option<Robots>,
) -> (Arc<Fetched>, Arc<Fetched>, bool, String) {
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
        let mut u = crate::web::Url::parse(page).ok();
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
    (agents_md, index_md, llms_ok, llms_text)
}
