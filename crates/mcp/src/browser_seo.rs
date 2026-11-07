//! `browser_seo`: the SEO and agent-readiness scan (`sawit_core::web::seo`), the same report the
//! browser's SEO panel shows. The scan runs here, in Rust; the app only hands over the page on
//! show and its rendered HTML, so a long scan never waits on the app. With `url`, the app isn't
//! needed at all.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::browser::BrowserOp;
use sawit_core::web::seo::{self, ScanOptions, ScanType};

use crate::app::pretty;
use crate::{Sawit, clip, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SeoArgs {
    #[schemars(
        description = "Address to scan (local addresses work: localhost, 127.0.0.1, *.local, LAN, any port). Default: the page open in the browser tab, read as rendered"
    )]
    pub url: Option<String>,
    #[schemars(description = "Only these check ids (default all; the report lists every id)")]
    #[serde(default)]
    pub checks: Vec<String>,
    #[schemars(
        description = "Which checks apply: all (default), content (a content site) or api (an API or app)"
    )]
    pub scan_type: Option<String>,
    #[schemars(
        description = "Also fetch an outside scanner's score for a public address (never sent for local or private ones)"
    )]
    #[serde(default)]
    pub external: bool,
    #[schemars(description = "text (default) or json (the whole report)")]
    pub format: Option<String>,
    #[serde(flatten)]
    pub at: crate::browser::At,
}

/// The scan's options from the agent's arguments.
pub(crate) fn options(a: &SeoArgs) -> Result<ScanOptions, String> {
    let scan_type = match a.scan_type.as_deref() {
        None => ScanType::All,
        Some(t) => ScanType::parse(t)
            .ok_or_else(|| format!("Unknown scan_type “{t}”. Use all, content or api."))?,
    };
    let unknown: Vec<&str> = a
        .checks
        .iter()
        .map(String::as_str)
        .filter(|c| seo::catalog::def(c).is_none())
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "Unknown checks: {}. Leave checks out to run them all; the report lists every id.",
            unknown.join(", ")
        ));
    }
    match a.format.as_deref() {
        None | Some("text") | Some("json") => {}
        Some(f) => return Err(format!("Unknown format “{f}”. Use text or json.")),
    }
    Ok(ScanOptions {
        scan_type,
        checks: a.checks.clone(),
        external: a.external,
        rendered_html: None,
    })
}

#[tool_router(router = browser_seo_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Scan a page for SEO and agent readiness, as the browser's SEO panel does: title, description, canonical, robots, hreflang, headings, favicons, manifest, Open Graph and X tags with the share image's size, JSON-LD, sitemaps, llms.txt, alt text, broken links;, and the agent-readiness checks (discovery files, Markdown negotiation, AI bot rules, MCP endpoints, OAuth metadata, OpenAPI, WebMCP, accessibility tree), scored per layer with a total, grade and Essentials score. Each check gives pass/warning/fail, evidence and a fix. Default: the page open in the browser tab"
    )]
    async fn browser_seo(&self, Parameters(a): Parameters<SeoArgs>) -> CallToolResult {
        let mut options = tri!(options(&a));
        let url = match a.url.clone().filter(|u| !u.trim().is_empty()) {
            Some(url) => normalize(&url),
            None => {
                let state = tri!(self.browser(&a.at, BrowserOp::State).await);
                let url = state["url"].as_str().unwrap_or_default().to_string();
                if !url.starts_with("http") {
                    return fail(format!(
                        "The browser tab has no web page ({url}). Open one with browser_navigate, or pass url."
                    ));
                }
                // The page as rendered, when the page agent answers; else the server's HTML.
                let html = self
                    .browser(
                        &a.at,
                        BrowserOp::Evaluate {
                            script: "document.documentElement.outerHTML".into(),
                        },
                    )
                    .await
                    .ok()
                    .and_then(|v| v["value"].as_str().map(String::from));
                options.rendered_html = html;
                url
            }
        };
        let report = seo::scan(&url, options).await;
        if a.format.as_deref() == Some("json") {
            return ok(clip(
                pretty(&serde_json::to_value(&report).unwrap_or_default()),
                120_000,
            ));
        }
        ok(clip(seo::report_text(&report), 60_000))
    }
}

/// `example.com` → `https://example.com`; `localhost:3000` → `http://localhost:3000`.
pub(crate) fn normalize(url: &str) -> String {
    let url = url.trim();
    if url.contains("://") {
        return url.to_string();
    }
    let local = url.starts_with("localhost") || url.starts_with("127.") || url.starts_with("[::1]");
    format!("{}://{url}", if local { "http" } else { "https" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(json: serde_json::Value) -> SeoArgs {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn options_check_scan_type_checks_and_format() {
        let o = options(&args(
            serde_json::json!({"scan_type": "content", "checks": ["title"]}),
        ))
        .unwrap();
        assert_eq!(o.scan_type, ScanType::Content);
        assert!(options(&args(serde_json::json!({"scan_type": "shop"}))).is_err());
        let e = options(&args(serde_json::json!({"checks": ["nope"]}))).unwrap_err();
        assert!(e.contains("nope"));
        assert!(options(&args(serde_json::json!({"format": "xml"}))).is_err());
    }

    #[test]
    fn addresses_get_a_scheme() {
        assert_eq!(normalize("kyzn.life"), "https://kyzn.life");
        assert_eq!(normalize("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize("http://x.dev/"), "http://x.dev/");
    }
}
