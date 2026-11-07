//! The site's MCP server, found from its well-known files (or at `/mcp`) and probed over
//! Streamable HTTP with plain JSON-RPC: `initialize`, `tools/list`, `resources/list`, and an
//! unknown method for its errors. A server that asks for OAuth is noted, not signed in to.

use serde_json::{Value, json};

use super::checks::{fail, na, pass, seen, warn};
use super::site::Site;
use super::{CheckResult, Evidence};
use crate::web::Fetched;

/// What the probe found.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub endpoint: String,
    /// It asked for authorization (401) instead of answering.
    pub needs_auth: bool,
    pub www_authenticate: Option<String>,
    pub initialize: Option<Value>,
    pub tools: Vec<Value>,
    pub resources: Vec<Value>,
    /// The unknown method's answer: a JSON-RPC error with a code and message.
    pub error_ok: bool,
    pub evidence: Vec<Evidence>,
}

/// The JSON-RPC message in an answer: plain JSON, or the first `data:` event of an SSE stream.
pub fn message(got: &Fetched) -> Option<Value> {
    let text = got.text();
    if got.mime().contains("event-stream")
        || text.trim_start().starts_with("event:")
        || text.trim_start().starts_with("data:")
    {
        return text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .find_map(|d| serde_json::from_str::<Value>(d.trim()).ok());
    }
    serde_json::from_str(&text).ok()
}

/// The MCP endpoints the site names, best first, then the usual places.
async fn endpoints(site: &Site) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |u: Option<&str>| {
        if let Some(u) = u.filter(|u| u.starts_with("http")) {
            let u = u.to_string();
            if !out.contains(&u) {
                out.push(u);
            }
        }
    };
    let (_, mcp_json) = site.json("/.well-known/mcp.json").await;
    let (_, card) = site.json("/.well-known/mcp/server-card.json").await;
    for v in [mcp_json, card].into_iter().flatten() {
        for k in ["url", "endpoint", "mcp_url", "serverUrl"] {
            add(v[k].as_str());
        }
        for list in ["servers", "remotes", "endpoints", "transports"] {
            for s in v[list].as_array().into_iter().flatten() {
                add(s["url"].as_str().or(s["endpoint"].as_str()).or(s.as_str()));
            }
        }
        if let Some(map) = v["mcpServers"].as_object() {
            for s in map.values() {
                add(s["url"].as_str());
            }
        }
    }
    add(Some(&site.at("/mcp")));
    add(Some(&site.at("/api/mcp")));
    if !site.local {
        let host = site.host.trim_start_matches("www.");
        add(Some(&format!("https://mcp.{host}/mcp")));
        add(Some(&format!("https://mcp.{host}/")));
    }
    out
}

const ACCEPT: (&str, &str) = ("Accept", "application/json, text/event-stream");
const JSON: (&str, &str) = ("Content-Type", "application/json");

fn rpc(id: u64, method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// Finds and probes the site's MCP server; `None` when nothing answers like one.
pub async fn probe(site: &Site) -> Option<Probe> {
    let init = rpc(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "sawit-scan", "version": env!("CARGO_PKG_VERSION")}}),
    );
    for endpoint in endpoints(site).await.into_iter().take(5) {
        let got = site
            .request(
                "POST",
                &endpoint,
                &[ACCEPT, JSON, ("MCP-Protocol-Version", "2025-06-18")],
                Some(&init),
            )
            .await;
        if got.status == 401 {
            let www = got.header("www-authenticate").map(String::from);
            // Only a 401 that speaks OAuth (Bearer) is taken for an MCP server.
            if www
                .as_deref()
                .is_some_and(|w| w.to_ascii_lowercase().contains("bearer"))
            {
                return Some(Probe {
                    endpoint,
                    needs_auth: true,
                    www_authenticate: www,
                    evidence: vec![seen(&got, "asks for authorization")],
                    ..Default::default()
                });
            }
            continue;
        }
        let Some(answer) = message(&got) else {
            continue;
        };
        let Some(result) = answer
            .get("result")
            .filter(|r| r["protocolVersion"].is_string() || r["serverInfo"].is_object())
        else {
            continue;
        };
        let mut p = Probe {
            endpoint: endpoint.clone(),
            initialize: Some(result.clone()),
            evidence: vec![seen(&got, "initialize")],
            ..Default::default()
        };
        let session = got.header("mcp-session-id").map(String::from);
        let mut headers: Vec<(&str, &str)> =
            vec![ACCEPT, JSON, ("MCP-Protocol-Version", "2025-06-18")];
        if let Some(s) = &session {
            headers.push(("Mcp-Session-Id", s));
        }
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
        let _ = site.request("POST", &endpoint, &headers, Some(&note)).await;
        let (list_tools, list_resources, bogus) = (
            rpc(2, "tools/list", json!({})),
            rpc(3, "resources/list", json!({})),
            rpc(4, "sawit/no-such-method", json!({})),
        );
        let (tools, resources, unknown) = futures::join!(
            site.request("POST", &endpoint, &headers, Some(&list_tools)),
            site.request("POST", &endpoint, &headers, Some(&list_resources)),
            site.request("POST", &endpoint, &headers, Some(&bogus)),
        );
        p.tools = message(&tools)
            .and_then(|m| m["result"]["tools"].as_array().cloned())
            .unwrap_or_default();
        p.resources = message(&resources)
            .and_then(|m| m["result"]["resources"].as_array().cloned())
            .unwrap_or_default();
        p.error_ok = message(&unknown)
            .is_some_and(|m| m["error"]["code"].is_number() && m["error"]["message"].is_string());
        p.evidence
            .push(seen(&tools, format!("tools/list: {} tools", p.tools.len())));
        return Some(p);
    }
    None
}

/// Whether a tool name is generic.
fn generic(name: &str) -> bool {
    matches!(
        name,
        "run" | "query" | "tool" | "do" | "execute" | "call" | "action" | "get" | "search"
    )
}

/// The MCP checks from a probe.
pub async fn checks(site: &Site, probe: Option<&Probe>) -> Vec<CheckResult> {
    let ids = [
        "mcp-transport-modern",
        "mcp-server-identity",
        "mcp-tool-listing",
        "mcp-tool-descriptions",
        "mcp-param-schemas",
        "mcp-tool-naming",
        "mcp-tool-annotations",
        "mcp-error-handling",
        "mcp-resource-listing",
    ];
    let (card_got, card) = site.json("/.well-known/mcp/server-card.json").await;
    let card_check = match &card {
        Some(c) if (c["name"].is_string() || c["serverInfo"]["name"].is_string()) => {
            pass("mcp-server-card", "A server card with a name.").with_ev(seen(&card_got, ""))
        }
        Some(_) => warn("mcp-server-card", 0.5, "The server card has no name.")
            .with_ev(seen(&card_got, "")),
        None => fail("mcp-server-card", "No /.well-known/mcp/server-card.json.")
            .with_ev(seen(&card_got, "")),
    };
    let Some(p) = probe else {
        let mut out = vec![fail(
            "mcp-server",
            "No MCP server found (well-known files, /mcp, /api/mcp).",
        )];
        out.extend(ids.iter().map(|id| na(id, "No MCP server.")));
        out.push(card_check);
        out.push(na("mcp-auth-mechanism", "No MCP server."));
        out.push(na("mcp-oauth-metadata", "No MCP server."));
        out.push(na("mcp-pkce-s256", "No MCP server."));
        return out;
    };
    let mut out = Vec::new();
    if p.needs_auth {
        out.push(
            pass(
                "mcp-server",
                format!("{} asks for OAuth before it answers.", p.endpoint),
            )
            .with_all_ev(p.evidence.clone()),
        );
        out.extend(ids.iter().map(|id| na(id, "Behind sign-in: not read.")));
        out.push(card_check);
        out.push(match &p.www_authenticate {
            Some(w) => pass(
                "mcp-auth-mechanism",
                format!("OAuth: {}", super::checks::snippet(w, 100)),
            ),
            None => warn("mcp-auth-mechanism", 0.5, "401 without WWW-Authenticate."),
        });
        // The protected-resource metadata on the server's origin, and its authorization server.
        let origin = crate::web::origin(&p.endpoint).unwrap_or_default();
        let (prm_got, prm) = site
            .json(&format!("{origin}/.well-known/oauth-protected-resource"))
            .await;
        out.push(match &prm {
            Some(_) => pass("mcp-oauth-metadata", "Protected-resource metadata answers.")
                .with_ev(seen(&prm_got, "")),
            None => fail(
                "mcp-oauth-metadata",
                "No protected-resource metadata on the server's origin.",
            )
            .with_ev(seen(&prm_got, "")),
        });
        let auth = prm
            .as_ref()
            .and_then(|v| v["authorization_servers"][0].as_str().map(String::from))
            .unwrap_or(origin);
        let (_, meta) = site
            .json(&format!(
                "{}/.well-known/oauth-authorization-server",
                auth.trim_end_matches('/')
            ))
            .await;
        let s256 = meta
            .as_ref()
            .and_then(|m| m["code_challenge_methods_supported"].as_array().cloned())
            .is_some_and(|a| a.iter().any(|v| v == "S256"));
        out.push(if s256 {
            pass("mcp-pkce-s256", "S256 supported.")
        } else {
            fail(
                "mcp-pkce-s256",
                "S256 isn't listed in code_challenge_methods_supported.",
            )
        });
        return out;
    }
    let init = p.initialize.clone().unwrap_or_default();
    let name = init["serverInfo"]["name"].as_str().unwrap_or("?");
    let version = init["serverInfo"]["version"].as_str().unwrap_or("?");
    out.push(
        pass(
            "mcp-server",
            format!(
                "{} answers: {name} {version} (protocol {}).",
                p.endpoint,
                init["protocolVersion"].as_str().unwrap_or("?")
            ),
        )
        .with_all_ev(p.evidence.clone()),
    );
    out.push(pass("mcp-transport-modern", "Streamable HTTP."));
    let has_name =
        init["serverInfo"]["name"].is_string() && init["serverInfo"]["version"].is_string();
    out.push(match (has_name, init["instructions"].is_string()) {
        (true, true) => pass(
            "mcp-server-identity",
            format!("{name} {version}, with instructions."),
        ),
        (true, false) => warn(
            "mcp-server-identity",
            0.5,
            format!("{name} {version}, no instructions."),
        ),
        _ => fail("mcp-server-identity", "initialize doesn't name the server."),
    });
    let n = p.tools.len();
    out.push(match n {
        0 => fail("mcp-tool-listing", "tools/list answers no tools."),
        1..=30 => pass("mcp-tool-listing", format!("{n} tools.")),
        _ => warn(
            "mcp-tool-listing",
            1. / 3.,
            format!("{n} tools: a lot to choose from."),
        ),
    });
    if n == 0 {
        for id in [
            "mcp-tool-descriptions",
            "mcp-param-schemas",
            "mcp-tool-naming",
            "mcp-tool-annotations",
        ] {
            out.push(na(id, "No tools."));
        }
    } else {
        let described = p
            .tools
            .iter()
            .filter(|t| t["description"].as_str().is_some_and(|d| d.len() >= 30))
            .count();
        out.push(share(
            "mcp-tool-descriptions",
            described,
            n,
            "tools have descriptions of 30+ characters",
        ));
        let schemas = p
            .tools
            .iter()
            .filter(|t| {
                t["inputSchema"]["type"] == "object" && (t["inputSchema"]["properties"].is_object())
            })
            .count();
        out.push(share(
            "mcp-param-schemas",
            schemas,
            n,
            "tools have typed input schemas",
        ));
        let names: Vec<&str> = p.tools.iter().filter_map(|t| t["name"].as_str()).collect();
        let snake = names.iter().all(|n| {
            n.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.'
            })
        });
        let camel = names
            .iter()
            .all(|n| !n.contains('_') && n.chars().next().is_some_and(|c| c.is_ascii_lowercase()));
        let generic_names: Vec<&&str> = names.iter().filter(|n| generic(n)).collect();
        out.push(if (snake || camel) && generic_names.is_empty() {
            pass(
                "mcp-tool-naming",
                format!("All {n} names follow one convention."),
            )
        } else if !generic_names.is_empty() {
            warn(
                "mcp-tool-naming",
                0.5,
                format!(
                    "Generic names: {}.",
                    generic_names
                        .iter()
                        .map(|s| **s)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        } else {
            warn("mcp-tool-naming", 0.5, "Names mix conventions.")
        });
        let annotated = p
            .tools
            .iter()
            .filter(|t| t["annotations"].is_object())
            .count();
        out.push(share(
            "mcp-tool-annotations",
            annotated,
            n,
            "tools have annotations",
        ));
    }
    out.push(if p.error_ok {
        pass(
            "mcp-error-handling",
            "An unknown method answers a JSON-RPC error.",
        )
    } else {
        fail(
            "mcp-error-handling",
            "An unknown method doesn't answer a proper JSON-RPC error.",
        )
    });
    out.push(if p.resources.is_empty() {
        fail("mcp-resource-listing", "resources/list answers none.")
    } else {
        pass(
            "mcp-resource-listing",
            format!("{} resources.", p.resources.len()),
        )
    });
    out.push(card_check);
    out.push(pass("mcp-auth-mechanism", "Public: no sign-in needed."));
    out.push(na("mcp-oauth-metadata", "The server is public."));
    out.push(na("mcp-pkce-s256", "The server is public."));
    out
}

fn share(id: &str, have: usize, of: usize, what: &str) -> CheckResult {
    let details = format!("{have} of {of} {what}.");
    if have == of {
        pass(id, details)
    } else if have * 2 >= of {
        warn(id, have as f64 / of as f64, details)
    } else {
        fail(id, details)
    }
}

trait Ev {
    fn with_ev(self, e: Evidence) -> Self;
    fn with_all_ev(self, e: Vec<Evidence>) -> Self;
}

impl Ev for CheckResult {
    fn with_ev(mut self, e: Evidence) -> Self {
        self.evidence.push(e);
        self
    }
    fn with_all_ev(mut self, e: Vec<Evidence>) -> Self {
        self.evidence.extend(e);
        self
    }
}
