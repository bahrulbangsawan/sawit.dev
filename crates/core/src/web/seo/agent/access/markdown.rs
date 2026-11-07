//! The Markdown the site serves and its API documents: front matter, redirects, the token
//! budget, code fences, sign-in gates, OpenAPI, the developer portal, the API catalog and
//! Markdown by negotiation or user agent.

use std::sync::Arc;

use super::super::super::checks::{fail, na, pass, seen, warn};
use super::super::super::openapi;
use super::super::super::site::Site;
use super::super::super::{CheckResult, Evidence};
use super::super::{With, is_markdown};
use crate::web::Fetched;

/// The Markdown and API document checks, in order.
pub(super) async fn checks(
    site: &Site,
    out: &mut Vec<CheckResult>,
    page: &str,
    index_md: &Fetched,
    llms_ok: bool,
    llms_text: &str,
    docs_site: &Option<Arc<Fetched>>,
) {
    let h = &site.head;
    // Markdown the site serves: by negotiation, at /index.md, or llms.txt.
    let negotiated = site.get_with(page, &[("Accept", "text/markdown")]).await;
    let served_md: Option<String> = if negotiated.mime() == "text/markdown" {
        Some(negotiated.text())
    } else if is_markdown(site, index_md) {
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
    let md_docs: Vec<&str> = [served_md.as_deref(), llms_ok.then_some(llms_text)]
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
            page,
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
}
