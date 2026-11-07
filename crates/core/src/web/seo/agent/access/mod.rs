//! Access: can agents read the site? The sitemap's dates, content without JavaScript, the
//! agents' user agents and robots.txt here; the discovery files ([`files`]), the structured data
//! ([`schema`]) and the Markdown and API documents ([`markdown`]) in their own files.

use futures::future::join_all;

use super::super::checks::{fail, find_sitemap, na, pass, robots, seen, warn};
use super::super::robots::AI_USER;
use super::super::site::Site;
use super::super::sitemap::Sitemap;
use super::super::{CheckResult, Evidence};
use super::{AGENTS, With};

mod files;
mod markdown;
mod schema;

pub(super) async fn access(site: &Site) -> Vec<CheckResult> {
    let mut out = Vec::new();
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

    let (agents_md, index_md, llms_ok, llms_text) =
        files::checks(site, &mut out, &page, &robots_txt).await;
    let docs_site = schema::checks(site, &mut out, &agents_md, &llms_text).await;
    markdown::checks(
        site, &mut out, &page, &index_md, llms_ok, &llms_text, &docs_site,
    )
    .await;
    out
}
