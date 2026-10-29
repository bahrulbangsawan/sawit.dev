//! `browser_*`: the running app's browser tab (see `sawit_core::browser`),
//! for an agent checking the app it is building: open pages, read them as an
//! outline with element refs, click and type, wait, screenshot, and read the
//! console and network logs.
//!
//! The app answers each op at once; waiting (for a load, a text, a promise)
//! is done here by polling.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content};
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Command;
use sawit_core::browser::{BrowserOp, Target, pick_dev_server};
use serde_json::{Value, json};

use crate::app::{base64_decode, pretty};
use crate::{Sawit, fail, ok, tri};

const POLL: Duration = Duration::from_millis(200);
const DEFAULT_LOAD_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
/// How long an action waits to see whether it started a navigation.
const SETTLE: Duration = Duration::from_millis(250);

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NavigateArgs {
    #[schemars(
        description = "Address to open: a URL, localhost:5173, or words to search. Omit (with no action) to open the project's running dev server"
    )]
    pub url: Option<String>,
    #[schemars(description = "back, forward or reload instead of a url")]
    pub action: Option<String>,
    #[schemars(description = "How long to wait for the page to load, in ms (default 15000)")]
    pub timeout_ms: Option<u64>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ElementArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element ref from browser_snapshot, e.g. e12"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ClickArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element ref from browser_snapshot, e.g. e12"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Double-click")]
    #[serde(default)]
    pub double: bool,
    #[schemars(description = "left (default) or right")]
    pub button: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TypeArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Field ref from browser_snapshot; default the focused element"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    pub text: String,
    #[schemars(description = "Empty the field first")]
    #[serde(default)]
    pub clear: bool,
    #[schemars(description = "Press Enter afterwards (submits a form)")]
    #[serde(default)]
    pub submit: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeyArgs {
    #[schemars(
        description = "Enter, Tab, Shift+Tab, Escape, Backspace, Delete, Space, ArrowDown, PageDown, Home, End, Meta+a, or one character"
    )]
    pub key: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SelectArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "The <select>'s ref from browser_snapshot"
    )]
    pub reference: Option<String>,
    #[schemars(description = "Or a CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Option values or visible labels (several for a multiple select)")]
    pub values: Vec<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScrollArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Element to scroll (or, with no dx/dy, to bring into view); default the page"
    )]
    pub reference: Option<String>,
    pub selector: Option<String>,
    #[schemars(description = "Pixels right (negative: left)")]
    pub dx: Option<f64>,
    #[schemars(description = "Pixels down (negative: up)")]
    pub dy: Option<f64>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WaitArgs {
    #[schemars(description = "Wait until this text is on the page (any case)")]
    pub text: Option<String>,
    #[schemars(description = "Wait until this text is gone")]
    pub text_gone: Option<String>,
    #[schemars(description = "Wait until a visible element matches this CSS selector")]
    pub selector: Option<String>,
    #[schemars(description = "Or just wait this many ms")]
    pub time_ms: Option<u64>,
    #[schemars(description = "Give up after this many ms (default and most 30000)")]
    pub timeout_ms: Option<u64>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ScreenshotArgs {
    #[serde(rename = "ref")]
    #[schemars(
        rename = "ref",
        description = "Capture only this element (ref from browser_snapshot)"
    )]
    pub reference: Option<String>,
    pub selector: Option<String>,
    #[schemars(description = "Where to write the PNG; default Sawit's cache folder")]
    pub output_path: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ConsoleArgs {
    #[schemars(description = "error, warning, info, log or debug; default all")]
    pub level: Option<String>,
    #[schemars(description = "Only messages containing this text (any case)")]
    pub pattern: Option<String>,
    #[schemars(description = "Newest messages to return (default 100)")]
    pub limit: Option<usize>,
    #[schemars(description = "Empty the log after reading")]
    #[serde(default)]
    pub clear: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NetworkArgs {
    #[schemars(description = "Only URLs containing this text (any case)")]
    pub pattern: Option<String>,
    #[schemars(description = "Only failed requests (status 400+ or no response)")]
    #[serde(default)]
    pub failed_only: bool,
    #[schemars(description = "Newest requests to return (default 100)")]
    pub limit: Option<usize>,
    #[schemars(description = "Empty the log after reading")]
    #[serde(default)]
    pub clear: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EvaluateArgs {
    #[schemars(
        description = "A JS expression (document.title), or a function body with return; a returned promise is awaited"
    )]
    pub script: String,
    #[schemars(description = "How long to await a promise, in ms (default 10000)")]
    pub timeout_ms: Option<u64>,
    pub project: Option<String>,
}

/// `back` / `forward` / `reload`, or the page at `url`.
fn navigate_op(url: Option<String>, action: Option<&str>) -> Result<Option<BrowserOp>, String> {
    match (url.filter(|u| !u.trim().is_empty()), action.map(str::trim)) {
        (_, Some("back")) => Ok(Some(BrowserOp::Back)),
        (_, Some("forward")) => Ok(Some(BrowserOp::Forward)),
        (_, Some("reload")) => Ok(Some(BrowserOp::Reload)),
        (_, Some(other)) if !other.is_empty() => Err(format!(
            "Unknown action “{other}”: use back, forward or reload."
        )),
        (Some(url), _) => Ok(Some(BrowserOp::Navigate { url })),
        (None, _) => Ok(None),
    }
}

fn target(reference: Option<String>, selector: Option<String>) -> Target {
    let some = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Target {
        reference: some(reference),
        selector: some(selector),
    }
}

fn required(target: Target) -> Result<Target, String> {
    if target.is_empty() {
        return Err("Give a ref (from browser_snapshot) or a CSS selector.".into());
    }
    Ok(target)
}

fn timeout(ms: Option<u64>, default: Duration) -> Duration {
    ms.map(Duration::from_millis)
        .unwrap_or(default)
        .min(MAX_TIMEOUT)
}

/// Where a screenshot goes when no path is given.
fn screenshot_path(cache: &std::path::Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    cache
        .join("browser")
        .join(format!("screenshot-{stamp}.png"))
}

impl Sawit {
    async fn browser(&self, project: &Option<String>, op: BrowserOp) -> Result<Value, String> {
        self.app_call(project.clone(), Command::Browser { op })
            .await
    }

    /// Polls until the page has loaded (or `limit` passes); the final state,
    /// with `"timed_out": true` when it was still loading.
    async fn wait_loaded(
        &self,
        project: &Option<String>,
        limit: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + limit;
        loop {
            let state = self.browser(project, BrowserOp::State).await?;
            if state["loading"] != true {
                return Ok(state);
            }
            if Instant::now() >= deadline {
                let mut state = state;
                state["timed_out"] = true.into();
                return Ok(state);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// After a click or key: gives a navigation it started time to begin,
    /// then waits for it, so the answer shows where the page ended up.
    async fn after_action(&self, project: &Option<String>, mut result: Value) -> CallToolResult {
        tokio::time::sleep(SETTLE).await;
        if let Ok(state) = self.wait_loaded(project, DEFAULT_LOAD_TIMEOUT).await {
            result["page"] = json!({ "url": state["url"], "title": state["title"], "loading": state["loading"] });
        }
        ok(pretty(&result))
    }

    /// The URL to open when none is given: the dev server running from the
    /// project folder, or the only one running.
    async fn dev_server_url(&self, project: &Option<String>) -> Result<String, String> {
        let jobs = sawit_core::probe::listening_jobs()
            .await
            .map_err(|e| e.to_string())?;
        let folder = project
            .as_deref()
            .map(crate::expand_home)
            .unwrap_or_else(|| PathBuf::from("/"));
        pick_dev_server(&jobs, &folder).ok_or_else(|| {
            "Give a url: no single dev server is running for this project (see list_ports).".into()
        })
    }
}

#[tool_router(router = browser_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Open a page in Sawit's browser tab (or go back, forward, reload) and wait for it to load. With no url, opens the project's running dev server. Returns url, title, console error and failed request counts"
    )]
    async fn browser_navigate(&self, Parameters(a): Parameters<NavigateArgs>) -> CallToolResult {
        let op = match tri!(navigate_op(a.url, a.action.as_deref())) {
            Some(op) => op,
            None => BrowserOp::Navigate {
                url: tri!(self.dev_server_url(&a.project).await),
            },
        };
        tri!(self.browser(&a.project, op).await);
        tokio::time::sleep(SETTLE).await;
        let limit = timeout(a.timeout_ms, DEFAULT_LOAD_TIMEOUT);
        let mut state = tri!(self.wait_loaded(&a.project, limit).await);
        if state["timed_out"] == true {
            state["note"] = "Still loading; browser_snapshot works on what has arrived.".into();
        }
        ok(pretty(&state))
    }

    #[tool(
        description = "Read the browser tab's page as an accessibility outline: roles, names, values and states, each element with a ref (e12) for browser_click, browser_type and the others. Refs reset on every snapshot and navigation"
    )]
    async fn browser_snapshot(&self, Parameters(a): Parameters<PageArgs>) -> CallToolResult {
        let v = tri!(self.browser(&a.project, BrowserOp::Snapshot).await);
        let mut out = format!(
            "Page: {}\nURL: {}\n",
            v["title"].as_str().unwrap_or_default(),
            v["url"].as_str().unwrap_or_default()
        );
        if v["ready"] != "complete" {
            out.push_str(&format!(
                "Still loading ({}).\n",
                v["ready"].as_str().unwrap_or("?")
            ));
        }
        if let Some(modal) = v["modal"].as_str() {
            out.push_str(&format!("A dialog is open: {modal}\n"));
        }
        out.push('\n');
        out.push_str(v["snapshot"].as_str().unwrap_or_default());
        ok(out)
    }

    #[tool(
        description = "Click an element in the browser tab by ref (from browser_snapshot) or CSS selector; double or right click too. Fails, naming the cover, when another element is on top. Waits for a navigation it starts"
    )]
    async fn browser_click(&self, Parameters(a): Parameters<ClickArgs>) -> CallToolResult {
        let op = BrowserOp::Click {
            target: tri!(required(target(a.reference, a.selector))),
            double: a.double,
            button: a.button,
        };
        let v = tri!(self.browser(&a.project, op).await);
        self.after_action(&a.project, v).await
    }

    #[tool(
        description = "Hover an element in the browser tab by ref or CSS selector (runs hover handlers: menus, tooltips)"
    )]
    async fn browser_hover(&self, Parameters(a): Parameters<ElementArgs>) -> CallToolResult {
        let op = BrowserOp::Hover {
            target: tri!(required(target(a.reference, a.selector))),
        };
        let v = tri!(self.browser(&a.project, op).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Type text into a field in the browser tab (by ref or CSS selector, else the focused one); clear it first, submit with Enter after. Works with React and other controlled inputs"
    )]
    async fn browser_type(&self, Parameters(a): Parameters<TypeArgs>) -> CallToolResult {
        let submit = a.submit;
        let op = BrowserOp::Type {
            target: target(a.reference, a.selector),
            text: a.text,
            clear: a.clear,
            submit,
        };
        let v = tri!(self.browser(&a.project, op).await);
        if submit {
            return self.after_action(&a.project, v).await;
        }
        ok(pretty(&v))
    }

    #[tool(
        description = "Press a key in the browser tab's focused element: Enter, Tab, Escape, Backspace, arrows, PageDown, Meta+a, one character"
    )]
    async fn browser_press_key(&self, Parameters(a): Parameters<KeyArgs>) -> CallToolResult {
        let v = tri!(
            self.browser(&a.project, BrowserOp::Press { key: a.key })
                .await
        );
        self.after_action(&a.project, v).await
    }

    #[tool(description = "Pick options in a <select> in the browser tab by value or visible label")]
    async fn browser_select_option(&self, Parameters(a): Parameters<SelectArgs>) -> CallToolResult {
        if a.values.is_empty() {
            return fail("Give at least one value.");
        }
        let op = BrowserOp::Select {
            target: tri!(required(target(a.reference, a.selector))),
            values: a.values,
        };
        let v = tri!(self.browser(&a.project, op).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Scroll the browser tab's page (or an element) by dx/dy pixels, or bring an element into view. Returns the scroll position and whether it reached the bottom"
    )]
    async fn browser_scroll(&self, Parameters(a): Parameters<ScrollArgs>) -> CallToolResult {
        let target = target(a.reference, a.selector);
        let (dx, dy) = (a.dx.unwrap_or_default(), a.dy.unwrap_or_default());
        let dy = if target.is_empty() && dx == 0. && dy == 0. {
            600.
        } else {
            dy
        };
        let v = tri!(
            self.browser(&a.project, BrowserOp::Scroll { target, dx, dy })
                .await
        );
        ok(pretty(&v))
    }

    #[tool(
        description = "Wait in the browser tab until a text appears, a text is gone, or a CSS selector matches a visible element (or just wait time_ms). At most 30 s"
    )]
    async fn browser_wait_for(&self, Parameters(a): Parameters<WaitArgs>) -> CallToolResult {
        if let Some(ms) = a.time_ms {
            tokio::time::sleep(Duration::from_millis(ms).min(MAX_TIMEOUT)).await;
            return ok(format!(
                "Waited {} ms.",
                ms.min(MAX_TIMEOUT.as_millis() as u64)
            ));
        }
        let (text, gone) = match (a.text, a.text_gone) {
            (Some(t), _) => (Some(t), false),
            (None, Some(t)) => (Some(t), true),
            (None, None) => (None, false),
        };
        if text.is_none() && a.selector.is_none() {
            return fail("Give text, text_gone, selector or time_ms.");
        }
        let deadline = Instant::now() + timeout(a.timeout_ms, MAX_TIMEOUT);
        let started = Instant::now();
        loop {
            let op = BrowserOp::Query {
                text: text.clone(),
                selector: a.selector.clone(),
            };
            // A page mid-load has no agent yet: keep waiting.
            if let Ok(v) = self.browser(&a.project, op).await
                && v["found"].as_bool() == Some(!gone)
            {
                return ok(format!(
                    "Done after {} ms: {}",
                    started.elapsed().as_millis(),
                    v["url"].as_str().unwrap_or_default()
                ));
            }
            if Instant::now() >= deadline {
                let what = match (&text, &a.selector) {
                    (Some(t), _) if gone => format!("“{t}” to go away"),
                    (Some(t), _) => format!("“{t}”"),
                    (None, Some(s)) => format!("“{s}” to match"),
                    _ => String::new(),
                };
                return fail(format!(
                    "Timed out after {} ms waiting for {what}. Check browser_snapshot or browser_console.",
                    started.elapsed().as_millis()
                ));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    #[tool(
        description = "Screenshot the browser tab's visible page (or one element by ref/selector) as PNG: returned as an image and written to a file"
    )]
    async fn browser_screenshot(
        &self,
        Parameters(a): Parameters<ScreenshotArgs>,
    ) -> CallToolResult {
        let op = BrowserOp::Screenshot {
            target: target(a.reference, a.selector),
        };
        let v = tri!(self.browser(&a.project, op).await);
        let encoded = v["png_base64"].as_str().unwrap_or_default().to_string();
        let bytes = tri!(base64_decode(&encoded));
        if bytes.is_empty() {
            return fail("The browser sent an empty screenshot.");
        }
        let path = a
            .output_path
            .map(|p| crate::expand_home(&p))
            .unwrap_or_else(|| screenshot_path(&self.paths.cache_dir()));
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return fail(format!("Unable to create {}: {e}", dir.display()));
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            return fail(format!("Unable to write {}: {e}", path.display()));
        }
        CallToolResult::success(vec![
            Content::image(encoded, "image/png"),
            Content::text(format!(
                "{}×{} screenshot written to {}",
                v["width"],
                v["height"],
                path.display()
            )),
        ])
    }

    #[tool(
        description = "Read the browser tab's console messages and page errors (uncaught exceptions, promise rejections), newest last; filter by level and text"
    )]
    async fn browser_console(&self, Parameters(a): Parameters<ConsoleArgs>) -> CallToolResult {
        let op = BrowserOp::Console {
            level: a.level,
            pattern: a.pattern,
            limit: a.limit,
            clear: a.clear,
        };
        let v = tri!(self.browser(&a.project, op).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Read the browser tab's requests: fetch/XHR with method, status, duration and (cut) request and response bodies, plus scripts, styles and images; filter by URL text or failures"
    )]
    async fn browser_network(&self, Parameters(a): Parameters<NetworkArgs>) -> CallToolResult {
        let op = BrowserOp::Network {
            pattern: a.pattern,
            failed_only: a.failed_only,
            limit: a.limit,
            clear: a.clear,
        };
        let v = tri!(self.browser(&a.project, op).await);
        ok(pretty(&v))
    }

    #[tool(
        description = "Run JavaScript in the browser tab's page and return its JSON value (DOM nodes are described); a returned promise is awaited"
    )]
    async fn browser_evaluate(&self, Parameters(a): Parameters<EvaluateArgs>) -> CallToolResult {
        let v = tri!(
            self.browser(&a.project, BrowserOp::Evaluate { script: a.script })
                .await
        );
        let Some(id) = v["pending"].as_u64() else {
            return ok(pretty(&v["value"]));
        };
        let deadline = Instant::now() + timeout(a.timeout_ms, Duration::from_secs(10));
        loop {
            let v = tri!(self.browser(&a.project, BrowserOp::Settled { id }).await);
            if v["done"] == true {
                return ok(pretty(&v["value"]));
            }
            if Instant::now() >= deadline {
                return fail("The promise didn't settle in time.");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_ops() {
        assert_eq!(navigate_op(None, Some("back")), Ok(Some(BrowserOp::Back)));
        assert_eq!(
            navigate_op(Some("localhost:3000".into()), None),
            Ok(Some(BrowserOp::Navigate {
                url: "localhost:3000".into()
            }))
        );
        assert_eq!(navigate_op(Some("  ".into()), None), Ok(None));
        assert!(
            navigate_op(None, Some("up"))
                .unwrap_err()
                .contains("reload")
        );
    }

    #[test]
    fn targets() {
        assert!(required(target(None, Some(" ".into()))).is_err());
        assert_eq!(
            required(target(Some("e4".into()), None))
                .unwrap()
                .reference
                .as_deref(),
            Some("e4")
        );
    }

    #[test]
    fn timeouts_are_capped() {
        assert_eq!(timeout(Some(90_000), DEFAULT_LOAD_TIMEOUT), MAX_TIMEOUT);
        assert_eq!(timeout(None, DEFAULT_LOAD_TIMEOUT), DEFAULT_LOAD_TIMEOUT);
    }
}
