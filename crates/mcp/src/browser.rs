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
use sawit_core::browser::{BrowserOp, Target, front_project, pick_dev_server, resize_mode};
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
        description = "Enter, Tab, Shift+Tab, Escape, Backspace, Delete, Space, ArrowDown, PageDown, Home, End, Meta+a, or one character. Meta+Shift+. is Sawit's: it opens or closes the Annotate panel"
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
    #[schemars(
        description = "Absolute path (or ~/…) to write the PNG to; default Sawit's cache folder"
    )]
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
    #[schemars(
        description = "Empty the tab's console log after reading: every message kept so far, not only those returned, and with them the page's console errors that the user's Annotate notes add when copied or sent to a terminal"
    )]
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
    #[schemars(
        description = "Empty the tab's network log after reading: every request kept so far, not only those returned, so the failed-request count browser_navigate answers starts again from zero"
    )]
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnnotationsArgs {
    #[schemars(description = "Remove the annotations after reading them")]
    #[serde(default)]
    pub clear: bool,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResizeArgs {
    #[schemars(
        description = "iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop, or responsive to fill the tab"
    )]
    pub device: Option<String>,
    #[schemars(description = "Viewport width in CSS px (with height)")]
    pub width: Option<u32>,
    #[schemars(description = "Viewport height in CSS px (with width)")]
    pub height: Option<u32>,
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

pub(crate) fn target(reference: Option<String>, selector: Option<String>) -> Target {
    let some = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Target {
        reference: some(reference),
        selector: some(selector),
    }
}

pub(crate) fn required(target: Target) -> Result<Target, String> {
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

/// Whether `error`, a page op's, may go away by waiting for the page: the page has no agent to run
/// the op yet while it loads, or WebKit couldn't hand the op to it just now. Nothing else does: no
/// app running, no browser open, a selector that isn't one (MCP-9).
fn waits_out(error: &str) -> bool {
    use sawit_core::browser::{NOT_READY, UNREACHABLE};
    error == NOT_READY || error.starts_with(UNREACHABLE)
}

impl Sawit {
    pub(crate) async fn browser(
        &self,
        project: &Option<String>,
        op: BrowserOp,
    ) -> Result<Value, String> {
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
    /// project folder ([`Self::dev_server_folder`]).
    async fn dev_server_url(&self, project: &Option<String>) -> Result<String, String> {
        let folder = self
            .dev_server_folder(project, crate::project_dir(None))
            .await?;
        let jobs = sawit_core::probe::listening_jobs()
            .await
            .map_err(|e| e.to_string())?;
        pick_dev_server(&jobs, &folder).ok_or_else(|| {
            "Give a url: no single dev server is running for this project (see list_ports).".into()
        })
    }

    /// The folder whose dev server `browser_navigate` opens when it is given no url (MCP-10):
    /// `project`, as it is on disk, which is how a dev server's folder is listed; else
    /// `agent_folder`, the folder the agent runs in, as the canvas and app tools take it; else,
    /// for an agent run from no project, the project of the front window, which shows the page.
    /// Never the whole Mac: another project's server would open, or none with two running.
    async fn dev_server_folder(
        &self,
        project: &Option<String>,
        agent_folder: Option<PathBuf>,
    ) -> Result<PathBuf, String> {
        if let Some(folder) = crate::app::project_path(project.clone()).or(agent_folder) {
            return Ok(folder);
        }
        let state = self.app_call(None, Command::State).await?;
        front_project(&state).ok_or_else(|| {
            "Give a url, or the project whose dev server to open: the front Sawit window has no \
             project."
                .into()
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
        description = "Press a key in the browser tab's focused element: Enter, Tab, Escape, Backspace, arrows, PageDown, Meta+a, one character. Two keys go to Sawit instead, as the user's do: Meta+Shift+. opens or closes the Annotate panel (with Annotate open and its picker off, it picks again; the answer has the panel and what the picker picks), and Escape while Sawit's picker is on (Annotate, or Design's inspect) turns it off"
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
            match self.browser(&a.project, op).await {
                Ok(v) if v["found"].as_bool() == Some(!gone) => {
                    return ok(format!(
                        "Done after {} ms: {}",
                        started.elapsed().as_millis(),
                        v["url"].as_str().unwrap_or_default()
                    ));
                }
                // A page mid-load has no agent yet: keep waiting.
                Ok(_) => {}
                Err(e) if waits_out(&e) => {}
                // No app, no browser, a selector that isn't one: waiting won't change them.
                Err(e) => return fail(e),
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
        let named = tri!(crate::output_path(
            "browser_screenshot",
            a.output_path.as_deref()
        ));
        let path = named.unwrap_or_else(|| screenshot_path(&self.paths.cache_dir()));
        let op = BrowserOp::Screenshot {
            target: target(a.reference, a.selector),
        };
        let v = tri!(self.browser(&a.project, op).await);
        let encoded = v["png_base64"].as_str().unwrap_or_default().to_string();
        let bytes = tri!(base64_decode(&encoded, "The browser"));
        if bytes.is_empty() {
            return fail("The browser sent an empty screenshot.");
        }
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
        description = "Read the browser tab's console messages and page errors (uncaught exceptions, promise rejections), newest last; filter by level and text. With clear: true it then empties the tab's console log, all of it, which also drops the console errors the user's Annotate notes would carry"
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
        description = "Read the browser tab's requests: fetch/XHR with method, status, duration and (cut) request and response bodies, plus scripts, styles and images; filter by URL text or failures. Secrets read [redacted]: what is typed into the page's password fields, and the values of fields named like secrets (password, token, api_key, authorization, cookie, session, …) in URLs and bodies. With clear: true it then empties the tab's network log, all of it"
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
        description = "Set the browser tab's viewport: a device (iPhone 15, Pixel 8, iPad mini, iPad Pro 11″, Laptop, Desktop), a width and height in CSS px, or responsive to fill the tab again"
    )]
    async fn browser_resize(&self, Parameters(a): Parameters<ResizeArgs>) -> CallToolResult {
        if let Err(e) = resize_mode(a.device.as_deref(), a.width, a.height) {
            return fail(e);
        }
        let v = tri!(
            self.browser(
                &a.project,
                BrowserOp::Resize {
                    device: a.device,
                    width: a.width,
                    height: a.height,
                },
            )
            .await
        );
        ok(pretty(&v))
    }

    #[tool(
        description = "Read the annotations the user made in the browser tab, as Markdown: what to change and its kind, and for each element its selector, DOM path, tag and key attributes (data-testid, aria-*, href…), accessible role and name, text, source file and component in a dev build, box in the viewport and on the page, main computed styles, neighbours, and a screenshot path; the page's console errors too. clear removes them after reading"
    )]
    async fn browser_annotations(
        &self,
        Parameters(a): Parameters<AnnotationsArgs>,
    ) -> CallToolResult {
        let v = tri!(
            self.browser(&a.project, BrowserOp::Annotations { clear: a.clear })
                .await
        );
        match v["markdown"].as_str() {
            Some(md) => ok(md.to_string()),
            // Annotate's keys as the user's keymap binds them, which may not be the default's.
            None => {
                let keys = self.keys_for("browser::ToggleAnnotate").await;
                let keys = match keys.as_slice() {
                    [] => String::new(),
                    keys => format!(" ({})", keys.join(" or ")),
                };
                ok(format!(
                    "No annotations yet. The user adds them with Annotate in the Browser tab's toolbar{keys}."
                ))
            }
        }
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
    fn resize_args_need_a_device_or_size() {
        let a = ResizeArgs {
            device: None,
            width: None,
            height: None,
            project: None,
        };
        let e = resize_mode(a.device.as_deref(), a.width, a.height).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(Err(e), sawit_core::browser::resize_mode(None, None, None));
    }

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

    /// A stand-in for the running app, listening where the app does: it answers every request
    /// with `answer`.
    fn fake_app(paths: &sawit_core::instances::AppPaths, answer: Value) {
        use std::io::{Read as _, Write as _};
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                // `is_running` only connects, and sends nothing.
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let line = format!("{}\n", sawit_core::app_control::reply_ok(&answer));
                let _ = stream.write_all(line.as_bytes());
            }
        });
    }

    /// MCP-7: `output_path` is read as every tool that writes a file reads one: trimmed, `~` for
    /// the home folder, and absolute. A space at its end became part of the file's name, and a
    /// relative path was written in whatever folder sawit-mcp ran in.
    #[tokio::test]
    async fn a_screenshot_path_is_read_as_every_tool_reads_one() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        };
        let server = Sawit::new(paths.clone());
        let shot = |output: String| {
            Parameters(ScreenshotArgs {
                reference: None,
                selector: None,
                output_path: Some(output),
                project: None,
            })
        };
        let said = |result: &CallToolResult| {
            let texts = result.content.iter().filter_map(|c| c.as_text());
            texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
        };

        // Refused before the app is asked: none runs yet, and nothing is written.
        let refused = server
            .browser_screenshot(shot("shots/page.png".into()))
            .await;
        assert_eq!(refused.is_error, Some(true));
        assert!(
            said(&refused).contains("must be absolute"),
            "{}",
            said(&refused)
        );

        fake_app(
            &paths,
            json!({"png_base64": "iVBORw0KGgo=", "width": 1, "height": 1}),
        );
        let png = dir.path().join("page.png");
        let taken = server
            .browser_screenshot(shot(format!(" {}  ", png.display())))
            .await;
        assert_ne!(taken.is_error, Some(true), "{}", said(&taken));
        assert_eq!(std::fs::read(&png).unwrap(), b"\x89PNG\r\n\x1a\n");
        assert!(said(&taken).ends_with(&format!("written to {}", png.display())));
    }

    /// MCP-10: with no url, browser_navigate opens the dev server of the project: the one named,
    /// else the folder the agent runs in (as the canvas and app tools take it), else the project
    /// of the front window, which the app shows the page in. It looked under `/`, so another
    /// project's server opened, or with two running anywhere on the Mac none did.
    #[tokio::test]
    async fn the_dev_server_opened_is_the_projects() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        };
        let server = Sawit::new(paths.clone());
        let shop = dir.path().join("shop");
        std::fs::create_dir_all(&shop).unwrap();
        let shop = shop.canonicalize().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&shop, &link).unwrap();
        let agent = dir.path().join("agent");

        let named = Some(link.display().to_string());
        let folder = server.dev_server_folder(&named, Some(agent.clone())).await;
        assert_eq!(folder, Ok(shop.clone()));
        let folder = server.dev_server_folder(&None, Some(agent.clone())).await;
        assert_eq!(folder, Ok(agent));

        // An agent run from no project: the front window's.
        let front = json!({"windows": [
            {"project": "/code/back", "frontmost": false},
            {"project": shop, "frontmost": true},
        ]});
        fake_app_answering(&paths, vec![Ok(front)]);
        let folder = server.dev_server_folder(&None, None).await;
        assert_eq!(folder, Ok(shop));
    }

    /// A stand-in for the running app that answers its requests with `answers` in turn, the last
    /// one again once they are used up.
    fn fake_app_answering(
        paths: &sawit_core::instances::AppPaths,
        answers: Vec<Result<Value, String>>,
    ) {
        use std::io::{Read as _, Write as _};
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter().peekable();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let answer = match answers.next() {
                    Some(answer) if answers.peek().is_none() => {
                        answers = vec![answer.clone()].into_iter().peekable();
                        answer
                    }
                    Some(answer) => answer,
                    None => break,
                };
                let line = match answer {
                    Ok(value) => sawit_core::app_control::reply_ok(&value),
                    Err(error) => sawit_core::app_control::reply_error(&error),
                };
                let _ = stream.write_all(format!("{line}\n").as_bytes());
            }
        });
    }

    /// MCP-9: browser_wait_for waits out only what waiting changes: a page that has no agent yet
    /// while it loads. Any other answer comes back at once: with no app running, or a selector
    /// that isn't one, it waited its whole timeout and then said it had timed out.
    #[tokio::test]
    async fn waiting_for_a_page_answers_what_waiting_wont_change_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        };
        let server = Sawit::new(paths.clone());
        let wait = |selector: &str| {
            Parameters(WaitArgs {
                text: None,
                text_gone: None,
                selector: Some(selector.into()),
                time_ms: None,
                timeout_ms: Some(3_000),
                project: None,
            })
        };
        let said = |result: &CallToolResult| {
            let texts = result.content.iter().filter_map(|c| c.as_text());
            texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
        };

        let started = Instant::now();
        let no_app = server.browser_wait_for(wait("main")).await;
        assert_eq!(no_app.is_error, Some(true));
        assert!(
            said(&no_app).contains("Sawit isn't running"),
            "{}",
            said(&no_app)
        );
        assert!(
            started.elapsed() < Duration::from_millis(1_500),
            "{:?}",
            started.elapsed()
        );

        let not_a_selector = "“[[[” isn't a valid CSS selector.".to_string();
        fake_app_answering(&paths, vec![Err(not_a_selector.clone())]);
        let started = Instant::now();
        let refused = server.browser_wait_for(wait("[[[")).await;
        assert_eq!(said(&refused), not_a_selector);
        assert!(
            started.elapsed() < Duration::from_millis(1_500),
            "{:?}",
            started.elapsed()
        );
    }

    /// A page that is loading has no agent to answer yet: browser_wait_for keeps asking.
    #[tokio::test]
    async fn waiting_for_a_page_waits_out_its_load() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        };
        let server = Sawit::new(paths.clone());
        let loading = Err(sawit_core::browser::NOT_READY.to_string());
        let found = Ok(json!({"found": true, "url": "http://localhost:5173/"}));
        fake_app_answering(&paths, vec![loading.clone(), loading, found]);
        let done = server
            .browser_wait_for(Parameters(WaitArgs {
                text: Some("Welcome".into()),
                text_gone: None,
                selector: None,
                time_ms: None,
                timeout_ms: Some(5_000),
                project: None,
            }))
            .await;
        assert_ne!(done.is_error, Some(true));
    }

    /// A screenshot never replaces a canvas file: the tool says so before it asks the app (none
    /// runs here), and the board is as it was. Another path gets the picture.
    #[tokio::test]
    async fn a_screenshot_never_replaces_a_canvas_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = sawit_core::instances::AppPaths {
            root: dir.path().join("home"),
        };
        let server = Sawit::new(paths.clone());
        let board = dir.path().join(".nyawit").join("canvas.sawit");
        std::fs::create_dir_all(board.parent().unwrap()).unwrap();
        std::fs::write(&board, b"the board").unwrap();
        let shot = |output: &std::path::Path| {
            Parameters(ScreenshotArgs {
                reference: None,
                selector: None,
                output_path: Some(output.display().to_string()),
                project: None,
            })
        };
        let said = |result: &CallToolResult| {
            let texts = result.content.iter().filter_map(|c| c.as_text());
            texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
        };

        let refused = server.browser_screenshot(shot(&board)).await;
        assert_eq!(refused.is_error, Some(true));
        assert_eq!(
            said(&refused),
            format!(
                "browser_screenshot would overwrite a canvas file: {}",
                board.display()
            )
        );
        assert_eq!(std::fs::read(&board).unwrap(), b"the board");

        // The first bytes of every PNG.
        fake_app(
            &paths,
            json!({"png_base64": "iVBORw0KGgo=", "width": 1, "height": 1}),
        );
        let png = dir.path().join("page.png");
        let taken = server.browser_screenshot(shot(&png)).await;
        assert_ne!(taken.is_error, Some(true), "{}", said(&taken));
        assert_eq!(std::fs::read(&png).unwrap(), b"\x89PNG\r\n\x1a\n");
    }

    /// What an agent is told of the browser is what it does. browser_press_key's ⇧⌘. and Esc go
    /// to Sawit, not the page, and ⇧⌘. is the only way an agent opens Annotate; browser_console
    /// and browser_network empty their whole log when asked to, so they say what that drops and
    /// aren't announced read-only; and app_state's `browser` is the active project's, which may
    /// not be the agent's own.
    #[test]
    fn the_browser_tools_say_what_they_change() {
        let tools = Sawit::router().list_all();
        let sent = |name: &str| {
            let tool = tools.iter().find(|tool| tool.name == name);
            let tool = tool.unwrap_or_else(|| panic!("no tool {name}"));
            serde_json::to_value(tool).unwrap()
        };
        let press = sent("browser_press_key").to_string();
        for said in [
            "Meta+Shift+. opens or closes the Annotate panel",
            "Escape while Sawit's picker is on",
            "Meta+Shift+. is Sawit's",
        ] {
            assert!(press.contains(said), "no {said:?} in {press}");
        }
        for (name, drops) in [
            (
                "browser_console",
                "the console errors the user's Annotate notes would carry",
            ),
            (
                "browser_network",
                "the failed-request count browser_navigate answers",
            ),
        ] {
            let tool = sent(name);
            let text = tool.to_string();
            assert!(text.contains("all of it") && text.contains(drops), "{text}");
            assert_eq!(tool["annotations"]["readOnlyHint"], false, "{name}");
            assert_eq!(tool["annotations"]["destructiveHint"], true, "{name}");
        }
        let state = sent("app_state").to_string();
        assert!(
            state.contains("`browser`: the browser tab of the window's active project")
                && state.contains("may not be the browser your browser_* tools use"),
            "{state}"
        );
    }
}
