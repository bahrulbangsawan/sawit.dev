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
/// The picker, highlights and markers (a page module).
pub const OVERLAY_JS: &str = include_str!("browser_overlay.js");
/// The design scan, stylesheet rules and element styles (a page module; uses the overlay's helpers).
pub const DESIGN_JS: &str = include_str!("browser_design.js");
/// Where the agent takes its page modules.
pub const MODULES_PLACEHOLDER: &str = "/*__SAWIT_MODULES__*/";

/// The agent with its page modules spliced in and `token` set.
pub fn page_script(token: &str) -> String {
    let modules = [OVERLAY_JS, DESIGN_JS].join("\n");
    AGENT_JS
        .replace(MODULES_PLACEHOLDER, &modules)
        .replace(TOKEN_PLACEHOLDER, token)
}

/// The script that runs an agent's `browser_evaluate` script on the page: an expression,
/// or (`body`) a function body that uses `return`. WebKit compiles the script with this
/// call, and the page agent only runs it: compiled by the agent (`new Function`), it failed
/// on pages whose Content-Security-Policy forbids `eval`. [`evaluate_as_body`] picks the form.
pub fn evaluate_call(script: &str, body: bool) -> String {
    // On its own line: a script ending in a `//` comment still closes.
    let function = if body {
        format!("function () {{\n{script}\n}}")
    } else {
        format!("function () {{ return (\n{script}\n); }}")
    };
    format!(
        "(() => {{ const a = window.__sawitAgent; return a ? a.run({{ op: \"evaluate\", fn: {function} }}) : \"\"; }})()"
    )
}

/// Whether an agent's `browser_evaluate` script goes to WebKit as a function body at once:
/// its expression form ([`evaluate_call`]) doesn't compile and its body form does. WebKit
/// reports a failed compile to the page as an uncaught `SyntaxError` of the page's own (in
/// its console, to `window.onerror`, in the notes' console errors), so the form is picked
/// here first, by QuickJS compiling each call without running it. `false` when the
/// expression compiles, and when neither does (a real error, or syntax QuickJS lacks):
/// WebKit then tries the expression, and the body after its `SyntaxError`.
pub fn evaluate_as_body(script: &str) -> bool {
    let Ok(rt) = rquickjs::Runtime::new() else {
        return false;
    };
    rt.set_memory_limit(COMPILE_MEMORY);
    let Ok(cx) = rquickjs::Context::full(&rt) else {
        return false;
    };
    cx.with(|cx| {
        !compiles(&cx, &evaluate_call(script, false)) && compiles(&cx, &evaluate_call(script, true))
    })
}

/// What compiling an agent's script may take; past it, the script counts as not compiling.
const COMPILE_MEMORY: usize = 64 << 20;

/// Whether `source` compiles as a script, the way WebKit compiles one (sloppy mode). Nothing
/// in it runs: the text is an agent's, and could close any wrapper put around it.
fn compiles(cx: &rquickjs::Ctx<'_>, source: &str) -> bool {
    use rquickjs::qjs;

    let Ok(text) = std::ffi::CString::new(source) else {
        return false;
    };
    let flags = (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
    let ctx = cx.as_raw().as_ptr();
    // SAFETY: `ctx` is the live context `cx` borrows. `JS_Eval` reads `input[input_len]`,
    // which is the NUL `CString` ends with. It returns an owned value, the compiled script
    // or the exception marker; the script, or the exception taken from the context, is freed
    // here once, so the runtime is left holding nothing.
    unsafe {
        let compiled = qjs::JS_Eval(
            ctx,
            text.as_ptr(),
            text.as_bytes().len() as _,
            c"evaluate".as_ptr(),
            flags,
        );
        if qjs::JS_IsException(compiled) {
            qjs::JS_FreeValue(ctx, qjs::JS_GetException(ctx));
            return false;
        }
        qjs::JS_FreeValue(ctx, compiled);
        true
    }
}

/// Console messages and requests kept per browser tab, newest last.
pub const MAX_CONSOLE: usize = 500;
pub const MAX_NETWORK: usize = 300;

/// What an agent's op that doesn't open the browser gets while the project has none open.
pub const NOT_OPEN: &str = "The browser isn't open. Open a page with browser_navigate.";

/// What a page op gets while the page has no agent to run it: it is still loading, or isn't a
/// web page. Waiting for the page waits this out.
pub const NOT_READY: &str = "The page isn't ready yet (it may still be loading, or isn't a web page). Try again after browser_wait_for.";

/// How the error of a page op that WebKit couldn't hand to the page starts, the reason after it.
/// Waiting for the page waits this out too.
pub const UNREACHABLE: &str = "The page couldn't be reached";

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
    /// Sizes the page like a device (`device`) or `width` × `height`, or
    /// back to filling the tab (`device: "responsive"`).
    Resize {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
    /// The annotations the user made, as markdown; `clear` removes them
    /// after the answer is built.
    Annotations {
        #[serde(default)]
        clear: bool,
    },
    /// An element's computed styles; `format` is `json` (default, with
    /// contrast), `css`, `tailwind` or `html`.
    Inspect {
        #[serde(flatten)]
        target: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    /// The page's design scan and derived tokens; `part` is `summary`
    /// (default), `colors`, `fonts`, `assets`, `tokens` or `variables`.
    Design {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
    },
    /// Starts writing the design export folder in the background (or joins
    /// the one running).
    ExportDesign {
        #[serde(default)]
        include_videos: bool,
        /// Replace what an earlier export left in the folder; without it, a
        /// folder that holds anything is refused, and nothing starts.
        #[serde(default)]
        replace: bool,
    },
    /// How the last design export is doing.
    ExportStatus,
    /// Opens the side panel (`annotate` or `design`, on its `tab`: inspect, colors, fonts,
    /// assets or system), or closes it (`panel` left out). For scripts and visual checks:
    /// agents read annotations and design through their own ops, panel or not.
    Panel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        panel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
    },
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
                | Self::Resize { .. }
                | Self::Annotations { .. }
                | Self::Inspect { .. }
                | Self::Design { .. }
                | Self::ExportDesign { .. }
                | Self::ExportStatus
                | Self::Panel { .. }
        )
    }

    /// Brings the browser tab to the front (others work in the background).
    pub fn shows_tab(&self) -> bool {
        matches!(
            self,
            Self::Navigate { .. } | Self::Screenshot { .. } | Self::Resize { .. }
        )
    }
}

/// A device the browser tab can imitate.
#[derive(Debug, PartialEq)]
pub struct Device {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
}

pub const DEVICES: &[Device] = &[
    Device {
        name: "iPhone 15",
        width: 393,
        height: 852,
    },
    Device {
        name: "Pixel 8",
        width: 412,
        height: 915,
    },
    Device {
        name: "iPad mini",
        width: 744,
        height: 1133,
    },
    Device {
        name: "iPad Pro 11″",
        width: 834,
        height: 1194,
    },
    Device {
        name: "Laptop",
        width: 1280,
        height: 800,
    },
    Device {
        name: "Desktop",
        width: 1440,
        height: 900,
    },
];

fn device_key(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !matches!(c, '″' | '"'))
        .collect::<String>()
        .to_lowercase()
}

/// A device by name, ignoring case and the inch mark.
pub fn device_by_name(name: &str) -> Option<&'static Device> {
    let key = device_key(name);
    DEVICES.iter().find(|d| device_key(d.name) == key)
}

/// How the page is sized in its tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum ViewportMode {
    /// Fills the tab.
    #[default]
    Responsive,
    Fixed {
        name: Option<String>,
        width: u32,
        height: u32,
    },
}

/// A device size scaled down to fit an area.
#[derive(Debug, PartialEq)]
pub struct Fit {
    pub scale: f64,
    pub width: f32,
    pub height: f32,
}

/// `size` (CSS px) at the largest scale up to 1 that fits `area`.
///
/// Scaled down, the view gets a whole number of points (WebKit rounds the
/// view's size, then `innerWidth` is that size over the zoom, whole), and
/// the zoom is set so the width comes out a quarter pixel over the device's:
/// `innerWidth` is exactly the device width. The height lands within a pixel
/// or so of the device's.
pub fn fit(size: (u32, u32), area: (f32, f32)) -> Fit {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let scale = (f64::from(area.0) / w)
        .min(f64::from(area.1) / h)
        .clamp(0.0, 1.0);
    if scale >= 1.0 {
        return Fit {
            scale: 1.0,
            width: w as f32,
            height: h as f32,
        };
    }
    let width = (w * scale).floor().max(1.0);
    let scale = width / (w + 0.25);
    Fit {
        scale,
        width: width as f32,
        height: ((h + 0.25) * scale).round().max(1.0) as f32,
    }
}

/// The mode an agent asks for: a device name, `responsive`, or a size.
pub fn resize_mode(
    device: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<ViewportMode, String> {
    if let Some(name) = device {
        if name.trim().eq_ignore_ascii_case("responsive") {
            return Ok(ViewportMode::Responsive);
        }
        return device_by_name(name)
            .map(|d| ViewportMode::Fixed {
                name: Some(d.name.into()),
                width: d.width,
                height: d.height,
            })
            .ok_or_else(|| {
                format!(
                    "Unknown device “{}”. Use iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive.",
                    name.trim()
                )
            });
    }
    match (width, height) {
        (Some(w), Some(h)) => {
            if (200..=3840).contains(&w) && (200..=3840).contains(&h) {
                Ok(ViewportMode::Fixed { name: None, width: w, height: h })
            } else {
                Err("Width and height must be between 200 and 3840.".into())
            }
        }
        _ => Err("Give a device (iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive) or a width and height.".into()),
    }
}

/// What the address field loads for typed text: URLs as they are, hosts
/// with a scheme added (`http://` for local ones), anything else searched.
/// A web URL whose host isn't one (whitespace, a second `:port`) is searched
/// too: WebKit can't load it.
pub fn normalize_address(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return "about:blank".into();
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = ["http://", "https://"]
        .iter()
        .find_map(|s| lower.strip_prefix(s))
    {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        return match split_host(authority) {
            Some(_) => text.to_string(),
            None => search_url(text),
        };
    }
    if ["file://", "about:", "data:", "mailto:", "tel:"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        return text.to_string();
    }
    if text.contains(char::is_whitespace) {
        return search_url(text);
    }
    let authority = lower.split(['/', '?', '#']).next().unwrap_or_default();
    let Some((name, port)) = split_host(authority) else {
        return search_url(text);
    };
    let local = name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".test")
        || name == "[::1]"
        || is_ipv4(name);
    if local {
        return format!("http://{text}");
    }
    if name.contains('.') || port {
        return format!("https://{text}");
    }
    search_url(text)
}

/// A URL authority's host and whether it names a port: `None` when it isn't a host
/// (empty, whitespace, a port that isn't a number, a second `:port`, a bare IPv6 address).
fn split_host(authority: &str) -> Option<(&str, bool)> {
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    // `[::1]` and `[::1]:3000`: the brackets hold the colons.
    let (name, port) = match host.strip_prefix('[') {
        Some(v6) => {
            let (inside, after) = v6.split_once(']')?;
            if inside.is_empty() || after.contains(['[', ']']) {
                return None;
            }
            (&host[..inside.len() + 2], after.strip_prefix(':'))
        }
        None => match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        },
    };
    if name.is_empty() || (!name.starts_with('[') && name.contains([':', '[', ']'])) {
        return None;
    }
    match port {
        Some(p) if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) => None,
        Some(_) => Some((name, true)),
        None if host.len() > name.len() => None,
        None => Some((name, false)),
    }
}

/// The search the address field runs for `text`.
pub fn search_url(text: &str) -> String {
    format!("{SEARCH}{}", encode_query(text.trim()))
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

    /// The entry as the network log keeps it, so that no password or token reaches an agent
    /// (`browser_network`), as `browser_snapshot` shows no password field's value: the values of
    /// fields named like secrets ([`secret_name`]) are [`REDACTED`] in the URL (its query, its
    /// fragment and a password before its host) and in the request and response bodies (JSON,
    /// also cut short, and forms). The rest is kept as it was, so the shape still shows. The page
    /// agent has already put [`REDACTED`] wherever a request carried what is typed in the page's
    /// password fields, whatever their names.
    pub fn redacted(mut self) -> NetworkEntry {
        self.url = redact_url(&self.url);
        self.request_body = self.request_body.as_deref().map(redact_body);
        self.response_body = self.response_body.as_deref().map(redact_body);
        self
    }
}

/// What a secret's value is in the network log instead ([`NetworkEntry::redacted`]).
pub const REDACTED: &str = "[redacted]";

/// Whether a field named `name` holds a secret: a password, a token, a key, a cookie, a session
/// or a card's number. Names are compared by their letters and digits in lower case, so
/// `new_password`, `apiKey`, `X-Auth-Token`, `user[password]` and `client_secret` are secrets,
/// and `max_tokens`, `token_type` and `author` aren't.
fn secret_name(name: &str) -> bool {
    /// Names of secrets on their own, which are other words inside longer names.
    const WHOLE: &[&str] = &[
        "pass", "pw", "pin", "mfa", "cvc", "cvv", "cvv2", "csc", "auth", "jwt", "bearer",
    ];
    /// How the names of secrets end.
    const ENDS: &[&str] = &[
        "token",
        "session",
        "sessionid",
        "sessionkey",
        "sessid",
        "sid",
        "pwd",
        "otp",
        "otpcode",
        "mfacode",
        "pincode",
        "passcode",
    ];
    /// Words in the names of secrets.
    const PARTS: &[&str] = &[
        "password",
        "passwd",
        "passphrase",
        "secret",
        "apikey",
        "privatekey",
        "authorization",
        "cookie",
        "credential",
        "signature",
        "creditcard",
        "cardnumber",
    ];
    let name: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    WHOLE.contains(&name.as_str())
        || ENDS.iter().any(|end| name.ends_with(end))
        || PARTS.iter().any(|part| name.contains(part))
}

/// `url` with its secrets [`REDACTED`]: the password of `user:password@`, and the values of the
/// secret fields of its query and of its fragment (`#access_token=…`).
pub(crate) fn redact_url(url: &str) -> String {
    let (rest, fragment) = match url.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (url, None),
    };
    let (base, query) = match rest.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (rest, None),
    };
    let mut out = match base.split_once("://") {
        Some((scheme, after)) => {
            let host_end = after.find('/').unwrap_or(after.len());
            match after[..host_end].rsplit_once('@') {
                Some((userinfo, host)) if userinfo.contains(':') => {
                    let user = userinfo.split_once(':').map_or(userinfo, |(user, _)| user);
                    format!("{scheme}://{user}:{REDACTED}@{host}{}", &after[host_end..])
                }
                _ => base.to_string(),
            }
        }
        None => base.to_string(),
    };
    if let Some(query) = query {
        out.push('?');
        out.push_str(&redact_form(query));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(&redact_form(fragment));
    }
    out
}

/// A request's or a response's body with its secrets [`REDACTED`]: as JSON when it starts as JSON
/// does, as a form when it looks like one ([`looks_like_form`]), else as it is. The note the page
/// agent puts after a body it cut short (`… (5000 chars)`) stays.
fn redact_body(body: &str) -> String {
    let (head, note) = cut_note(body);
    let redacted = if head.trim_start().starts_with(['{', '[']) {
        redact_json(head)
    } else if looks_like_form(head) {
        redact_form(head)
    } else {
        head.to_string()
    };
    redacted + note
}

/// `body` split before the note the page agent's `clip` puts after a body it cut: `… (5000
/// chars)`. Without one, all of it and no note.
fn cut_note(body: &str) -> (&str, &str) {
    const START: &str = "… (";
    if let Some(head) = body.strip_suffix(" chars)")
        && let Some(at) = head.rfind(START)
        && head.len() > at + START.len()
        && head[at + START.len()..].bytes().all(|b| b.is_ascii_digit())
    {
        return body.split_at(at);
    }
    (body, "")
}

/// Whether `text` reads as a form: `name=value` pairs joined by `&`, each name written as forms
/// write them. Text, HTML and SQL have other characters before their first `=`.
fn looks_like_form(text: &str) -> bool {
    text.contains('=')
        && text.split('&').all(|pair| {
            let name = pair.split_once('=').map_or(pair, |(name, _)| name);
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.[]%+*~:".contains(&b))
        })
}

/// `a=1&password=…`, a form or a URL's query, with the value of each secret field [`REDACTED`].
fn redact_form(form: &str) -> String {
    form.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if secret_name(&percent_decode(name)) => format!("{name}={REDACTED}"),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A form's name as it was before it was written into the form: `%5F` is `_`, `+` a space.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .filter(|hex| bytes[at] == b'%' && hex.iter().all(u8::is_ascii_hexdigit));
        match hex.and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()) {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `json`, also cut short, with every value under a secret field's name ([`secret_name`]) as
/// `"[redacted]"`: a string or a number, and each one inside an object or an array there. The
/// rest is copied as it is, spaces and all.
fn redact_json(json: &str) -> String {
    /// An object or an array the text is in, and whether everything in it is a secret's.
    struct Open {
        object: bool,
        secret: bool,
    }
    let secret_value = format!("\"{REDACTED}\"");
    let bytes = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut open: Vec<Open> = Vec::new();
    // In an object: whether the next string is a name, and whether the value after the last
    // name is a secret.
    let (mut name_next, mut value_secret) = (false, false);
    let mut at = 0;
    while at < bytes.len() {
        let secret = match open.last() {
            Some(o) if o.object => value_secret,
            Some(o) => o.secret,
            None => false,
        };
        match bytes[at] {
            c @ (b'{' | b'[') => {
                open.push(Open {
                    object: c == b'{',
                    secret,
                });
                name_next = c == b'{';
                out.push(c as char);
                at += 1;
            }
            c @ (b'}' | b']') => {
                open.pop();
                name_next = false;
                out.push(c as char);
                at += 1;
            }
            b',' => {
                name_next = open.last().is_some_and(|o| o.object);
                out.push(',');
                at += 1;
            }
            b'"' => {
                let end = string_end(bytes, at);
                let text = &json[at..end];
                if name_next {
                    let inherited = open.last().is_some_and(|o| o.secret);
                    value_secret = inherited || secret_name(text);
                    name_next = false;
                    out.push_str(text);
                } else if secret {
                    out.push_str(&secret_value);
                } else {
                    out.push_str(text);
                }
                at = end;
            }
            b'-' | b'0'..=b'9' => {
                let len = bytes[at..]
                    .iter()
                    .take_while(|b| b"+-.eE0123456789".contains(b))
                    .count();
                out.push_str(if secret {
                    secret_value.as_str()
                } else {
                    &json[at..at + len]
                });
                at += len;
            }
            // Spaces, `:`, `true`, `false` and `null`, a whole character at a time.
            _ => {
                let len = json[at..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&json[at..at + len]);
                at += len;
            }
        }
    }
    out
}

/// Where the JSON string whose `"` is at `start` ends: after its closing `"`, or at the end of a
/// text cut short inside it.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// What a page sends over IPC.
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    Console(ConsoleEntry),
    Network(NetworkEntry),
    /// The address changed without a load (`pushState`, hash).
    Url(String),
    /// The user picked an element: `{mode, element}`.
    Picked(Value),
    /// The pointer is over an element in inspect mode: `{selector, label, rect}`.
    Hovered(Value),
    /// A key pressed in the page: `escape`, while picking.
    Key(String),
    /// The user changed an annotation from its popover on the page:
    /// `{action: "update", n, comment, intent}` or `{action: "remove", n}`.
    Annotation(Value),
    /// The user pressed in the page (a real `pointerdown`, never a script's), which AppKit then
    /// gives the keyboard.
    Pressed,
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
        "picked" => serde_json::from_str(json).ok().map(PageEvent::Picked),
        "hovered" => serde_json::from_str(json).ok().map(PageEvent::Hovered),
        "key" => serde_json::from_str::<String>(json)
            .ok()
            .map(PageEvent::Key),
        "annotation" => serde_json::from_str(json).ok().map(PageEvent::Annotation),
        "pressed" => Some(PageEvent::Pressed),
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

    /// Errors from the current page (after its last `— navigated to`
    /// marker): the newest `limit`, oldest first.
    pub fn page_errors(&self, limit: usize) -> Vec<ConsoleEntry> {
        let start = self
            .console
            .iter()
            .rposition(|e| e.text.starts_with("— navigated to"))
            .map_or(0, |i| i + 1);
        let errors: Vec<&ConsoleEntry> = self
            .console
            .iter()
            .skip(start)
            .filter(|e| e.level == "error")
            .collect();
        errors[errors.len().saturating_sub(limit)..]
            .iter()
            .map(|e| (*e).clone())
            .collect()
    }

    /// Keeps `entry`, with its secrets hidden ([`NetworkEntry::redacted`]).
    pub fn push_network(&mut self, entry: NetworkEntry) {
        if self.network.len() == MAX_NETWORK {
            self.network.pop_front();
        }
        self.network.push_back(entry.redacted());
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

/// Decodes standard base64 (padding optional, line breaks skipped); `None` on other
/// characters.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let text = text.trim_end().trim_end_matches('=');
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
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
    match dev_servers(jobs, project).as_slice() {
        [job] => dev_server_url(job),
        _ => None,
    }
}

/// The dev servers running from inside `folder` (what `npm run dev` and the like start, not a
/// database, a daemon or Docker), with a port.
pub fn dev_servers<'a>(jobs: &'a [PortJob], folder: &Path) -> Vec<&'a PortJob> {
    jobs.iter()
        .filter(|j| {
            j.dev
                && !j.is_daemon()
                && !j.is_docker()
                && j.cwd
                    .as_deref()
                    .is_some_and(|c| Path::new(c).starts_with(folder))
                && !j.ports.is_empty()
        })
        .collect()
}

/// Where the browser opens `job`, a dev server: its lowest port on localhost.
pub fn dev_server_url(job: &PortJob) -> Option<String> {
    let port = job.ports.iter().map(|(p, _)| *p).min()?;
    Some(format!("http://localhost:{port}"))
}

/// The front window's project, from the app's `state` (its answer to `Command::State`): its
/// browser shows the page `browser_navigate` and `sawit browse` open for no project of their own.
pub fn front_project(state: &Value) -> Option<std::path::PathBuf> {
    let windows = state["windows"].as_array()?;
    let front = windows.iter().find(|window| window["frontmost"] == true)?;
    front["project"].as_str().map(std::path::PathBuf::from)
}

/// The open project that holds `folder`, from the app's `state`: the deepest of the windows'
/// projects, as the app picks the window and the browser for a folder, and as it is on disk
/// (through the links on its way, [`crate::files::resolve`]), as `lsof` lists a dev server's.
pub fn project_holding(state: &Value, folder: &Path) -> Option<std::path::PathBuf> {
    let folder = crate::files::resolve(folder);
    state["windows"]
        .as_array()?
        .iter()
        .flat_map(|window| window["projects"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(|root| crate::files::resolve(Path::new(root)))
        .filter(|root| folder.starts_with(root))
        .max_by_key(|root| root.components().count())
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

    /// Text that isn't a URL WebKit can read is searched, never loaded: the browser asks
    /// `NSURL` for what it loads, and `NSURL` has none for these (wry unwraps it, so loading
    /// one aborted Sawit). A second `:port`, whitespace or a bad port in the host, and a bare
    /// IPv6 address are typos.
    #[test]
    fn hosts_that_arent_urls_are_searched() {
        for typed in [
            "localhost:3000:3000",
            "127.0.0.1:3000:3000",
            "http://localhost 3000",
            "http:// localhost:3000",
            "https://my site.com",
            "https://localhost:3000:3000",
            "HTTPS://localhost:3000:3000/x",
            "http://127.0.0.1:3000:3000",
            "https://foo:exa%7Cmple.com",
            "https://::1",
            "https://user@host:1:2",
            "https://",
            "example.com:80a",
            "a.b:c/x",
        ] {
            let url = normalize_address(typed);
            assert!(url.starts_with(SEARCH), "{typed} → {url}");
            assert_eq!(url, search_url(typed), "{typed}");
        }
        // Spaces after the host are the path's: WebKit encodes them.
        assert_eq!(
            normalize_address("https://example.com/a b"),
            "https://example.com/a b"
        );
        assert_eq!(normalize_address("[::1]:3000/x"), "http://[::1]:3000/x");
        assert_eq!(
            normalize_address("https://[::1]:3000"),
            "https://[::1]:3000"
        );
        assert_eq!(
            normalize_address("user@example.com:8443/x"),
            "https://user@example.com:8443/x"
        );
        // Mail and phone links go to their apps, as the page's links do.
        assert_eq!(normalize_address("mailto:a@b.c"), "mailto:a@b.c");
        assert_eq!(normalize_address("tel:+15551234"), "tel:+15551234");
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
    fn page_errors_since_navigation() {
        let mut logs = Logs::default();
        logs.push_console(console("error", "old"));
        logs.push_console(console("info", "— navigated to http://x/"));
        for i in 0..25 {
            logs.push_console(console("error", &format!("e{i}")));
        }
        let errors = logs.page_errors(20);
        assert_eq!(errors.len(), 20);
        assert_eq!(errors[0].text, "e5");
        assert_eq!(errors[19].text, "e24");
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
    fn secrets_are_known_by_their_names() {
        for name in [
            "password",
            "Password",
            "new_password",
            "password_confirmation",
            "user[password]",
            "passwd",
            "pass",
            "pw",
            "userPwd",
            "api_key",
            "apiKey",
            "X-API-Key",
            "token",
            "access_token",
            "refreshToken",
            "X-CSRF-Token",
            "client_secret",
            "secretAccessKey",
            "Authorization",
            "auth",
            "cookie",
            "Set-Cookie",
            "session",
            "sessionid",
            "PHPSESSID",
            "connect.sid",
            "credentials",
            "X-Amz-Signature",
            "private_key",
            "jwt",
            "otp",
            "pin",
            "cvc",
            "card_number",
        ] {
            assert!(secret_name(name), "{name} holds a secret");
        }
        for name in [
            "email",
            "user",
            "username",
            "name",
            "id",
            "author",
            "authority",
            "max_tokens",
            "input_tokens",
            "token_type",
            "sessions",
            "passport",
            "compass",
            "sidebar",
            "pinned",
            "keyboard",
            "description",
            "",
        ] {
            assert!(!secret_name(name), "{name} holds no secret");
        }
    }

    /// The values of secret fields read [redacted] wherever they are in JSON, and nothing else
    /// changes: not the names, the order, the spaces, nor values that aren't secrets (`true`,
    /// `null` and counts stay).
    #[test]
    fn json_bodies_keep_their_shape_and_lose_their_secrets() {
        let body = r#"{"email": "ana@x.dev", "password": "hunter2", "remember": true}"#;
        assert_eq!(
            redact_body(body),
            r#"{"email": "ana@x.dev", "password": "[redacted]", "remember": true}"#
        );
        // Nested, in arrays, numbers, and a secret's whole object or array.
        let body = r#"[{"user":{"id":7,"api_key":"sk-1","pin":1234,"tokens":3}},{"credentials":{"user":"a","pass":["x",9,null]}}]"#;
        assert_eq!(
            redact_body(body),
            r#"[{"user":{"id":7,"api_key":"[redacted]","pin":"[redacted]","tokens":3}},{"credentials":{"user":"[redacted]","pass":["[redacted]","[redacted]",null]}}]"#
        );
        // A response with a token, pretty-printed: only the value changes.
        let body = "{\n  \"access_token\": \"eyJhbGciOi.x.y\",\n  \"token_type\": \"Bearer\",\n  \"expires_in\": 3600\n}";
        assert_eq!(
            redact_body(body),
            "{\n  \"access_token\": \"[redacted]\",\n  \"token_type\": \"Bearer\",\n  \"expires_in\": 3600\n}"
        );
        // Quotes, braces and names inside strings are text, not JSON.
        let body = r#"{"note":"say \"password\": {x}","secret":"a\"b}c","n":"é"}"#;
        assert_eq!(
            redact_body(body),
            r#"{"note":"say \"password\": {x}","secret":"[redacted]","n":"é"}"#
        );
    }

    /// The page agent cuts long bodies (`… (5000 chars)`): a cut JSON body is redacted as far as
    /// it goes, a secret cut in the middle too, and the note stays.
    #[test]
    fn bodies_cut_short_are_redacted_too() {
        let body = r#"{"user":"ana","password":"hunter2-and-mo… (5000 chars)"#;
        assert_eq!(
            redact_body(body),
            r#"{"user":"ana","password":"[redacted]"… (5000 chars)"#
        );
        let body = r#"{"items":[1,2],"session":{"id":"s… (9000 chars)"#;
        assert_eq!(
            redact_body(body),
            r#"{"items":[1,2],"session":{"id":"[redacted]"… (9000 chars)"#
        );
        assert_eq!(
            redact_body("a=1&token=abc… (3000 chars)"),
            "a=1&token=[redacted]… (3000 chars)"
        );
        // A note that isn't one is part of the body.
        assert_eq!(cut_note("x… (12 apples)"), ("x… (12 apples)", ""));
    }

    #[test]
    fn forms_and_urls_lose_their_secrets() {
        assert_eq!(
            redact_body("user=ana&password=hunter2&remember=on"),
            "user=ana&password=[redacted]&remember=on"
        );
        // Names as forms write them: `user[password]`, URL-encoded.
        assert_eq!(
            redact_body("user%5Bpassword%5D=x&api%5Fkey=y&q=a+b"),
            "user%5Bpassword%5D=[redacted]&api%5Fkey=[redacted]&q=a+b"
        );
        // FormData as the page agent writes it: values as they are.
        assert_eq!(
            redact_body("name=Ana Lee&client_secret=s 3&file=[file a.png]"),
            "name=Ana Lee&client_secret=[redacted]&file=[file a.png]"
        );
        // A form's value may hold spaces, as FormData's do.
        assert_eq!(redact_body("token=abc is what we send"), "token=[redacted]");
        // Text, HTML and SQL aren't forms.
        for body in [
            "<a href=\"/x?token=abc\">x</a>",
            "select * from t where password = 'x'",
            "plain text",
        ] {
            assert_eq!(redact_body(body), body);
        }
        assert_eq!(
            redact_url("https://api.x.dev/v1/me?api_key=sk-1&page=2"),
            "https://api.x.dev/v1/me?api_key=[redacted]&page=2"
        );
        assert_eq!(
            redact_url("http://localhost:5173/callback#access_token=abc&token_type=bearer"),
            "http://localhost:5173/callback#access_token=[redacted]&token_type=bearer"
        );
        assert_eq!(
            redact_url("https://ana:hunter2@db.x.dev:8443/path?x=1"),
            "https://ana:[redacted]@db.x.dev:8443/path?x=1"
        );
        for url in [
            "http://localhost:5173/assets/app.js",
            "https://x.dev/#/settings",
            "https://ana@x.dev/a?b=c",
            "https://x.dev/search?q=password",
        ] {
            assert_eq!(redact_url(url), url);
        }
    }

    /// What the network log keeps, and so what browser_network answers, holds no secret.
    #[test]
    fn the_network_log_keeps_no_secret() {
        let mut logs = Logs::default();
        let mut login = request("http://localhost:5173/api/login?token=t0k", Some(200), None);
        login.method = "POST".into();
        login.request_body = Some(r#"{"email":"ana@x.dev","password":"hunter2"}"#.into());
        login.response_body = Some(r#"{"ok":true,"session":"s3ss10n"}"#.into());
        logs.push_network(login);
        let answer = logs.network_json(None, false, None).to_string();
        for secret in ["t0k", "hunter2", "s3ss10n"] {
            assert!(!answer.contains(secret), "{secret} in {answer}");
        }
        let kept = &logs.network[0];
        assert_eq!(kept.url, "http://localhost:5173/api/login?token=[redacted]");
        assert_eq!(
            kept.request_body.as_deref(),
            Some(r#"{"email":"ana@x.dev","password":"[redacted]"}"#)
        );
        // Filtering by URL still finds it.
        assert_eq!(logs.network_json(Some("login"), false, None)["matching"], 1);
    }

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\xfe\x10"] {
            assert_eq!(base64_decode(&base64_encode(bytes)).unwrap(), bytes);
        }
        assert!(base64_decode("a$b").is_none());
        assert_eq!(
            base64_decode("iVBO\nRw==\n").unwrap(),
            [0x89, b'P', b'N', b'G']
        );
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
    fn dev_servers_in_a_folder() {
        let mut api = job("/code/shop/api", 3000);
        api.ports.push((3001, crate::ports::BindAddr::LoopbackV4));
        let mut database = job("/code/shop", 5432);
        database.dev = false;
        let jobs = [job("/code/shop/web", 5173), api, database];
        let found = |folder: &str| -> Vec<u16> {
            dev_servers(&jobs, Path::new(folder))
                .iter()
                .map(|j| j.ports[0].0)
                .collect()
        };
        assert_eq!(found("/code/shop"), [5173, 3000]);
        assert_eq!(found("/code/shop/api"), [3000]);
        assert_eq!(found("/code/blog"), Vec::<u16>::new());
        // The lowest port of a server with several.
        assert_eq!(
            dev_server_url(dev_servers(&jobs, Path::new("/code/shop/api"))[0]),
            Some("http://localhost:3000".into())
        );
    }

    /// What `sawit browse` reads of the app's state: the front window's project, and the open
    /// project that holds a folder, the deepest of any window's.
    #[test]
    fn projects_in_the_apps_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (shop, api) = (root.join("shop"), root.join("shop").join("api"));
        std::fs::create_dir_all(api.join("src")).unwrap();
        let state = json!({"windows": [
            {"project": shop, "projects": [shop], "frontmost": false},
            {"project": api, "projects": [root.join("blog"), api], "frontmost": true},
        ]});
        assert_eq!(front_project(&state), Some(api.clone()));
        assert_eq!(project_holding(&state, &api.join("src")), Some(api.clone()));
        assert_eq!(project_holding(&state, &shop), Some(shop.clone()));
        assert_eq!(project_holding(&state, &root), None);
        // A folder named through a link is the project the link leads to.
        let link = root.join("link");
        std::os::unix::fs::symlink(&shop, &link).unwrap();
        assert_eq!(project_holding(&state, &link), Some(shop));
        assert_eq!(front_project(&json!({"windows": []})), None);
        assert_eq!(project_holding(&json!({}), &api), None);
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
    fn fits_devices_to_areas() {
        let f = fit((393, 852), (1000., 1000.));
        assert_eq!(
            f,
            Fit {
                scale: 1.0,
                width: 393.,
                height: 852.
            }
        );
        let f = fit((1440, 900), (720., 900.));
        assert_eq!((f.width, f.height), (720., 450.));
        assert!((f.scale - 0.5).abs() < 1e-3);
        assert!((fit((393, 852), (393., 426.)).scale - 0.5).abs() < 5e-3);
    }

    /// WebKit rounds the view to whole points, and `innerWidth` is whole too:
    /// 393 × 0.3644 = 143.2 pt became 143 pt, so `innerWidth` was 392.
    #[test]
    fn scaled_devices_keep_their_css_width() {
        for (size, area) in [
            ((393, 852), (500., 310.5)),
            ((393, 852), (232., 384.)),
            ((412, 915), (301.7, 600.)),
            ((1440, 900), (700., 435.)),
            ((834, 1194), (333.3, 333.3)),
        ] {
            let f = fit(size, area);
            assert_eq!(f.width, f.width.round(), "{size:?} in {area:?}");
            assert_eq!(f.height, f.height.round(), "{size:?} in {area:?}");
            assert!(f.width <= area.0, "{size:?} in {area:?}");
            let css_width = f64::from(f.width) / f.scale;
            assert_eq!(css_width as u32, size.0, "{size:?} in {area:?}");
            let css_height = f64::from(f.height) / f.scale;
            assert!(
                (css_height - f64::from(size.1)).abs() < 1.0 / f.scale,
                "{size:?}"
            );
        }
    }

    #[test]
    fn devices_by_name() {
        for name in [
            "iphone 15",
            "IPAD PRO 11",
            "Desktop",
            "iPad Pro 11\"",
            "iPad Pro 11″",
        ] {
            assert!(device_by_name(name).is_some(), "{name}");
        }
        assert!(device_by_name("watch").is_none());
        assert_eq!(DEVICES.len(), 6);
    }

    #[test]
    fn resize_modes() {
        assert_eq!(
            resize_mode(Some("responsive"), None, None),
            Ok(ViewportMode::Responsive)
        );
        assert_eq!(
            resize_mode(Some("iPhone 15"), None, None),
            Ok(ViewportMode::Fixed {
                name: Some("iPhone 15".into()),
                width: 393,
                height: 852
            })
        );
        assert_eq!(
            resize_mode(None, Some(800), Some(600)),
            Ok(ViewportMode::Fixed {
                name: None,
                width: 800,
                height: 600
            })
        );
        assert_eq!(
            resize_mode(None, Some(100), Some(600)),
            Err("Width and height must be between 200 and 3840.".into())
        );
        assert_eq!(
            resize_mode(None, None, None),
            Err("Give a device (iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive) or a width and height.".into())
        );
        assert_eq!(
            resize_mode(Some("watch"), None, None),
            Err("Unknown device “watch”. Use iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop or responsive.".into())
        );
        let op: BrowserOp =
            serde_json::from_str(r#"{"op":"resize","device":"iPhone 15"}"#).unwrap();
        assert_eq!(
            op,
            BrowserOp::Resize {
                device: Some("iPhone 15".into()),
                width: None,
                height: None
            }
        );
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            json!({"op": "resize", "device": "iPhone 15"})
        );
        assert!(op.is_native() && op.shows_tab());
    }

    #[test]
    fn annotations_op_is_native_and_stays_in_the_background() {
        let op: BrowserOp = serde_json::from_str(r#"{"op":"annotations","clear":true}"#).unwrap();
        assert_eq!(op, BrowserOp::Annotations { clear: true });
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            json!({"op": "annotations", "clear": true})
        );
        let bare: BrowserOp = serde_json::from_str(r#"{"op":"annotations"}"#).unwrap();
        assert_eq!(bare, BrowserOp::Annotations { clear: false });
        assert!(op.is_native() && !op.shows_tab());
    }

    #[test]
    fn design_ops_are_native_and_round_trip() {
        let target = Target {
            reference: Some("e3".into()),
            selector: None,
        };
        for (op, wire) in [
            (
                BrowserOp::Inspect {
                    target: target.clone(),
                    format: Some("css".into()),
                },
                json!({"op": "inspect", "ref": "e3", "format": "css"}),
            ),
            (
                BrowserOp::Inspect {
                    target: Target {
                        reference: None,
                        selector: Some("h1".into()),
                    },
                    format: None,
                },
                json!({"op": "inspect", "selector": "h1"}),
            ),
            (
                BrowserOp::Design {
                    part: Some("colors".into()),
                },
                json!({"op": "design", "part": "colors"}),
            ),
            (BrowserOp::Design { part: None }, json!({"op": "design"})),
            (
                BrowserOp::ExportDesign {
                    include_videos: true,
                    replace: true,
                },
                json!({"op": "export_design", "include_videos": true, "replace": true}),
            ),
            (BrowserOp::ExportStatus, json!({"op": "export_status"})),
        ] {
            assert_eq!(serde_json::to_value(&op).unwrap(), wire);
            assert_eq!(serde_json::from_value::<BrowserOp>(wire).unwrap(), op);
            assert!(op.is_native(), "{op:?}");
            assert!(!op.shows_tab(), "{op:?}");
        }
        // An export replaces nothing unless asked to.
        let bare: BrowserOp = serde_json::from_str(r#"{"op":"export_design"}"#).unwrap();
        assert_eq!(
            bare,
            BrowserOp::ExportDesign {
                include_videos: false,
                replace: false,
            }
        );
    }

    #[test]
    fn page_script_splices_the_modules() {
        let s = page_script("abc123");
        assert!(!s.contains(MODULES_PLACEHOLDER) && !s.contains(TOKEN_PLACEHOLDER));
        assert!(s.contains("\"abc123\""));
        assert_eq!(
            s.matches("Object.defineProperty(window, \"__sawitAgent\"")
                .count(),
            1
        );
        assert!(s.find("sawit-overlay").unwrap() < s.find("const OPS").unwrap());
        assert!(s.contains("blocked_sheets"));
        assert!(s.find("blocked_sheets").unwrap() < s.find("const OPS").unwrap());
    }

    /// Runs the overlay module alone with the agent's names stubbed, then `test`, which reads
    /// what the module put on `helpers`; answers the JSON `test` returns.
    fn on_overlay(test: &str) -> Value {
        const STUBS: &str = r#"
            var window = globalThis;
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function () {};
            EventTarget.prototype.removeEventListener = function () {};
            var document = { readyState: "loading" };
            var setTimeout = function () { return 0; };
            var TOKEN = "T0K3N";
            var apply = Reflect.apply;
            var sent = [];
            var send = function (kind, data) { sent.push([kind, data]); };
            var resolve = function () { throw new Error("none"); };
            var describe = function () { return ""; };
            var role = function () { return null; };
            var nameOf = function () { return ""; };
            var clip = function (t, n) { return t.slice(0, n); };
            var oneLine = function (t) { return (t || "").replace(/\s+/g, " ").trim(); };
            var modules = {};
            var helpers = {};
        "#;
        let source = format!("{STUBS}\n{OVERLAY_JS}\n{test}");
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        let mut sloppy = rquickjs::context::EvalOptions::default();
        sloppy.strict = false;
        let json: String = ctx.with(|ctx| {
            ctx.eval_with_options::<String, _>(source, sloppy)
                .unwrap_or_else(|e| {
                    let caught = ctx.catch();
                    panic!("the overlay failed: {e:?} {caught:?}")
                })
        });
        serde_json::from_str(&json).unwrap()
    }

    /// The popover sits below its element, flips above it near the bottom of the viewport,
    /// shifts inside the viewport's sides, and goes inside an element taller than the view.
    #[test]
    fn the_popover_flips_and_shifts_to_stay_in_view() {
        let got = on_overlay(
            r#"
            var view = { width: 1200, height: 800 };
            var size = { width: 320, height: 200 };
            var at = function (left, top, right, bottom, v) {
                return helpers.place({ left: left, top: top, right: right, bottom: bottom }, size, v || view);
            };
            JSON.stringify({
                below: at(100, 100, 200, 140),
                above: at(100, 700, 200, 740),
                shifted: at(1100, 100, 1180, 140),
                tall: at(0, -500, 1200, 3000),
                narrow: at(40, 100, 80, 140, { width: 300, height: 800 }),
                low: at(100, 790, 200, 900),
            })
        "#,
        );
        assert_eq!(
            got["below"],
            json!({"left": 100, "top": 148, "side": "below"})
        );
        assert_eq!(
            got["above"],
            json!({"left": 100, "top": 492, "side": "above"})
        );
        assert_eq!(
            got["shifted"],
            json!({"left": 872, "top": 148, "side": "below"})
        );
        assert_eq!(got["tall"], json!({"left": 8, "top": 8, "side": "inside"}));
        // Wider than the viewport: it starts at the margin rather than off its left edge.
        assert_eq!(got["narrow"]["left"], 8);
        // Partly off the bottom: above it, and never below the viewport.
        assert_eq!(
            got["low"],
            json!({"left": 100, "top": 582, "side": "above"})
        );
    }

    /// Names a build tool made up don't go into a selector meant to last.
    #[test]
    fn made_up_class_names_are_left_out_of_selectors() {
        let got = on_overlay(
            r#"
            var names = ["css-1x2y3z", "sc-bdVaJa", "jsx-4012", "radix-:r1:", "w-1/2", "svelte-xyz12",
                "btn-primary", "card", "hero", "nav-link", "is-active"];
            var out = {};
            for (var i = 0; i < names.length; i++) out[names[i]] = helpers.madeUp(names[i]);
            JSON.stringify(out)
        "#,
        );
        for name in [
            "css-1x2y3z",
            "sc-bdVaJa",
            "jsx-4012",
            "radix-:r1:",
            "w-1/2",
            "svelte-xyz12",
        ] {
            assert_eq!(got[name], true, "{name}");
        }
        for name in ["btn-primary", "card", "hero", "nav-link", "is-active"] {
            assert_eq!(got[name], false, "{name}");
        }
    }

    /// The overlay's ops are there for the app, `theme` among them, and the module loads on a
    /// page with none of the DOM it draws with.
    #[test]
    fn the_overlay_adds_its_ops() {
        let got = on_overlay("JSON.stringify(Object.keys(modules))");
        assert_eq!(
            got,
            json!(["overlay", "pick", "markers", "flash", "theme", "presence"])
        );
    }

    /// Runs the overlay module on a stand-in page with just enough DOM for the overlay to draw
    /// (a document, elements, a closed shadow root, styles and timers that are kept rather than
    /// run), then `test`; answers the JSON `test` returns. `run()` runs the timers set so far,
    /// `find(cls)` lists the overlay's nodes of a class, and `page(rect)` makes a page element.
    fn on_page(test: &str) -> Value {
        const PAGE: &str = r##"
            var window = globalThis;
            var innerWidth = 1200, innerHeight = 800;
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function () {};
            EventTarget.prototype.removeEventListener = function () {};
            function Node() {}
            Node.prototype.appendChild = function (child) {
                if (child.parentNode) child.remove();
                (this.kids = this.kids || []).push(child);
                child.parentNode = this;
                return child;
            };
            Object.defineProperty(Node.prototype, "textContent", {
                set: function (t) { this.text = String(t); },
                get: function () { return this.text || ""; },
            });
            Object.defineProperty(Node.prototype, "isConnected", {
                get: function () {
                    var n = this;
                    while (n.parentNode || n.hostNode) n = n.parentNode || n.hostNode;
                    return n === document;
                },
            });
            function Element() {}
            Element.prototype = Object.create(Node.prototype);
            Element.prototype.setAttribute = function (k, v) { (this.attrs = this.attrs || {})[k] = String(v); };
            Element.prototype.removeAttribute = function (k) { if (this.attrs) delete this.attrs[k]; };
            Element.prototype.getAttribute = function (k) { return this.attrs && k in this.attrs ? this.attrs[k] : null; };
            Element.prototype.getBoundingClientRect = function () {
                var r = this.rect || { left: 0, top: 0, width: 0, height: 0 };
                return { left: r.left, top: r.top, width: r.width, height: r.height, x: r.left, y: r.top,
                    right: r.left + r.width, bottom: r.top + r.height };
            };
            Element.prototype.remove = function () {
                var p = this.parentNode;
                if (p) { p.kids.splice(p.kids.indexOf(this), 1); this.parentNode = null; }
            };
            Element.prototype.attachShadow = function () { var s = new ShadowRoot(); s.hostNode = this; return s; };
            Element.prototype.scrollIntoView = function () { this.scrolled = (this.scrolled || 0) + 1; };
            function CSSStyleDeclaration() { this.props = {}; }
            Object.defineProperty(CSSStyleDeclaration.prototype, "cssText", {
                set: function (text) {
                    this.props = {};
                    var parts = String(text).split(";");
                    for (var i = 0; i < parts.length; i++) {
                        var at = parts[i].indexOf(":");
                        if (at > 0) this.props[parts[i].slice(0, at).trim()] = parts[i].slice(at + 1).trim();
                    }
                },
            });
            CSSStyleDeclaration.prototype.setProperty = function (k, v) { this.props[k] = v; };
            function HTMLElement() {}
            HTMLElement.prototype = Object.create(Element.prototype);
            Object.defineProperty(HTMLElement.prototype, "style", {
                get: function () { return this.css || (this.css = new CSSStyleDeclaration()); },
            });
            HTMLElement.prototype.focus = function () {};
            HTMLElement.prototype.blur = function () {};
            function DocumentFragment() {}
            DocumentFragment.prototype = Object.create(Node.prototype);
            var walk = function (n, test) {
                var kids = n.kids || [];
                for (var i = 0; i < kids.length; i++) {
                    if (test(kids[i])) return kids[i];
                    var found = walk(kids[i], test);
                    if (found) return found;
                }
                return null;
            };
            var hasClass = function (n, cls) {
                return (" " + ((n.attrs && n.attrs["class"]) || "") + " ").indexOf(" " + cls + " ") >= 0;
            };
            DocumentFragment.prototype.querySelector = function (sel) {
                if (sel[0] === "#") return walk(this, function (n) { return n.attrs && n.attrs.id === sel.slice(1); });
                return walk(this, function (n) { return hasClass(n, sel.slice(1)); });
            };
            function ShadowRoot() {}
            ShadowRoot.prototype = Object.create(DocumentFragment.prototype);
            // The overlay's markup, read flat: each element with a class or an id, under the root.
            Object.defineProperty(ShadowRoot.prototype, "innerHTML", {
                set: function (html) {
                    var tags = /<(\w+)([^>]*)>/g, m, root = null;
                    while ((m = tags.exec(html))) {
                        if (m[1] === "style") continue;
                        var el = document.createElement(m[1]);
                        var cls = /class="([^"]*)"/.exec(m[2]), id = / id="([^"]*)"/.exec(m[2]);
                        var hidden = /aria-hidden="([^"]*)"/.exec(m[2]);
                        if (!cls && !id) continue;
                        if (cls) el.setAttribute("class", cls[1]);
                        if (id) el.setAttribute("id", id[1]);
                        if (hidden) el.setAttribute("aria-hidden", hidden[1]);
                        if (root) root.appendChild(el); else { this.appendChild(el); root = el; }
                    }
                },
            });
            function Document() {}
            Document.prototype.createElement = function (tag) { var e = new HTMLElement(); e.localName = tag; return e; };
            var document = new Document();
            document.readyState = "complete";
            document.documentElement = document.createElement("html");
            document.appendChild = Node.prototype.appendChild;
            document.appendChild(document.documentElement);
            document.body = document.createElement("body");
            document.documentElement.appendChild(document.body);
            var timers = [];
            var setTimeout = function (f, ms) { timers.push({ f: f, ms: ms }); return timers.length; };
            var clearTimeout = function (id) { if (timers[id - 1]) timers[id - 1].f = null; };
            var setInterval = function () { return 0; };
            var clearInterval = function () {};
            var requestAnimationFrame = function () { return 0; };
            var getComputedStyle = function () {
                return { fontSize: "16px", paddingLeft: "8px", borderLeftWidth: "1px", font: "16px sans-serif" };
            };
            var run = function () {
                var now = timers; timers = [];
                for (var i = 0; i < now.length; i++) if (now[i].f) now[i].f();
                return now.length;
            };
            var TOKEN = "T0K3N";
            var apply = Reflect.apply;
            var sent = [];
            var send = function (kind, data) { sent.push([kind, data]); };
            var elements = {};
            var resolve = function (op) {
                if (!op.selector) return null;
                if (!elements[op.selector]) throw new Error("none");
                return elements[op.selector];
            };
            var page = function (selector, rect, name) {
                var el = document.createElement("button");
                el.rect = rect;
                el.label = name;
                document.body.appendChild(el);
                elements[selector] = el;
                return el;
            };
            var describe = function () { return ""; };
            var role = function (el) { return el.localName === "input" ? "textbox" : "button"; };
            var nameOf = function (el) { return el.label || ""; };
            var clip = function (t, n) { return t.slice(0, n); };
            var oneLine = function (t) { return (t || "").replace(/\s+/g, " ").trim(); };
            var modules = {};
            var helpers = {};
        "##;
        const AFTER: &str = r##"
            var host = function () {
                return walk(document.documentElement, function (n) { return n.localName === "sawit-overlay"; });
            };
            var shadowOf = function () { var h = host(); return h && h.attachedShadow; };
            var find = function (cls) {
                var out = [], h = host();
                if (!h) return out;
                var each = function (n) {
                    var kids = n.kids || [];
                    for (var i = 0; i < kids.length; i++) {
                        if (hasClass(kids[i], cls)) out.push(kids[i]);
                        each(kids[i]);
                    }
                };
                each(h.shadowRootForTests);
                return out;
            };
            var agent = function (id, name, color) { return { id: id, name: name, color: color }; };
            var presence = function (op) { op.op = "presence"; return modules.presence(op); };
        "##;
        // The shadow root is closed: the stand-in keeps it on the host for the test alone.
        let page = PAGE.replace(
            "var s = new ShadowRoot(); s.hostNode = this; return s;",
            "var s = new ShadowRoot(); s.hostNode = this; this.shadowRootForTests = s; return s;",
        );
        let source = format!("{page}\n{OVERLAY_JS}\n{AFTER}\n{test}");
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        let mut sloppy = rquickjs::context::EvalOptions::default();
        sloppy.strict = false;
        let json: String = ctx.with(|ctx| {
            ctx.eval_with_options::<String, _>(source, sloppy)
                .unwrap_or_else(|e| {
                    let caught = ctx.catch();
                    panic!("the page failed: {e:?} {caught:?}")
                })
        });
        serde_json::from_str(&json).unwrap()
    }

    /// An agent's click: its pointer goes to the button's centre in its colour, with its name and
    /// what it does in the pill, the button is ringed, and a click ripples where the pointer
    /// lands. None of it can take an event: the host and the agents' layer let every pointer
    /// event through, and the layer is hidden from assistive technology.
    #[test]
    fn an_agents_cursor_goes_to_what_it_clicks() {
        let got = on_page(
            r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var first = presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"),
                action: "click", selector: "#signup", lead: 120 });
            var cursor = find("agent")[0];
            JSON.stringify({
                first: first,
                transform: cursor.css.props.transform,
                color: cursor.css.props["--c"],
                glide: cursor.css.props["--glide"],
                name: find("name")[0].text,
                doing: find("doing")[0].text,
                ring: find("ring")[0].css.props,
                ripple: find("ripple")[0].css.props,
                ripples: find("ripple").length,
                scrolled: elements["#signup"].scrolled,
                host: host().css.props["pointer-events"],
                hidden_from_at: find("agents")[0].attrs["aria-hidden"],
            })
        "##,
        );
        assert_eq!(
            got["first"],
            json!({"shown": true, "doing": "clicking Sign up", "done": "clicked Sign up",
                   "x": 160, "y": 220})
        );
        assert_eq!(got["transform"], "translate(160px,220px)");
        assert_eq!(got["color"], "#2563eb");
        assert_eq!(got["glide"], "0ms", "the first time it shows where it is");
        assert_eq!(got["name"], "Claude Code");
        assert_eq!(got["doing"], "clicking Sign up");
        assert_eq!(got["ring"]["left"], "100px");
        assert_eq!(got["ring"]["width"], "120px");
        assert_eq!(got["ring"]["--c"], "#2563eb", "in the agent's colour");
        assert_eq!(got["ripple"]["--c"], "#2563eb");
        assert_eq!(got["ripples"], 1);
        assert_eq!(got["scrolled"], 1, "into view, as the click will");
        assert_eq!(got["host"], "none");
        assert_eq!(got["hidden_from_at"], "true");
    }

    /// Two agents at once have a cursor each, in their own colours; an agent that acts again
    /// glides there, and its type shows its caret in the field before the text lands.
    #[test]
    fn a_swarm_has_a_cursor_each_and_typing_shows_the_caret_first() {
        let got = on_page(
            r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var email = page("#email", { left: 400, top: 100, width: 300, height: 36 }, "Email");
            email.localName = "input";
            email.value = "";
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup", lead: 120 });
            presence({ token: TOKEN, agent: agent("b", "Codex · shop", "#c2410c"), action: "hover",
                selector: "#signup", lead: 120 });
            var typed = presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"),
                action: "type", selector: "#email", text: "ada@example.com", lead: 120 });
            var cursors = find("agent");
            JSON.stringify({
                count: cursors.length,
                colors: cursors.map(function (c) { return c.css.props["--c"]; }),
                glide: cursors[0].css.props["--glide"],
                at: cursors[0].css.props.transform,
                typed: typed.doing,
                carets: find("caret").map(function (c) { return c.css.props; }),
                badge: find("typing")[0].text,
            })
        "##,
        );
        assert_eq!(got["count"], 2);
        assert_eq!(got["colors"], json!(["#2563eb", "#c2410c"]));
        assert_eq!(got["glide"], "120ms", "it glides from where it was");
        assert_eq!(got["at"], "translate(550px,118px)");
        assert_eq!(got["typed"], "typing in Email");
        let caret = &got["carets"][0];
        assert_eq!(
            caret["left"], "409px",
            "after the empty field's padding and border"
        );
        assert_eq!(caret["--c"], "#2563eb", "in the agent's colour");
        assert_eq!(got["badge"], "▍");
    }

    /// An idle cursor fades after a few seconds, and goes after 30 s; a page's own call (without
    /// Sawit's token) draws none, and turning cursors off clears them.
    #[test]
    fn agents_cursors_fade_go_and_only_sawit_draws_them() {
        let got = on_page(
            r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            var forged = presence({ token: "nope", agent: agent("x", "Claude Code", "#2563eb"),
                action: "click", selector: "#signup" });
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup" });
            var fades = timers.filter(function (t) { return t.ms === 5000; }).length;
            var goes = timers.filter(function (t) { return t.ms === 30000; }).length;
            run();
            var after = find("agent").length;
            presence({ token: TOKEN, agent: agent("b", "Codex", "#c2410c"), action: "scroll" });
            var scrolled = find("doing")[0].text;
            presence({ token: TOKEN, action: "clear" });
            JSON.stringify({ forged: forged, fades: fades, goes: goes, after: after,
                scrolled: scrolled, cleared: find("agent").length })
        "##,
        );
        assert_eq!(got["forged"], json!({"shown": false}));
        assert_eq!(
            (got["fades"].as_u64(), got["goes"].as_u64()),
            (Some(1), Some(1))
        );
        assert_eq!(got["after"], 0, "gone once its timers ran");
        assert_eq!(got["scrolled"], "scrolling");
        assert_eq!(got["cleared"], 0);
    }

    /// The overlay hides for a screenshot, the agents' cursors with it, unless the user wants
    /// them in agents' screenshots: then only the cursors stay.
    #[test]
    fn screenshots_leave_the_agents_cursors_out() {
        let got = on_page(
            r##"
            page("#signup", { left: 100, top: 200, width: 120, height: 40 }, "Sign up");
            presence({ token: TOKEN, agent: agent("a", "Claude Code", "#2563eb"), action: "click",
                selector: "#signup" });
            var shown = host().css.props.display;
            modules.overlay({ hidden: true });
            var shot = host().css.props.display;
            modules.overlay({ hidden: true, agents: true });
            var kept = [host().css.props.display, find("root")[0].attrs["class"]];
            modules.overlay({ hidden: false });
            var back = [host().css.props.display, find("root")[0].attrs["class"]];
            JSON.stringify({ shown: shown, shot: shot, kept: kept, back: back })
        "##,
        );
        assert_eq!(got["shown"], "block");
        assert_eq!(got["shot"], "none");
        assert_eq!(got["kept"], json!(["block", "root shot"]));
        assert_eq!(got["back"], json!(["block", "root"]));
    }

    #[test]
    fn ipc_reads_picks_hovers_and_keys() {
        let picked = parse_ipc(
            "tok\npicked\n{\"mode\":\"annotate\",\"element\":{\"selector\":\"#a\"}}",
            "tok",
        );
        assert!(matches!(picked, Some(PageEvent::Picked(v)) if v["element"]["selector"] == "#a"));
        let hovered = parse_ipc("tok\nhovered\n{\"selector\":\"#a\"}", "tok");
        assert!(matches!(hovered, Some(PageEvent::Hovered(v)) if v["selector"] == "#a"));
        assert_eq!(
            parse_ipc("tok\nkey\n\"escape\"", "tok"),
            Some(PageEvent::Key("escape".into()))
        );
        assert_eq!(parse_ipc("bad\nkey\n\"escape\"", "tok"), None);
        let changed = parse_ipc("tok\nannotation\n{\"action\":\"remove\",\"n\":2}", "tok");
        assert!(matches!(changed, Some(PageEvent::Annotation(v)) if v["n"] == 2));
        assert_eq!(
            parse_ipc("bad\nannotation\n{\"action\":\"remove\",\"n\":2}", "tok"),
            None
        );
    }

    #[test]
    fn ipc_reads_a_press_in_the_page() {
        assert_eq!(
            parse_ipc("tok\npressed\ntrue", "tok"),
            Some(PageEvent::Pressed)
        );
        assert_eq!(parse_ipc("bad\npressed\ntrue", "tok"), None);
        // The page agent sends it for the user's press only, before the page's own listeners.
        // The agent's own clicks (`browser_click`) and a page script's are synthetic
        // (`isTrusted` false): they give the page no keyboard, so they mustn't move GPUI's.
        let start = AGENT_JS
            .find(r#"addEventListener("pointerdown""#)
            .expect("the agent's press listener");
        let listener = &AGENT_JS[start..];
        let listener = &listener[..listener.find('\n').unwrap_or(listener.len())];
        assert!(
            listener.contains(r#"if (e.isTrusted) send("pressed", true);"#),
            "{listener}"
        );
        assert!(listener.contains("capture: true"), "{listener}");
        // Nothing else in the page script sends it.
        assert_eq!(
            page_script("tok").matches(r#""pressed""#).count(),
            1,
            "another sender of \"pressed\""
        );
    }

    /// Every field of the design contract fixtures is written by the design module.
    #[test]
    fn design_module_writes_the_contract_fields() {
        fn keys(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, v) in m {
                        out.push(k.clone());
                        keys(v, out);
                    }
                }
                Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
                _ => {}
            }
        }
        let mut all = Vec::new();
        for fixture in [
            include_str!("site_design/fixtures/page_scan.json"),
            include_str!("site_design/fixtures/page_rules.json"),
            include_str!("site_design/fixtures/page_style.json"),
        ] {
            keys(&serde_json::from_str(fixture).unwrap(), &mut all);
        }
        for key in all {
            let written = [
                format!("{key}:"),
                format!("{{ {key},"),
                format!(" {key},"),
                format!(" {key} }}"),
            ];
            assert!(
                written.iter().any(|w| DESIGN_JS.contains(w.as_str())),
                "browser_design.js never writes `{key}`"
            );
        }
    }

    /// The design module's color reader handles the syntaxes WebKit computes (Tailwind v4's
    /// palette is oklch), so chromatic buttons and translucent layers are read on such pages.
    #[test]
    fn design_color_math_reads_modern_syntax() {
        let start = DESIGN_JS.find("// ---- Color math").unwrap();
        let end = DESIGN_JS.find("// ---- End of color math").unwrap();
        let source = format!(
            "(() => {{ {}\n const out = (c) => {{ const p = readColor(c); return p ? [Math.round(p[0]), Math.round(p[1]), Math.round(p[2]), p[3], Math.round(chromaOf(p) * 1000) / 1000] : null; }};\n\
             return JSON.stringify([\"oklch(0.623 0.214 259.815)\", \"oklch(0.985 0 0)\", \"oklab(0.628 0.225 0.126)\", \"lab(54.29 80.8 69.89)\", \"lch(54.29 106.84 40.85 / 0.5)\", \"color(display-p3 1 0 0)\", \"color(srgb 0 0.5 1)\", \"rgb(0 113 227 / 50%)\", \"rgba(0, 0, 0, 0)\", \"#0f7fff\", \"oklch(0.7 0.1 none)\", \"hsl(0 0% 0%)\"].map(out)); }})()",
            &DESIGN_JS[start..end]
        );
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        let json: String = ctx.with(|ctx| ctx.eval::<String, _>(source).unwrap());
        let got: Vec<Option<[f64; 5]>> = serde_json::from_str(&json).unwrap();
        let near = |i: usize, rgb: [f64; 3], alpha: f64| {
            let c = got[i].unwrap_or_else(|| panic!("color {i} unread"));
            for k in 0..3 {
                assert!((c[k] - rgb[k]).abs() <= 2.0, "color {i}: {c:?} vs {rgb:?}");
            }
            assert!((c[3] - alpha).abs() < 1e-6, "color {i} alpha: {c:?}");
            c[4]
        };
        // Tailwind v4 blue-500 (just outside sRGB, clipped): chromatic.
        assert!(near(0, [43.0, 127.0, 255.0], 1.0) >= 0.03);
        // Tailwind neutral-50: not.
        assert!(near(1, [250.0, 250.0, 250.0], 1.0) < 0.03);
        near(2, [255.0, 0.0, 0.0], 1.0);
        near(3, [255.0, 0.0, 0.0], 1.0);
        near(4, [255.0, 0.0, 0.0], 0.5);
        // Out of sRGB: clamped.
        near(5, [255.0, 0.0, 0.0], 1.0);
        near(6, [0.0, 128.0, 255.0], 1.0);
        near(7, [0.0, 113.0, 227.0], 0.5);
        near(8, [0.0, 0.0, 0.0], 0.0);
        near(9, [15.0, 127.0, 255.0], 1.0);
        assert!(got[10].is_some());
        // Left to the browser's canvas in the page.
        assert!(got[11].is_none());
    }

    /// Runs the page script (token `T0K3N`) in QuickJS on a stand-in page, then `test`, which
    /// returns a JSON string. The page: a `window` whose `webkit.messageHandlers.ipc` keeps what
    /// is posted (`posted`, `kinds`, `count`), wry's `window.ipc` shim (it looks the handler up
    /// when called), `EventTarget.prototype`'s listener methods (the window's too) keeping the
    /// listeners added in the browser's own list (`listeners`, by type), `fire` and `click` for
    /// the browser dispatching an event to them, and one element, `#buy`, under every point.
    /// The helpers use nothing a test's page script may patch. It all runs as sloppy-mode code,
    /// as WebKit runs a page and the user script (rquickjs would make it strict).
    fn on_stand_in_page(test: &str) -> Value {
        const PAGE: &str = r#"
            var window = globalThis;
            var posted = [];
            var listeners = {};
            function Handler() {}
            Handler.prototype.postMessage = function (s) { posted[posted.length] = String(s); };
            window.webkit = { messageHandlers: { ipc: new Handler() } };
            Object.defineProperty(window, "ipc", { value: Object.freeze({ postMessage: function (s) { window.webkit.messageHandlers.ipc.postMessage(s); } }) });
            function EventTarget() {}
            EventTarget.prototype.addEventListener = function (type, fn) {
                var all = listeners[type] || (listeners[type] = []);
                for (var i = 0; i < all.length; i++) if (all[i] === fn) return;
                all[all.length] = fn;
            };
            EventTarget.prototype.removeEventListener = function (type, fn) {
                var all = listeners[type] || [];
                var left = [];
                for (var i = 0; i < all.length; i++) if (all[i] !== fn) left[left.length] = all[i];
                listeners[type] = left;
            };
            window.addEventListener = EventTarget.prototype.addEventListener;
            window.removeEventListener = EventTarget.prototype.removeEventListener;
            var console = { log: function () {}, info: function () {}, warn: function () {}, error: function () {}, debug: function () {}, trace: function () {} };
            var performance = { now: function () { return 0; } };
            var location = { href: "http://localhost:5173/" };
            var history = { pushState: function () {}, replaceState: function () {} };
            function XMLHttpRequest() {}
            XMLHttpRequest.prototype.open = function () {};
            XMLHttpRequest.prototype.send = function () {};
            var setTimeout = function () { return 0; };
            var clearTimeout = function () {};
            var setInterval = function () { return 0; };
            var clearInterval = function () {};
            var requestAnimationFrame = function () { return 0; };
            var innerWidth = 800, innerHeight = 600;
            var Node = { ELEMENT_NODE: 1, TEXT_NODE: 3, DOCUMENT_FRAGMENT_NODE: 11 };
            function Element() {}
            function ShadowRoot() {}
            var CSS = { escape: function (s) { return s; } };
            var getComputedStyle = function () { return { getPropertyValue: function () { return ""; } }; };
            var document = { readyState: "complete", documentElement: {} };
            var el = new Element();
            el.id = "buy"; el.localName = "button"; el.nodeType = 1; el.className = ""; el.classList = [];
            el.innerText = "Buy"; el.outerHTML = "<button id=buy>Buy</button>"; el.parentElement = null; el.isConnected = true;
            el.getBoundingClientRect = function () { return { x: 1, y: 1, width: 40, height: 20, left: 1, top: 1, right: 41, bottom: 21 }; };
            el.getAttribute = function () { return null; };
            el.hasAttribute = function () { return false; };
            el.getRootNode = function () { return document; };
            el.contains = function (other) { return other === el; };
            document.elementsFromPoint = function () { return [el]; };
            document.querySelectorAll = function () { return [el]; };
            var kinds = function (from) {
                var out = [];
                for (var i = from; i < posted.length; i++) out[out.length] = posted[i].split("\n")[1];
                return out;
            };
            var count = function (from, kind) {
                var all = kinds(from);
                var n = 0;
                for (var i = 0; i < all.length; i++) if (all[i] === kind) n++;
                return n;
            };
            var noop = function () {};
            var fire = function (type, event) {
                var all = listeners[type] || [];
                var now = [];
                for (var i = 0; i < all.length; i++) now[now.length] = all[i];
                for (var j = 0; j < now.length; j++) now[j](event);
            };
            var click = function (trusted) {
                fire("click", { isTrusted: trusted, type: "click", clientX: 5, clientY: 5, preventDefault: noop, stopImmediatePropagation: noop });
            };
            var shiftCmdPeriod = function (trusted) {
                fire("keydown", { isTrusted: trusted, type: "keydown", key: ".", code: trusted ? "Period" : "", metaKey: true, shiftKey: true, altKey: false, preventDefault: noop, stopImmediatePropagation: noop });
            };
        "#;
        let source = format!("{PAGE}\n{}\n{test}", page_script("T0K3N"));
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        let mut sloppy = rquickjs::context::EvalOptions::default();
        sloppy.strict = false;
        let json: String = ctx.with(|ctx| {
            ctx.eval_with_options::<String, _>(source, sloppy)
                .unwrap_or_else(|e| {
                    let caught = ctx.catch();
                    let why = caught
                        .as_exception()
                        .map(|x| format!("{:?} {:?}", x.message(), x.stack()));
                    panic!("the stand-in page failed: {e:?} {why:?}")
                })
        });
        serde_json::from_str(&json).unwrap()
    }

    /// A page script that patches the IPC handler's `postMessage` (on its prototype, which
    /// wry's shim looks up on every call) sees what the agent posts after that; the token
    /// signing it must not be in it, or the page could sign messages of its own.
    #[test]
    fn page_scripts_never_see_the_token() {
        let got = on_stand_in_page(
            r#"
            var stolen = [];
            Handler.prototype.postMessage = function (s) { stolen.push(String(s)); };
            window.webkit.messageHandlers.ipc.postMessage = Handler.prototype.postMessage;
            console.error("boom");
            JSON.stringify({ stolen: stolen.filter(function (m) { return m.indexOf("T0K3N") >= 0; }).length, sent: kinds(0) })
        "#,
        );
        assert_eq!(got["stolen"], 0, "{got}");
        assert_eq!(got["sent"], json!(["console"]), "{got}");
    }

    /// The picker takes only the user's clicks: a page that arms it through `__sawitAgent`
    /// and dispatches a click of its own picks nothing.
    #[test]
    fn synthetic_clicks_pick_nothing() {
        let got = on_stand_in_page(
            r#"
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            var from = posted.length;
            click(false);
            var forged = count(from, "picked");
            click(true);
            var real = count(from, "picked") - forged;
            JSON.stringify({ forged: forged, real: real })
        "#,
        );
        assert_eq!(got, json!({"forged": 0, "real": 1}));
    }

    /// The picker's listeners never reach a page script: one that kept them could call them
    /// with an `isTrusted: true` object of its own, and pick (with a screenshot, taking the
    /// keyboard) while the user is annotating. The page here patched, before the picker was
    /// armed, every way they could pass it: the window's and `EventTarget.prototype`'s listener
    /// methods, the array iterator (`for…of` and destructuring), `Array.prototype`'s callbacks
    /// and `Reflect.apply`.
    #[test]
    fn pages_never_get_the_pickers_listeners() {
        let got = on_stand_in_page(
            r#"
            var kept = [];
            var keep = function (v) {
                if (typeof v === "function") kept[kept.length] = v;
                else if (v && typeof v === "object" && typeof v.length === "number")
                    for (var i = 0; i < v.length; i++) if (typeof v[i] === "function") kept[kept.length] = v[i];
            };
            var add = EventTarget.prototype.addEventListener;
            var remove = EventTarget.prototype.removeEventListener;
            var keptAdd = function (type, fn, options) { keep(fn); return add.call(this, type, fn, options); };
            var keptRemove = function (type, fn, options) { keep(fn); return remove.call(this, type, fn, options); };
            window.addEventListener = keptAdd;
            window.removeEventListener = keptRemove;
            EventTarget.prototype.addEventListener = keptAdd;
            EventTarget.prototype.removeEventListener = keptRemove;
            var iterator = Object.getPrototypeOf([][Symbol.iterator]());
            var next = iterator.next;
            iterator.next = function () {
                var r = next.call(this);
                if (r && !r.done) keep(r.value);
                return r;
            };
            var methods = ["forEach", "map", "filter", "some", "every", "find", "findIndex", "reduce", "flatMap", "slice", "concat", "includes", "indexOf"];
            for (var m = 0; m < methods.length; m++) (function (name) {
                var real = Array.prototype[name];
                Array.prototype[name] = function () {
                    for (var i = 0; i < this.length; i++) keep(this[i]);
                    for (var j = 0; j < arguments.length; j++) keep(arguments[j]);
                    return real.apply(this, arguments);
                };
            })(methods[m]);
            var reflectApply = Reflect.apply;
            Reflect.apply = function (f, self, args) { keep(f); keep(args); return reflectApply(f, self, args); };

            // Sawit arms the picker (the user opened Annotate), stops it and arms it again.
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            __sawitAgent.run({ op: "pick", mode: "off" });
            __sawitAgent.run({ op: "pick", mode: "annotate" });
            var from = posted.length;
            var event = { isTrusted: true, type: "click", key: ".", code: "Period", metaKey: true, shiftKey: true, altKey: false,
                clientX: 5, clientY: 5, deltaY: -1, preventDefault: noop, stopImmediatePropagation: noop };
            for (var i = 0; i < kept.length; i++) {
                try { kept[i](event); } catch (_) {}
            }
            var forged = kinds(from).length;
            click(true);
            JSON.stringify({ forged: forged, real: count(from, "picked") })
        "#,
        );
        assert_eq!(got, json!({"forged": 0, "real": 1}));
    }

    /// ⇧⌘. opens Annotate and arms the picker, and the page's script answers it from no one: a
    /// page can dispatch one too, and the user's own is the keymap's, which sees ⌘ keys before
    /// the page does, so a key the keymap doesn't bind to Annotate doesn't toggle it. (An
    /// agent's `browser_press_key "Meta+Shift+."` goes to Sawit.)
    #[test]
    fn the_page_script_answers_no_shift_cmd_period() {
        let got = on_stand_in_page(
            r#"
            var from = posted.length;
            shiftCmdPeriod(false);
            var synthetic = count(from, "key");
            shiftCmdPeriod(true);
            JSON.stringify({ synthetic: synthetic, real: count(from, "key") - synthetic })
        "#,
        );
        assert_eq!(got, json!({"synthetic": 0, "real": 0}));
    }

    /// What the user typed into the page's password fields never reaches the network log: a
    /// request that carries it, under any name, as typed, URL-encoded or in a JSON string, is
    /// logged with [redacted] there (Sawit then hides fields named like secrets). A value shorter
    /// than 4 characters is left alone: it would hide the same letters everywhere.
    #[test]
    fn typed_passwords_never_reach_the_network_log() {
        let got = on_stand_in_page(
            r#"
            var typed = [{ value: 'hunter2"!' }, { value: "abc" }];
            document.querySelectorAll = function (selector) {
                return selector.indexOf("password") >= 0 ? typed : [el];
            };
            var request = function (url, body) {
                var xhr = new XMLHttpRequest();
                var done = null;
                xhr.addEventListener = function (type, fn) { if (type === "loadend") done = fn; };
                xhr.status = 200;
                xhr.responseType = "";
                xhr.responseText = "{}";
                xhr.open("POST", url);
                xhr.send(body);
                done();
                return JSON.parse(posted[posted.length - 1].split("\n").slice(2).join("\n"));
            };
            var json = request("http://localhost:5173/login", '{"user":"ana","pw":"hunter2\\"!","code":"abc"}');
            var form = request("http://localhost:5173/login?p=" + encodeURIComponent('hunter2"!'), "u=ana&p=hunter2%22!");
            JSON.stringify({ json: json.request_body, url: form.url, form: form.request_body })
        "#,
        );
        assert_eq!(
            got,
            json!({
                "json": r#"{"user":"ana","pw":"[redacted]","code":"abc"}"#,
                "url": "http://localhost:5173/login?p=[redacted]",
                "form": "u=ana&p=[redacted]",
            })
        );
    }

    /// The agent, with the modules spliced into it, is strict-mode code. WebKit runs a user
    /// script as sloppy code unless it says otherwise, and JavaScriptCore answers
    /// `Function.prototype.caller` for a sloppy function, arrows included: a page function the
    /// agent calls (a replaced `JSON.stringify` or `preventDefault`, a getter on an event) got
    /// `send`, which signs with the token, or the picker's own `click`, from its caller. With
    /// either a page opened Annotate and picked for itself. QuickJS has no `caller`, but tells
    /// the two apart: reading it throws for a strict function and not for a sloppy one.
    #[test]
    fn the_agent_is_strict_mode_code() {
        // The agent's hooks, on a page that is sloppy (as pages are).
        let got = on_stand_in_page(
            r#"
            var strict = function (f) { try { f.caller; return false; } catch (_) { return true; } };
            JSON.stringify({ console: strict(console.log), xhr: strict(XMLHttpRequest.prototype.send),
                history: strict(history.pushState), page: strict(function () {}) })
        "#,
        );
        assert_eq!(
            got,
            json!({"console": true, "xhr": true, "history": true, "page": false})
        );
        // So is every other function in it: the directive is first in the one function that
        // holds the agent and its modules.
        let mut code = AGENT_JS
            .lines()
            .skip_while(|line| line.starts_with("//") || line.trim().is_empty());
        assert_eq!(code.next(), Some("(() => {"));
        assert_eq!(code.next().map(str::trim), Some("\"use strict\";"));
        assert!(AGENT_JS.trim_end().ends_with("})();"));
    }

    /// `browser_evaluate` on a page whose Content-Security-Policy forbids `eval` (helmet's
    /// default `script-src 'self'`): the page's `Function` and `eval` refuse, as WebKit's do
    /// there, and the script still runs, as an expression or as a function body.
    #[test]
    fn evaluate_runs_where_the_page_forbids_eval() {
        let run = |call: String| {
            let test = format!(
                r#"
                var refuse = function () {{ throw new EvalError("Refused to evaluate a string as JavaScript because 'unsafe-eval' is not an allowed source of script"); }};
                Function.prototype.constructor = refuse;
                globalThis.Function = refuse;
                globalThis.eval = refuse;
                document.title = "Shop";
                {call}
            "#
            );
            on_stand_in_page(&test)
        };
        assert_eq!(
            run(evaluate_call("1 + 1", false)),
            json!({"ok": true, "value": 2})
        );
        assert_eq!(
            run(evaluate_call(
                "const t = document.title;\nreturn t + '!'",
                true
            )),
            json!({"ok": true, "value": "Shop!"})
        );
        // A throw is the script's error.
        assert_eq!(run(evaluate_call("null.x", false))["ok"], json!(false));
    }

    /// `browser_evaluate` picks its form before WebKit compiles anything. WebKit reports a
    /// compile's `SyntaxError` to the page as an error of its own (the page's console,
    /// `window.onerror`, the notes' console errors), so trying the expression there first left
    /// one behind for every script written as a function body.
    #[test]
    fn evaluate_picks_its_form_before_webkit_compiles() {
        // An expression, as WebKit tried first, also when it reads as a body too.
        for script in [
            "document.title",
            "1 // c",
            "({a: 1})",
            "{a: 1}",
            "x = 5",
            "async () => (await fetch('/a')).status",
            "`${location.host}/a`",
            "/a+/.test(document.title)",
            "010",
        ] {
            assert!(!evaluate_as_body(script), "{script}");
        }
        // A function body, at once.
        for script in [
            "return 1",
            "const t = 1; return t",
            "document.title;",
            "if (!document.body) return null\nreturn document.body.innerText // the text",
            "with (document) return title",
            "for (const a of document.links) a.remove()",
        ] {
            assert!(evaluate_as_body(script), "{script}");
        }
        // Neither compiles here (a real error, or syntax QuickJS lacks): WebKit says, trying the
        // expression first.
        for script in ["1 +", "return (", "}", "a\0b"] {
            assert!(!evaluate_as_body(script), "{script:?}");
        }
    }

    /// The agent's script is only compiled here, never run: it could close the wrapper and run
    /// anything (loop forever) in Sawit's own QuickJS.
    #[test]
    fn evaluate_forms_are_compiled_not_run() {
        let rt = rquickjs::Runtime::new().unwrap();
        let cx = rquickjs::Context::full(&rt).unwrap();
        cx.with(|cx| {
            let ran = || {
                !cx.eval::<rquickjs::Value, _>("globalThis.ran")
                    .unwrap()
                    .is_undefined()
            };
            assert!(compiles(&cx, "globalThis.ran = true"));
            assert!(!ran());
            assert!(compiles(&cx, "globalThis.ran = true; while (true) {}"));
            assert!(!ran());
            // Compiled as WebKit compiles a script: sloppy-mode code.
            assert!(compiles(&cx, "with (document) title"));
            // A failed compile leaves nothing pending.
            assert!(!compiles(&cx, "1 +"));
            assert!(!cx.has_exception());
        });
    }

    #[test]
    fn agent_script_parses() {
        assert!(AGENT_JS.contains(TOKEN_PLACEHOLDER) && AGENT_JS.contains("__sawitAgent"));
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            // Compiles without running: there is no DOM here.
            let source = format!("(function () {{ {} }})", page_script("t"));
            ctx.eval::<rquickjs::Value, _>(source)
                .map(|_| ())
                .unwrap_or_else(|e| panic!("agent script doesn't parse: {e:?}"));
        });
    }
}
