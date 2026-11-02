//! `browser_*`: the running app's browser tab (see `sawit_core::browser`),
//! for an agent checking the app it is building: open pages, read them as an
//! outline with element refs, click and type, wait, screenshot, and read the
//! console and network logs.
//!
//! The app answers each op at once; waiting (for a load, a text, a promise)
//! is done here by polling.

use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content};
use rmcp::{tool, tool_router};
use sawit_core::browser::{BrowserOp, resize_mode};

use crate::app::{base64_decode, pretty};
use crate::{Sawit, fail, ok, tri};

mod args;
mod mesurer;
mod page;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_mesurer;

pub use args::{
    AnnotationsArgs, ClickArgs, ConsoleArgs, ElementArgs, EvaluateArgs, GuideArg, KeyArgs,
    LayoutGuideArg, MesurerArgs, NavigateArgs, NetworkArgs, PageArgs, ResizeArgs, ScreenshotArgs,
    ScrollArgs, SelectArgs, TypeArgs, WaitArgs,
};
use mesurer::{mesurer_op, screenshot_mesurer};
use page::{navigate_op, screenshot_path, timeout, waits_out};
pub(crate) use page::{required, target};

const POLL: Duration = Duration::from_millis(200);
const DEFAULT_LOAD_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
/// How long an action waits to see whether it started a navigation.
const SETTLE: Duration = Duration::from_millis(250);

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
        description = "Screenshot the browser tab's visible page (or one element by ref/selector) as PNG: returned as an image and written to a file. Annotate's page tools (mesurer) are left out unless mesurer is drawings or show"
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
            mesurer: tri!(screenshot_mesurer(a.mesurer.as_deref())),
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
        description = "Read the annotations the user made in the browser tab, as Markdown: what to change and its kind, and for each element its selector, DOM path, tag and key attributes (data-testid, aria-*, href…), accessible role and name, text, source file and component in a dev build, box in the viewport and on the page, main computed styles, neighbours, and a screenshot path; the page's console errors too. The comment threads the user pinned with Annotate's page tools (mesurer) come as notes of kind Comment, with their replies (resolved ones left out), and each page's other markup (text notes, arrows and pen strokes with the elements under them, pinned distances, measurements, guides, layout guides, X-ray, captures) as a Page markup section. clear removes them after reading, and deletes the comment threads it returned"
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
        description = "Annotate's page tools in the browser tab (mesurer: inspect, guides, rulers, layout guides, X-ray, arrows, pen, text notes, comments, Responsive previews): state says whether they are on the page and what they have there (comment threads, guides, layout guides, drawings, measurements; full adds comments' HTML); open or close turns Annotate on or off; set changes X-ray, rulers, the tool, minimized, or replaces the page's guides or layout guides (e.g. layout_guides [{kind: columns, count: 12, gutter: 20}]); clear removes the page's drawings, measurements, guides, layout_guides, comments or all. It never writes a comment: those are the user's words. Read them with browser_annotations"
    )]
    async fn browser_mesurer(&self, Parameters(a): Parameters<MesurerArgs>) -> CallToolResult {
        let op = tri!(mesurer_op(&a));
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
