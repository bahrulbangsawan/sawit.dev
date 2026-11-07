//! Agents' clicks on a tab on screen, as real mouse events ([`Engine::native_click`]):
//! the page agent checks the element and says where it is (`aim`), the app sends the events, and
//! answers once the page has had them (`aimed`). A tab that isn't on screen, a right click (which
//! would open WebKit's own menu), a hover, or events that never reach the page take the page
//! agent's synthetic events, as before.
//!
//! The page's report of a trusted press (`Pressed`, which gives the user's press the keyboard) is
//! ignored while an agent's events are on their way: they never move the keyboard.

use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::{BrowserOp, Target};
use serde_json::{Value, json};

use super::{BrowserView, OpResult};

/// How long the app waits for its events to reach the page.
const REACH: Duration = Duration::from_millis(1500);
/// How often it asks.
const ASK_EVERY: Duration = Duration::from_millis(25);
/// How long after its events a trusted press in the page is still the agent's.
const PRESS_IS_AGENTS: Duration = Duration::from_millis(1500);

impl BrowserView {
    /// Whether a trusted press the page reports now came from an agent's events.
    pub(super) fn agent_pointing(&self) -> bool {
        self.agent_pointer_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// Runs a left click (single or double) as real mouse events when the tab is on screen;
    /// hands `reply` back otherwise, for the page agent's events.
    pub(super) fn try_native_pointer(
        &mut self,
        op: &BrowserOp,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) -> Result<(), oneshot::Sender<OpResult>> {
        // A right click would open WebKit's own menu; a hover's mouse-moved event never
        // reaches the page when handed to the view: both stay the page agent's.
        let (target, double) = match op {
            BrowserOp::Click {
                target,
                double,
                button,
            } if button.as_deref() != Some("right") => (target, *double),
            _ => return Err(reply),
        };
        let Some(engine) = self.engine.clone() else {
            return Err(reply);
        };
        if !self.page_shown() || !engine.takes_native_pointer() {
            return Err(reply);
        }
        let aim = self.page_op(aim_op(target));
        let fallback = op.clone();
        cx.spawn(async move |this, cx| {
            let aim = match aim.await {
                Ok(Ok(aim)) => aim,
                Ok(Err(e)) => {
                    let _ = reply.send(Err(e));
                    return;
                }
                Err(_) => {
                    let _ = reply.send(Err("The page didn't answer.".into()));
                    return;
                }
            };
            let field = |k: &str| aim[k].as_f64().unwrap_or_default();
            let at = (field("x"), field("y"));
            let viewport = (
                aim["viewport"][0].as_f64().unwrap_or_default(),
                aim["viewport"][1].as_f64().unwrap_or_default(),
            );
            let sent = this.update(cx, |this, _| {
                this.agent_pointer_until = Some(Instant::now() + REACH + PRESS_IS_AGENTS);
                engine.native_click(double, at, viewport)
            });
            let reached = match sent {
                Ok(Ok(())) => wait_for_page(&this, double, cx).await,
                _ => Reached::No,
            };
            if reached == Reached::No {
                // Nothing reached the page: its agent's events instead.
                let answer = this.update(cx, |this, _| {
                    this.page_op(serde_json::to_value(&fallback).unwrap_or_default())
                });
                let answer = match answer {
                    Ok(answer) => answer.await.unwrap_or_else(|_| Err("No answer.".into())),
                    Err(_) => Err("The browser tab was closed.".into()),
                };
                let _ = reply.send(answer);
                return;
            }
            let name = aim["target"].clone();
            let _ = reply.send(Ok(json!({ "clicked": name, "url": aim["url"] })));
        })
        .detach();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reached {
    /// All of it.
    Yes,
    /// Some of it, within the wait: the page has the rest coming.
    Partly,
    /// None of it.
    No,
}

/// Asks the page (`aimed`) until it has had the events, within [`REACH`].
async fn wait_for_page(this: &WeakEntity<BrowserView>, double: bool, cx: &mut AsyncApp) -> Reached {
    let start = Instant::now();
    let mut last = Reached::No;
    while start.elapsed() < REACH {
        let asked = this.update(cx, |this, _| this.page_op(json!({ "op": "aimed" })));
        let Ok(asked) = asked else {
            return last;
        };
        if let Ok(Ok(seen)) = asked.await {
            last = reached(double, &seen);
            if last == Reached::Yes {
                return last;
            }
        }
        cx.background_executor().timer(ASK_EVERY).await;
    }
    last
}

/// What of a click's events the page has had (`aimed`: `{mousemove, mousedown, click}`).
fn reached(double: bool, seen: &Value) -> Reached {
    let n = |k: &str| seen[k].as_u64().unwrap_or(0);
    let done = n("click") >= if double { 2 } else { 1 };
    if done {
        Reached::Yes
    } else if n("mousemove") + n("mousedown") + n("click") > 0 {
        Reached::Partly
    } else {
        Reached::No
    }
}

fn aim_op(target: &Target) -> Value {
    let mut op = serde_json::to_value(target).unwrap_or_else(|_| json!({}));
    op["op"] = "aim".into();
    op
}

#[cfg(test)]
mod tests {
    use sawit_core::browser::Target;
    use serde_json::json;

    use super::{Reached, aim_op, reached};

    #[test]
    fn reached_counts_what_the_page_had() {
        let (click, double) = (false, true);
        let seen = |m: u64, d: u64, c: u64| json!({"mousemove": m, "mousedown": d, "click": c});
        assert_eq!(reached(click, &seen(1, 1, 1)), Reached::Yes);
        assert_eq!(reached(double, &seen(1, 2, 1)), Reached::Partly);
        assert_eq!(reached(double, &seen(1, 2, 2)), Reached::Yes);
        assert_eq!(reached(click, &seen(0, 0, 0)), Reached::No);
    }

    #[test]
    fn aim_carries_the_target() {
        let target = Target {
            reference: Some("e3".into()),
            selector: None,
        };
        assert_eq!(aim_op(&target), json!({"op": "aim", "ref": "e3"}));
    }
}
