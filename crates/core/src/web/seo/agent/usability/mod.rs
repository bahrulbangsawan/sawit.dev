//! Usability: can agents use the site? The MCP server, the API and its auth here; the page's
//! accessibility tree and the other interfaces agents use in [`page`].

use std::sync::Arc;

use futures::future::join_all;

use super::super::CheckResult;
use super::super::checks::{fail, na, pass, seen, warn};
use super::super::mcp::{self, Probe};
use super::super::openapi;
use super::super::site::Site;
use super::With;
use crate::web::Fetched;

mod page;

pub(super) async fn usability(site: &Site) -> Vec<CheckResult> {
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

    page::checks(site, &mut out, &probe, &spec).await;
    out
}
