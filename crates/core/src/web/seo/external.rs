//! The optional external score: an outside agent-readiness scanner's HTTP API, called from Rust
//! for production addresses only, when asked. Its total is shown beside Sawit's, and its results
//! fill the checks Sawit can't run itself (search ranking, directories), marked as external.
//! Local and private addresses are never sent.

use std::time::Duration;

use serde_json::Value;

use super::{CheckResult, ExternalScore, Layer, LayerScore, Status, catalog};
use crate::web::{Request, fetch_with, is_local};

/// The scanner's scan endpoint (its API needs no key to read).
const SCAN_API: &str = "https://ora.ai/api/scan";
/// How long the outside scan is waited for.
const WAIT: Duration = Duration::from_secs(90);

/// Why there is no external score for `url`, or `None` when one may be asked for.
pub fn refused(url: &str) -> Option<String> {
    if is_local(url) {
        return Some("A local or private address: it isn't sent to an outside service.".into());
    }
    None
}

/// Asks the outside scanner; the score, and its results for the checks only it answers.
pub async fn scan(url: &str) -> (ExternalScore, Vec<CheckResult>) {
    if let Some(why) = refused(url) {
        return (missing(why), Vec::new());
    }
    let host = crate::web::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_else(|| url.to_string());
    let body = serde_json::json!({ "url": host }).to_string();
    let got = fetch_with(
        SCAN_API,
        Request::default()
            .method(reqwest::Method::POST)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body)
            .timeout(WAIT),
    )
    .await;
    if !got.ok() {
        let why = got
            .error
            .clone()
            .unwrap_or_else(|| format!("The outside scanner answered {}.", got.status));
        return (missing(why), Vec::new());
    }
    match serde_json::from_slice::<Value>(&got.body) {
        Ok(v) => parse(&v),
        Err(_) => (
            missing("The outside scanner's answer couldn't be read.".into()),
            Vec::new(),
        ),
    }
}

fn missing(why: String) -> ExternalScore {
    ExternalScore {
        missing: Some(why),
        ..Default::default()
    }
}

fn layer_of(id: &str) -> Option<Layer> {
    match id {
        "discovery" => Some(Layer::Discovery),
        "accessibility" | "access" => Some(Layer::Access),
        "usability" => Some(Layer::Usability),
        "payments" => Some(Layer::Payments),
        _ => None,
    }
}

/// Reads the scanner's answer.
pub fn parse(v: &Value) -> (ExternalScore, Vec<CheckResult>) {
    let mut score = ExternalScore {
        score: v["score"].as_u64().map(|s| s as u32),
        grade: v["grade"].as_str().map(String::from),
        ..Default::default()
    };
    if score.score.is_none() {
        score.missing = Some(
            v["error"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| "The outside scanner gave no score.".into()),
        );
    }
    let mut results = Vec::new();
    for l in v["layers"].as_array().into_iter().flatten() {
        let Some(layer) = layer_of(l["id"].as_str().unwrap_or("")) else {
            continue;
        };
        let (s, m) = (
            l["score"].as_f64().unwrap_or(0.),
            l["maxScore"].as_f64().unwrap_or(0.),
        );
        score.layers.push(LayerScore {
            layer,
            name: layer.name().into(),
            score: s,
            max: m,
            percent: super::checks::percent(s, m),
            weight: layer.weight(),
        });
        for c in l["checks"].as_array().into_iter().flatten() {
            let id = c["id"].as_str().unwrap_or("");
            // Only the checks Sawit leaves to the outside scan take its result.
            let Some(def) = catalog::def(id).filter(|d| d.external) else {
                continue;
            };
            let status = match c["status"].as_str().unwrap_or("") {
                "pass" => Status::Pass,
                "warning" | "warn" => Status::Warn,
                "fail" => Status::Fail,
                _ => Status::Na,
            };
            let max = f64::from(def.max);
            let got = c["score"].as_f64().unwrap_or(0.);
            let their_max = c["maxScore"].as_f64().unwrap_or(max).max(1.);
            let mut r = super::checks::result(
                id,
                status,
                got / their_max,
                c["details"].as_str().unwrap_or(""),
            );
            r.external = true;
            results.push(r);
        }
    }
    (score, results)
}
