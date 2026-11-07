//! The page as agents use it: its accessibility tree, hidden instructions, NLWeb's /ask, a CLI,
//! the OpenAPI document's quality, GraphQL and MCP apps.

use std::sync::Arc;

use serde_json::Value;

use super::super::super::CheckResult;
use super::super::super::checks::{fail, na, pass, warn};
use super::super::super::mcp::Probe;
use super::super::super::openapi;
use super::super::super::site::Site;
use crate::web::Fetched;

/// The page and interface checks, in order, after the API and auth ones.
pub(super) async fn checks(
    site: &Site,
    out: &mut Vec<CheckResult>,
    probe: &Option<Probe>,
    spec: &Option<(Arc<Fetched>, Value)>,
) {
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
}
