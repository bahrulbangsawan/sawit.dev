//! The site's OpenAPI document, found at the usual places or through its API catalog, and read
//! for how well its operations suit agents: ids, descriptions, typed schemas, errors, versions,
//! pagination, idempotency, batches and async jobs. JSON documents only (YAML is found, noted,
//! and not read).

use std::sync::Arc;

use serde_json::Value;

use super::CheckResult;
use super::checks::{fail, na, pass, warn};
use super::site::Site;
use crate::web::Fetched;

const PLACES: &[&str] = &[
    "/openapi.json",
    "/.well-known/openapi.json",
    "/api/openapi.json",
    "/swagger.json",
    "/api/swagger.json",
    "/v1/openapi.json",
    "/api-docs",
    "/docs/openapi.json",
];

/// The OpenAPI (or Swagger) document, with where it answered.
pub async fn find(site: &Site) -> Option<(Arc<Fetched>, Value)> {
    let is_spec = |v: &Value| {
        (v["openapi"].is_string() || v["swagger"].is_string()) && v["paths"].is_object()
    };
    // The API catalog's service-desc links first.
    let (_, cat) = site.json("/.well-known/api-catalog").await;
    let mut places: Vec<String> = cat
        .as_ref()
        .and_then(|c| c["linkset"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .flat_map(|l| l["service-desc"].as_array().cloned().unwrap_or_default())
        .filter_map(|d| d["href"].as_str().map(String::from))
        .collect();
    places.extend(PLACES.iter().map(|p| p.to_string()));
    for place in places.iter().take(10) {
        let (got, v) = site.json(place).await;
        if let Some(v) = v.filter(is_spec) {
            return Some((got, v));
        }
    }
    None
}

/// How many OAuth scopes the document's security schemes declare.
pub fn scope_count(spec: &Value) -> Option<usize> {
    let schemes = spec["components"]["securitySchemes"]
        .as_object()
        .or(spec["securityDefinitions"].as_object())?;
    let mut n = 0;
    for s in schemes.values() {
        if let Some(flows) = s["flows"].as_object() {
            for f in flows.values() {
                n += f["scopes"].as_object().map_or(0, |m| m.len());
            }
        }
        n += s["scopes"].as_object().map_or(0, |m| m.len());
    }
    Some(n)
}

/// Every operation: `(path, method, operation)`.
fn operations(spec: &Value) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    for (path, item) in spec["paths"].as_object().into_iter().flatten() {
        for method in ["get", "post", "put", "patch", "delete"] {
            if let Some(op) = item.get(method) {
                out.push((path.clone(), method.to_string(), op.clone()));
            }
        }
    }
    out
}

fn ratio(id: &str, have: usize, of: usize, what: &str) -> CheckResult {
    let details = format!("{have} of {of} operations {what}.");
    if of == 0 {
        return na(id, "The document has no operations.");
    }
    let share = have as f64 / of as f64;
    if share >= 0.95 {
        pass(id, details)
    } else if share >= 0.5 {
        warn(id, share, details)
    } else {
        fail(id, details)
    }
}

/// The checks that read the document; not applicable without one.
pub fn checks(spec: Option<&(Arc<Fetched>, Value)>) -> Vec<CheckResult> {
    const IDS: &[&str] = &[
        "api-schema-analysis",
        "response-schema-coverage",
        "api-error-model",
        "api-versioning-policy",
        "pagination-shape",
        "function-calling-compat",
        "idempotency-key-support",
        "batch-endpoints",
        "async-job-pattern",
    ];
    let Some((_, spec)) = spec else {
        // Without a document an agent can't map the API to calls: those two fail, the rest
        // don't apply.
        return IDS
            .iter()
            .map(|id| match *id {
                "api-schema-analysis" | "function-calling-compat" => fail(
                    id,
                    "No API schema found: agents can't discover the endpoints.",
                ),
                _ => na(id, "No OpenAPI document."),
            })
            .collect();
    };
    let ops = operations(spec);
    let n = ops.len();
    let mut out = Vec::new();
    let described = ops
        .iter()
        .filter(|(_, _, o)| {
            o["operationId"].is_string()
                && (o["summary"].is_string() || o["description"].is_string())
        })
        .count();
    out.push(ratio(
        "api-schema-analysis",
        described,
        n,
        "have an operationId and a description",
    ));
    let typed = ops
        .iter()
        .filter(|(_, _, o)| {
            o["responses"].as_object().is_some_and(|r| {
                r.iter().any(|(code, resp)| {
                    code.starts_with('2')
                        && (resp["content"].is_object()
                            || resp["schema"].is_object()
                            || resp["$ref"].is_string())
                })
            })
        })
        .count();
    out.push(ratio(
        "response-schema-coverage",
        typed,
        n,
        "define typed responses",
    ));
    let error_refs = ops
        .iter()
        .filter(|(_, _, o)| {
            o["responses"].as_object().is_some_and(|r| {
                r.iter().any(|(code, resp)| {
                    (code.starts_with('4') || code.starts_with('5') || code == "default")
                        && (resp.to_string().contains("$ref") || resp["content"].is_object())
                })
            })
        })
        .count();
    let has_error_schema = spec["components"]["schemas"].as_object().is_some_and(|s| {
        s.keys().any(|k| {
            k.to_ascii_lowercase().contains("error") || k.to_ascii_lowercase().contains("problem")
        })
    });
    out.push(if has_error_schema && error_refs > 0 {
        pass(
            "api-error-model",
            format!("An error schema, used by {error_refs} operations."),
        )
    } else if has_error_schema || error_refs > 0 {
        warn(
            "api-error-model",
            0.5,
            "Errors are described, but not with one shared schema.",
        )
    } else {
        fail("api-error-model", "No error schema.")
    });
    let text = spec.to_string().to_ascii_lowercase();
    let versioned = ops
        .iter()
        .any(|(p, _, _)| p.starts_with("/v1") || p.starts_with("/v2") || p.contains("/v1/"))
        || spec["servers"].to_string().contains("/v1")
        || text.contains("api-version");
    let deprecation = text.contains("\"deprecated\":true")
        || text.contains("\"sunset\":")
        || text.contains("\"deprecation\":");
    out.push(match (versioned, deprecation) {
        (true, true) => pass(
            "api-versioning-policy",
            "Versioned, with a deprecation policy.",
        ),
        (true, false) => warn(
            "api-versioning-policy",
            2. / 3.,
            "Versioned, but no deprecation or Sunset policy.",
        ),
        _ => fail("api-versioning-policy", "No API versioning found."),
    });
    let cursor = text.contains("\"cursor\"")
        || text.contains("next_cursor")
        || text.contains("\"after\"")
        || text.contains("starting_after");
    let offset =
        text.contains("\"offset\"") || text.contains("\"page\"") || text.contains("\"per_page\"");
    out.push(match (cursor, offset) {
        (true, _) => pass("pagination-shape", "Cursor pagination."),
        (false, true) => warn(
            "pagination-shape",
            0.5,
            "Offset or page pagination: cursors suit agents better.",
        ),
        _ => fail("pagination-shape", "No pagination found."),
    });
    let fc = ops
        .iter()
        .filter(|(_, m, o)| {
            let _ = m;
            let params_typed = o["parameters"].as_array().is_some_and(|p| {
                !p.is_empty()
                    && p.iter()
                        .all(|x| x["schema"].is_object() || x["$ref"].is_string())
            });
            o["operationId"].is_string()
                && (o["requestBody"]["content"].to_string().contains("schema") || params_typed)
        })
        .count();
    out.push(ratio(
        "function-calling-compat",
        fc,
        n,
        "map to tool calls (operationId and typed input)",
    ));
    out.push(if text.contains("idempotency-key") {
        pass("idempotency-key-support", "Idempotency-Key is accepted.")
    } else {
        fail(
            "idempotency-key-support",
            "No Idempotency-Key header in the document.",
        )
    });
    let batch = ops.iter().any(|(p, _, _)| {
        let p = p.to_ascii_lowercase();
        p.contains("batch") || p.contains("bulk")
    });
    out.push(if batch {
        pass("batch-endpoints", "Batch or bulk operations.")
    } else {
        fail("batch-endpoints", "No batch or bulk operations.")
    });
    let jobs = ops.iter().any(|(p, _, o)| {
        o["responses"]["202"].is_object()
            || p.to_ascii_lowercase().contains("/jobs")
            || p.to_ascii_lowercase().contains("/operations")
    });
    out.push(if jobs {
        pass(
            "async-job-pattern",
            "Long work answers 202 or has jobs to poll.",
        )
    } else {
        fail("async-job-pattern", "No async job pattern.")
    });
    out
}
