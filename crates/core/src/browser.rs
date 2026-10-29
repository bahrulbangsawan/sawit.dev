//! The browser tab, without GPUI: the ops an agent sends it (over
//! `app_control`'s `Command::Browser`), what the address field turns typed
//! text into, the console and network logs the page agent reports, and the
//! page agent itself ([`AGENT_JS`]).
//!
//! The agent is injected into every page. It answers ops synchronously
//! (through `evaluate_script_with_callback`) and reports console messages,
//! requests and URL changes over IPC as `<token>\n<kind>\n<json>`; messages
//! without the webview's random token come from page scripts and are dropped.

use std::collections::VecDeque;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ports::PortJob;

/// The page agent; the app replaces [`TOKEN_PLACEHOLDER`] before injecting it.
pub const AGENT_JS: &str = include_str!("browser_agent.js");
pub const TOKEN_PLACEHOLDER: &str = "__SAWIT_TOKEN__";

/// Console messages and requests kept per browser tab, newest last.
pub const MAX_CONSOLE: usize = 500;
pub const MAX_NETWORK: usize = 300;

/// What the address field searches with when the text isn't a URL.
const SEARCH: &str = "https://duckduckgo.com/?q=";

/// An element: a ref from the last snapshot (`e12`) or a CSS selector.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Target {
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

impl Target {
    pub fn is_empty(&self) -> bool {
        self.reference.is_none() && self.selector.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserOp {
    Navigate {
        url: String,
    },
    Back,
    Forward,
    Reload,
    /// url, title, loading.
    State,
    /// The page as an outline with element refs.
    Snapshot,
    Click {
        #[serde(flatten)]
        target: Target,
        #[serde(default)]
        double: bool,
        /// `left` (default) or `right`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button: Option<String>,
    },
    Hover {
        #[serde(flatten)]
        target: Target,
    },
    /// Into `target`, else the focused element.
    Type {
        #[serde(flatten)]
        target: Target,
        text: String,
        #[serde(default)]
        clear: bool,
        #[serde(default)]
        submit: bool,
    },
    /// A key (`Enter`, `Tab`, `a`) with modifiers (`Meta+a`) to the focused element.
    Press {
        key: String,
    },
    Select {
        #[serde(flatten)]
        target: Target,
        values: Vec<String>,
    },
    /// Scrolls `target` (or the page) by dx/dy; with a target and no
    /// deltas, scrolls it into view.
    Scroll {
        #[serde(flatten)]
        target: Target,
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
    },
    /// Whether `text` is on the page / `selector` matches (for waits).
    Query {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    /// A JS expression, or a function body when it uses `return`.
    Evaluate {
        script: String,
    },
    /// An `evaluate` whose script returned a promise answers `{"pending": id}`;
    /// this asks whether that promise has settled.
    Settled {
        id: u64,
    },
    /// The visible page, or one element's box. Answers `png_base64`.
    Screenshot {
        #[serde(flatten)]
        target: Target,
    },
    Console {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default)]
        clear: bool,
    },
    Network {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default)]
        failed_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default)]
        clear: bool,
    },
    /// Opens Safari's Web Inspector on the page.
    Devtools,
}

impl BrowserOp {
    /// Answered by the app itself rather than the page agent.
    pub fn is_native(&self) -> bool {
        matches!(
            self,
            Self::Navigate { .. }
                | Self::Back
                | Self::Forward
                | Self::Reload
                | Self::State
                | Self::Screenshot { .. }
                | Self::Console { .. }
                | Self::Network { .. }
                | Self::Devtools
        )
    }

    /// Brings the browser tab to the front (others work in the background).
    pub fn shows_tab(&self) -> bool {
        matches!(self, Self::Navigate { .. } | Self::Screenshot { .. })
    }
}

/// What the address field loads for typed text: URLs as they are, hosts
/// with a scheme added (`http://` for local ones), anything else searched.
pub fn normalize_address(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return "about:blank".into();
    }
    let lower = text.to_ascii_lowercase();
    if ["http://", "https://", "file://", "about:", "data:"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        return text.to_string();
    }
    if text.contains(char::is_whitespace) {
        return format!("{SEARCH}{}", encode_query(text));
    }
    let host = lower
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit_once('@')
        .map_or(lower.as_str(), |(_, h)| h)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (name, true)
        }
        _ => (host, false),
    };
    let local = name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".test")
        || is_ipv4(name);
    if local {
        return format!("http://{text}");
    }
    if name.contains('.') || port {
        return format!("https://{text}");
    }
    format!("{SEARCH}{}", encode_query(text))
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// `application/x-www-form-urlencoded` for a search query.
fn encode_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    /// `log`, `info`, `warning`, `error` or `debug`.
    pub level: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Milliseconds since the page started.
    #[serde(default)]
    pub time_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// `fetch`, `xhr`, or the resource's initiator (`script`, `img`, …).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    /// The request never got a response (blocked, offline, CORS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub time_ms: f64,
}

impl NetworkEntry {
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.status.is_some_and(|s| s >= 400)
    }
}

/// What a page sends over IPC.
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    Console(ConsoleEntry),
    Network(NetworkEntry),
    /// The address changed without a load (`pushState`, hash).
    Url(String),
}

/// A page message, if it carries `token`.
pub fn parse_ipc(body: &str, token: &str) -> Option<PageEvent> {
    let rest = body.strip_prefix(token)?.strip_prefix('\n')?;
    let (kind, json) = rest.split_once('\n')?;
    match kind {
        "console" => serde_json::from_str(json).ok().map(PageEvent::Console),
        "network" => serde_json::from_str(json).ok().map(PageEvent::Network),
        "url" => serde_json::from_str::<String>(json)
            .ok()
            .map(PageEvent::Url),
        _ => None,
    }
}

/// Console messages and requests since the tab opened (or the last clear).
#[derive(Debug, Default)]
pub struct Logs {
    pub console: VecDeque<ConsoleEntry>,
    pub network: VecDeque<NetworkEntry>,
}

impl Logs {
    pub fn push_console(&mut self, entry: ConsoleEntry) {
        if self.console.len() == MAX_CONSOLE {
            self.console.pop_front();
        }
        self.console.push_back(entry);
    }

    pub fn push_network(&mut self, entry: NetworkEntry) {
        if self.network.len() == MAX_NETWORK {
            self.network.pop_front();
        }
        self.network.push_back(entry);
    }

    /// The newest `limit` messages at `level` (`warn` means `warning`)
    /// whose text holds `pattern` (any case), oldest first.
    pub fn console_json(
        &self,
        level: Option<&str>,
        pattern: Option<&str>,
        limit: Option<usize>,
    ) -> Value {
        let level = level.map(|l| match l.trim().to_ascii_lowercase().as_str() {
            "warn" => "warning".to_string(),
            "err" => "error".to_string(),
            other => other.to_string(),
        });
        let pattern = pattern.map(str::to_lowercase).filter(|p| !p.is_empty());
        let matching: Vec<&ConsoleEntry> = self
            .console
            .iter()
            .filter(|e| level.as_deref().is_none_or(|l| e.level == l))
            .filter(|e| {
                pattern
                    .as_deref()
                    .is_none_or(|p| e.text.to_lowercase().contains(p))
            })
            .collect();
        let shown = newest(&matching, limit.unwrap_or(100));
        json!({
            "total": self.console.len(),
            "matching": matching.len(),
            "errors": self.console.iter().filter(|e| e.level == "error").count(),
            "messages": shown,
        })
    }

    pub fn network_json(
        &self,
        pattern: Option<&str>,
        failed_only: bool,
        limit: Option<usize>,
    ) -> Value {
        let pattern = pattern.map(str::to_lowercase).filter(|p| !p.is_empty());
        let matching: Vec<&NetworkEntry> = self
            .network
            .iter()
            .filter(|e| !failed_only || e.failed())
            .filter(|e| {
                pattern
                    .as_deref()
                    .is_none_or(|p| e.url.to_lowercase().contains(p))
            })
            .collect();
        let shown = newest(&matching, limit.unwrap_or(100));
        json!({
            "total": self.network.len(),
            "matching": matching.len(),
            "failed": self.network.iter().filter(|e| e.failed()).count(),
            "requests": shown,
        })
    }
}

fn newest<'a, T>(items: &'a [&'a T], limit: usize) -> &'a [&'a T] {
    &items[items.len().saturating_sub(limit)..]
}

/// Standard base64 with padding (screenshots over the control socket).
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The website data store (cookies, storage) of the project's browser:
/// one per folder, stable across launches.
pub fn data_store_id(project: &Path) -> [u8; 16] {
    let bytes = project.as_os_str().as_encoded_bytes();
    let fnv = |seed: u64| {
        bytes.iter().fold(seed, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&fnv(0xcbf2_9ce4_8422_2325).to_be_bytes());
    id[8..].copy_from_slice(&fnv(0x6c62_272e_07bb_0142).to_be_bytes());
    // A version 4 UUID shape, as WKWebsiteDataStore expects a UUID.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// The project's dev server when exactly one is running from inside it.
pub fn pick_dev_server(jobs: &[PortJob], project: &Path) -> Option<String> {
    let mut found = jobs.iter().filter(|j| {
        j.dev
            && !j.is_daemon()
            && !j.is_docker()
            && j.cwd
                .as_deref()
                .is_some_and(|c| Path::new(c).starts_with(project))
            && !j.ports.is_empty()
    });
    let job = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let port = job.ports.iter().map(|(p, _)| *p).min()?;
    Some(format!("http://localhost:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        for (typed, url) in [
            ("localhost:5173", "http://localhost:5173"),
            ("127.0.0.1:8080/x", "http://127.0.0.1:8080/x"),
            ("app.test", "http://app.test"),
            ("example.com/a?b=1", "https://example.com/a?b=1"),
            ("http://x.dev", "http://x.dev"),
            ("HTTPS://X.dev", "HTTPS://X.dev"),
            ("file:///tmp/a.html", "file:///tmp/a.html"),
            ("about:blank", "about:blank"),
            ("", "about:blank"),
            ("myhost:3000", "https://myhost:3000"),
        ] {
            assert_eq!(normalize_address(typed), url, "{typed}");
        }
        assert_eq!(
            normalize_address("rust gpui"),
            "https://duckduckgo.com/?q=rust+gpui"
        );
        assert_eq!(
            normalize_address("c++"),
            "https://duckduckgo.com/?q=c%2B%2B"
        );
    }

    #[test]
    fn ipc_needs_the_token() {
        let body = "t0k\nconsole\n{\"level\":\"error\",\"text\":\"boom\",\"time_ms\":3}";
        assert_eq!(
            parse_ipc(body, "t0k"),
            Some(PageEvent::Console(ConsoleEntry {
                level: "error".into(),
                text: "boom".into(),
                url: None,
                line: None,
                time_ms: 3.,
            }))
        );
        assert_eq!(parse_ipc(body, "other"), None);
        assert_eq!(parse_ipc("console\n{}", ""), None);
        assert_eq!(
            parse_ipc("t0k\nurl\n\"http://a/#b\"", "t0k"),
            Some(PageEvent::Url("http://a/#b".into()))
        );
        assert_eq!(parse_ipc("t0k\nnope\n{}", "t0k"), None);
    }

    fn console(level: &str, text: &str) -> ConsoleEntry {
        ConsoleEntry {
            level: level.into(),
            text: text.into(),
            url: None,
            line: None,
            time_ms: 0.,
        }
    }

    fn request(url: &str, status: Option<u16>, error: Option<&str>) -> NetworkEntry {
        NetworkEntry {
            method: "GET".into(),
            url: url.into(),
            status,
            kind: "fetch".into(),
            duration_ms: None,
            request_body: None,
            response_body: None,
            error: error.map(Into::into),
            time_ms: 0.,
        }
    }

    #[test]
    fn console_ring_and_filters() {
        let mut logs = Logs::default();
        for i in 0..MAX_CONSOLE + 5 {
            logs.push_console(console("log", &i.to_string()));
        }
        assert_eq!(logs.console.len(), MAX_CONSOLE);
        assert_eq!(logs.console.front().unwrap().text, "5");
        logs.push_console(console("error", "TypeError: x is undefined"));
        logs.push_console(console("warning", "deprecated"));
        let v = logs.console_json(Some("error"), Some("TYPEerror"), Some(10));
        assert_eq!(v["matching"], 1);
        assert_eq!(v["errors"], 1);
        assert_eq!(v["messages"][0]["text"], "TypeError: x is undefined");
        assert_eq!(logs.console_json(Some("warn"), None, None)["matching"], 1);
        // The newest `limit`, oldest first.
        let v = logs.console_json(Some("log"), None, Some(2));
        assert_eq!(v["messages"][0]["text"], (MAX_CONSOLE + 3).to_string());
        assert_eq!(v["messages"][1]["text"], (MAX_CONSOLE + 4).to_string());
    }

    #[test]
    fn failed_requests() {
        let mut logs = Logs::default();
        logs.push_network(request("http://a/ok", Some(200), None));
        logs.push_network(request("http://a/missing", Some(404), None));
        logs.push_network(request("http://b/cors", None, Some("Load failed")));
        let v = logs.network_json(None, true, None);
        assert_eq!(v["matching"], 2);
        assert_eq!(v["failed"], 2);
        assert_eq!(logs.network_json(Some("A/OK"), false, None)["matching"], 1);
    }

    #[test]
    fn base64() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"h"), "aA==");
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(&[0x89, b'P', b'N', b'G']), "iVBORw==");
    }

    #[test]
    fn store_ids_are_stable_per_folder() {
        let a = data_store_id(Path::new("/code/shop"));
        assert_eq!(a, data_store_id(Path::new("/code/shop")));
        assert_ne!(a, data_store_id(Path::new("/code/blog")));
        assert_eq!(a[6] >> 4, 4);
    }

    fn job(cwd: &str, port: u16) -> PortJob {
        PortJob {
            root: 1,
            pids: vec![1],
            label: "bun run dev".into(),
            tool: "bun".into(),
            cmdline: "bun run dev".into(),
            listeners: vec![(2, "node".into())],
            ports: vec![(port, crate::ports::BindAddr::LoopbackV4)],
            cwd: Some(cwd.into()),
            dev: true,
        }
    }

    #[test]
    fn dev_server_of_the_project() {
        let project = Path::new("/code/shop");
        assert_eq!(
            pick_dev_server(
                &[job("/code/shop/web", 5173), job("/code/blog", 3000)],
                project
            ),
            Some("http://localhost:5173".into())
        );
        assert_eq!(
            pick_dev_server(
                &[job("/code/shop", 5173), job("/code/shop/api", 3000)],
                project
            ),
            None
        );
        assert_eq!(pick_dev_server(&[], project), None);
    }

    #[test]
    fn ops_wire_format() {
        let op: BrowserOp =
            serde_json::from_str(r#"{"op":"click","ref":"e3","double":false}"#).unwrap();
        assert_eq!(
            op,
            BrowserOp::Click {
                target: Target {
                    reference: Some("e3".into()),
                    selector: None
                },
                double: false,
                button: None,
            }
        );
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            json!({"op": "click", "ref": "e3", "double": false})
        );
        let op: BrowserOp = serde_json::from_str(r#"{"op":"type","text":"hi"}"#).unwrap();
        assert!(
            matches!(op, BrowserOp::Type { ref target, clear: false, .. } if target.is_empty())
        );
        assert!(BrowserOp::State.is_native() && !BrowserOp::Snapshot.is_native());
        assert!(serde_json::from_str::<BrowserOp>(r#"{"op":"fly"}"#).is_err());
    }

    #[test]
    fn agent_script_parses() {
        assert!(AGENT_JS.contains(TOKEN_PLACEHOLDER) && AGENT_JS.contains("__sawitAgent"));
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            // Compiles without running: there is no DOM here.
            let source = format!("(function () {{ {AGENT_JS} }})");
            ctx.eval::<rquickjs::Value, _>(source)
                .map(|_| ())
                .unwrap_or_else(|e| panic!("agent script doesn't parse: {e:?}"));
        });
    }
}
