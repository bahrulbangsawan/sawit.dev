//! Calling Local Explorer's API: the methods it takes, paths kept under it, writes that need
//! confirming, and the Workers a dev server runs.

use std::net::SocketAddr;

use serde_json::Value;

use super::http::{http, refuse_remote};
use super::{API_PATH, BODY_LIMIT, REQUEST_TIMEOUT};

/// One of Local Explorer's API calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    /// `GET`, `post`, ….
    pub fn parse(text: &str) -> Result<Method, String> {
        Ok(match text.trim().to_ascii_uppercase().as_str() {
            "" | "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            other => {
                return Err(format!(
                    "Local Explorer's API takes GET, POST, PUT, PATCH or DELETE, not {other}."
                ));
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Whether the call can change the dev server's data: anything but `GET`. Even a `POST` that
    /// only reads (a D1 `SELECT`, the observability query) is counted, as nothing tells them apart
    /// before it runs.
    pub fn writes(self) -> bool {
        self != Method::Get
    }
}

/// Refuses a call that can change data, unless the caller confirmed it: it writes to the dev
/// server's local KV, R2, D1, Durable Objects or Workflows (or, through a binding marked remote,
/// to the Cloudflare account's), and nothing undoes that.
pub fn check_confirmed(method: Method, confirm: bool) -> Result<(), String> {
    if method.writes() && !confirm {
        return Err(format!(
            "Refused: {} can change the dev server's data (KV keys, R2 objects, D1 rows, Durable Objects, Workflow instances; a binding marked remote reaches your Cloudflare account), which can't be undone. Read first with GET, then pass confirm: true.",
            method.as_str()
        ));
    }
    Ok(())
}

/// The request path for `path`, an API path given from the API's root (`storage/kv/namespaces`,
/// `local/scheduled?worker=api`) or whole (`/cdn-cgi/local/explorer/api/d1/database`): always
/// under the API. Refused: an absolute URL, a `.` or `..` segment written plainly or
/// percent-encoded (once or more), a backslash, whitespace, control characters or a fragment.
pub fn api_path(path: &str) -> Result<String, String> {
    let given = path.trim();
    let refuse = |why: &str| Err(format!("Refused path “{given}”: {why}."));
    if given.contains("://") || given.starts_with("//") {
        return refuse("give a path under the API, not a URL");
    }
    if given
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\' || c == '#')
    {
        return refuse("no spaces, backslashes, control characters or fragments");
    }
    let rest = match given.strip_prefix(API_PATH) {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '?']) => rest,
        _ => given,
    };
    let rest = rest.trim_start_matches('/');
    let route = rest.split('?').next().unwrap_or_default();
    // Decoded again and again, so `%252e%252e` can't come out as `..` at a server that decodes
    // twice.
    let mut decoded = route.to_string();
    for _ in 0..4 {
        let segments_bad = decoded
            .split('/')
            .any(|segment| segment == ".." || segment == ".");
        if segments_bad || decoded.contains('\\') {
            return refuse("it can't leave the Local Explorer API (no . or .. segments)");
        }
        let next = percent_decode(&decoded);
        if next == decoded {
            break;
        }
        decoded = next;
    }
    Ok(if rest.is_empty() {
        API_PATH.to_string()
    } else if rest.starts_with('?') {
        format!("{API_PATH}{rest}")
    } else {
        format!("{API_PATH}/{rest}")
    })
}

/// `%xx` escapes decoded, as bytes, then read as UTF-8 with anything else replaced.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        if bytes[ix] == b'%'
            && let Some(hex) = text.get(ix + 1..ix + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            ix += 3;
            continue;
        }
        out.push(bytes[ix]);
        ix += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What an API call answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// The answer was longer than [`BODY_LIMIT`] and `body` holds its start.
    pub truncated: bool,
}

/// Calls Local Explorer's API on the dev server at `addr`: `method` on `path` ([`api_path`]),
/// with `body` and its content type. Only loopback addresses are called. It doesn't ask for
/// confirmation: callers check writes with [`check_confirmed`] first.
pub async fn request(
    addr: SocketAddr,
    method: Method,
    path: &str,
    body: Option<(&[u8], &str)>,
) -> Result<Response, String> {
    refuse_remote(addr)?;
    let path = api_path(path)?;
    http(
        addr,
        method.as_str(),
        &path,
        body,
        REQUEST_TIMEOUT,
        BODY_LIMIT,
    )
    .await
}

/// A Worker the dev server runs, as `GET /local/workers` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorker {
    pub name: String,
    /// The Worker the dev server was started for (others are ones it binds to).
    pub is_self: bool,
    /// Its bindings, as (kind, binding name): (`d1`, `DB`).
    pub bindings: Vec<(String, String)>,
}

/// The Workers the dev server at `addr` runs, when its API lists them (`GET /local/workers`,
/// which Wrangler 4.118 has); empty otherwise.
pub async fn local_workers(addr: SocketAddr) -> Vec<LocalWorker> {
    match request(addr, Method::Get, "local/workers", None).await {
        Ok(r) if r.status == 200 => parse_workers(&r.body),
        _ => Vec::new(),
    }
}

/// `GET /local/workers`'s answer: `{"result": [{"name", "isSelf", "bindings": {"d1": [{"bindingName"}]}}]}`.
pub fn parse_workers(body: &[u8]) -> Vec<LocalWorker> {
    let Ok(doc) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(list) = doc.get("result").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|w| {
            let mut bindings = Vec::new();
            for (kind, items) in w
                .get("bindings")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                for item in items.as_array().into_iter().flatten() {
                    if let Some(name) = item.get("bindingName").and_then(Value::as_str) {
                        bindings.push((kind.clone(), name.to_string()));
                    }
                }
            }
            Some(LocalWorker {
                name: w.get("name")?.as_str()?.to_string(),
                is_self: w.get("isSelf").and_then(Value::as_bool).unwrap_or(false),
                bindings,
            })
        })
        .collect()
}
