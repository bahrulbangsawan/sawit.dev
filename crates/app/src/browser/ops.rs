//! Agents' ops ([`BrowserView::apply`], `sawit-mcp`'s `browser_*` tools): navigation, state,
//! logs, screenshots and the panels are answered here, the rest by the page agent through
//! `Engine::eval_with_callback`.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::{BrowserOp, Target, resize_mode};
use serde_json::{Value, json};

use super::engine::Engine;
use super::{BrowserView, FromPage, OpResult};

impl BrowserView {
    /// Runs an agent's op; the receiver gets the answer.
    pub fn apply(&mut self, op: BrowserOp, cx: &mut Context<Self>) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        if let Some(e) = &self.error {
            let _ = tx.send(Err(format!("The browser couldn't start: {e}")));
            return rx;
        }
        // Sawit's keys, not the page's (see `own_key`): no page needed.
        if let BrowserOp::Press { key } = &op
            && let Some(answer) = self.press_own_key(key, cx)
        {
            let _ = tx.send(Ok(answer));
            return rx;
        }
        // Reloaded by the user (the toolbar) or an agent: the page's links to other apps may ask
        // again, as after opening a page. A page's own reload doesn't come this way.
        if op == BrowserOp::Reload {
            self.external.reset();
        }
        if self.engine.is_none() {
            self.queued.push((op, tx));
            return rx;
        }
        self.run(op, tx, cx);
        rx
    }

    pub(super) fn run(
        &mut self,
        op: BrowserOp,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.engine.clone() else {
            let _ = reply.send(Err("The browser isn't open.".into()));
            return;
        };
        let value = match op {
            BrowserOp::Navigate { url } => {
                self.navigate(&url, cx);
                json!({ "navigating": self.url })
            }
            BrowserOp::Back | BrowserOp::Forward => {
                let back = matches!(op, BrowserOp::Back);
                engine.eval(if back {
                    "history.back()"
                } else {
                    "history.forward()"
                });
                json!({ "requested": if back { "back" } else { "forward" } })
            }
            BrowserOp::Reload => {
                self.loading = true;
                engine.reload();
                json!({ "reloading": self.current_url() })
            }
            BrowserOp::State => {
                let mut state = self.describe();
                state["console_errors"] = self
                    .logs
                    .console
                    .iter()
                    .filter(|e| e.level == "error")
                    .count()
                    .into();
                state["failed_requests"] = self
                    .logs
                    .network
                    .iter()
                    .filter(|e| e.failed())
                    .count()
                    .into();
                state
            }
            BrowserOp::Console {
                level,
                pattern,
                limit,
                clear,
            } => {
                let value = self
                    .logs
                    .console_json(level.as_deref(), pattern.as_deref(), limit);
                if clear {
                    self.logs.console.clear();
                }
                value
            }
            BrowserOp::Network {
                pattern,
                failed_only,
                limit,
                clear,
            } => {
                let value = self
                    .logs
                    .network_json(pattern.as_deref(), failed_only, limit);
                if clear {
                    self.logs.network.clear();
                }
                value
            }
            BrowserOp::Devtools => {
                engine.open_devtools();
                json!({ "opened": "Web Inspector" })
            }
            BrowserOp::Resize {
                device,
                width,
                height,
            } => match resize_mode(device.as_deref(), width, height) {
                Ok(mode) => {
                    self.set_viewport(mode, cx);
                    self.viewport_json()
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                    return;
                }
            },
            BrowserOp::Evaluate { script } => {
                evaluate(Rc::downgrade(&engine), script, reply);
                return;
            }
            BrowserOp::Annotations { clear } => {
                let markdown = self.annotations_markdown(cx);
                let count = self.annotate.read(cx).annotations(cx).len();
                self.annotate.update(cx, |a, cx| {
                    if clear {
                        a.clear(cx);
                    } else {
                        a.mark_sent(cx);
                    }
                });
                json!({ "markdown": markdown, "count": count })
            }
            BrowserOp::Inspect { target, format } => {
                let target = serde_json::to_value(&target).unwrap_or_default();
                return self.inspect_op(target, format, reply, cx);
            }
            BrowserOp::Design { part } => return self.design_op(part, reply, cx),
            BrowserOp::ExportDesign {
                include_videos,
                replace,
            } => {
                let _ = reply.send(self.export_op(include_videos, replace, cx));
                return;
            }
            BrowserOp::ExportStatus => {
                let _ = reply.send(self.export_status_op());
                return;
            }
            BrowserOp::Panel { panel, tab } => {
                let _ = reply.send(self.panel_op(panel.as_deref(), tab.as_deref(), cx));
                return;
            }
            BrowserOp::Screenshot { target } if target.is_empty() => {
                return self.shoot(None, reply, cx);
            }
            BrowserOp::Screenshot { target } => {
                // The element's box first (scrolled into view), then the capture.
                let to_view = self.to_view.clone();
                let reply = Mutex::new(Some(reply));
                let script = agent_call(&rect_op(&target));
                let _ = engine.eval_with_callback(&script, move |result| {
                    let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
                        return;
                    };
                    let rect = agent_result(&result).and_then(|r| {
                        let field = |k: &str| r[k].as_f64().unwrap_or_default();
                        let rect = (field("x"), field("y"), field("width"), field("height"));
                        if rect.2 < 1. || rect.3 < 1. {
                            return Err("The element isn't visible on screen.".to_string());
                        }
                        Ok(rect)
                    });
                    match rect {
                        Ok(rect) => {
                            let _ = to_view.unbounded_send(FromPage::Shoot {
                                rect: Some(rect),
                                reply,
                            });
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                        }
                    }
                });
                return;
            }
            op => {
                let reply = Mutex::new(Some(reply));
                let script = agent_call(&serde_json::to_value(&op).unwrap_or_default());
                let sent = engine.eval_with_callback(&script, move |result| {
                    if let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) {
                        let _ = reply.send(agent_result(&result));
                    }
                });
                if let Err(e) = sent {
                    eprintln!("sawit: browser script failed: {e}");
                }
                return;
            }
        };
        let _ = reply.send(Ok(value));
    }

    /// Runs `op` on the page and answers with what the page says.
    pub(crate) fn page_op(&self, op: Value) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        let Some(engine) = &self.engine else {
            let _ = tx.send(Err("The browser isn't open.".into()));
            return rx;
        };
        let tx = Arc::new(Mutex::new(Some(tx)));
        let callback_tx = tx.clone();
        let sent = engine.eval_with_callback(&agent_call(&op), move |result| {
            if let Some(tx) = callback_tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(agent_result(&result));
            }
        });
        if let Err(e) = sent
            && let Some(tx) = tx.lock().ok().and_then(|mut t| t.take())
        {
            let unreachable = sawit_core::browser::UNREACHABLE;
            let _ = tx.send(Err(format!("{unreachable}: {e}")));
        }
        rx
    }
}

/// Calls the page agent with `op`; an empty string means the page has no
/// agent (it is still loading, or isn't a web page).
pub(super) fn agent_call(op: &Value) -> String {
    format!("(() => {{ const a = window.__sawitAgent; return a ? a.run({op}) : \"\"; }})()")
}

pub(super) fn rect_op(target: &Target) -> Value {
    let mut op = serde_json::to_value(target).unwrap_or_else(|_| json!({}));
    op["op"] = "rect".into();
    op
}

/// Runs an agent's `browser_evaluate` script: as a function body when only that form
/// compiles (`evaluate_as_body` knows before WebKit compiles anything, whose `SyntaxError`
/// would reach the page as an error of its own), else as an expression, and as a body when
/// WebKit can't compile that either. WebKit compiles it (`evaluate_call`), so it runs on pages
/// whose Content-Security-Policy forbids `eval`.
fn evaluate(engine: std::rc::Weak<Engine>, script: String, reply: oneshot::Sender<OpResult>) {
    let Some(first) = engine.upgrade() else {
        let _ = reply.send(Err("The browser isn't open.".into()));
        return;
    };
    let answer = |result: Result<Option<String>, String>| {
        result.and_then(|text| agent_answer(text.as_deref().unwrap_or_default()))
    };
    let body = sawit_core::browser::evaluate_as_body(&script);
    first.eval_text(
        &sawit_core::browser::evaluate_call(&script, body),
        move |result| match result {
            Err(e) if !body && e.contains("SyntaxError") => match engine.upgrade() {
                Some(engine) => engine.eval_text(
                    &sawit_core::browser::evaluate_call(&script, true),
                    move |result| {
                        let _ = reply.send(answer(result));
                    },
                ),
                None => {
                    let _ = reply.send(Err("The browser was closed.".into()));
                }
            },
            result => {
                let _ = reply.send(answer(result));
            }
        },
    );
}

/// The agent's answer as WebKit hands it over: its return value (a JSON
/// string) serialized as JSON once more.
pub(super) fn agent_result(result: &str) -> OpResult {
    let text: String = serde_json::from_str(result).unwrap_or_default();
    agent_answer(&text)
}

/// The agent's answer: `{"ok": true, …}`, or `{"ok": false, "error"}`; nothing when the page
/// has no agent.
fn agent_answer(text: &str) -> OpResult {
    if text.is_empty() {
        return Err(sawit_core::browser::NOT_READY.into());
    }
    let mut value: Value =
        serde_json::from_str(text).map_err(|e| format!("The page sent a bad answer: {e}"))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The page couldn't do that.")
            .to_string());
    }
    if let Some(map) = value.as_object_mut() {
        map.remove("ok");
    }
    Ok(value)
}
