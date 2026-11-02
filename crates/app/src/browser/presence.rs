//! Agents in the browser: the page's overlay draws each agent's pointer and pill
//! (`browser/page/overlay.js`, its `presence` op), and the browser keeps what each agent did last
//! (`app_state.agents`), says it to screen readers, and shows an agent's page opening in the
//! toolbar until the page has loaded.
//!
//! For an op that acts on the page (click, hover, type, select, scroll, press), the cursor's op
//! goes to the page first, and the action [`LEAD`] after it, so the pointer has glided to the
//! target as the action happens: the action is no more than [`LEAD`] late. Under reduced motion,
//! with the cursors off, or with the page off screen, the action goes at once.

use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::agents::{self as model, AgentIdentity, GONE_AFTER};
use sawit_core::browser::{BrowserOp, Target};
use serde_json::{Value, json};

use super::{BrowserView, OpResult};
use crate::agents::Look;

/// How far ahead of an action its cursor's op goes: the pointer's glide.
pub(super) const LEAD: Duration = Duration::from_millis(120);
/// How long after announcing what an agent did the browser waits before announcing it again.
const ANNOUNCE_GAP: Duration = Duration::from_secs(2);

/// The agents at work in one browser.
#[derive(Default)]
pub(super) struct Agents {
    /// Each agent's last op here, in words, and when.
    seen: Vec<(Look, String, Instant)>,
    /// An agent's page opening, and its name ("/checkout"), until the page has loaded.
    pub opening: Option<(Look, String)>,
    /// What the browser last announced, read by a polite status line.
    pub announcement: SharedString,
    announced: Vec<(SharedString, Instant)>,
    /// Whether agents' cursors showed when the browser last looked: turned off, the page's are
    /// cleared.
    pub shown: bool,
}

impl Agents {
    /// No agent yet, with agents' cursors `shown` or not.
    pub fn new(shown: bool) -> Self {
        Self {
            shown,
            ..Self::default()
        }
    }

    fn saw(&mut self, look: &Look, doing: &str, now: Instant) {
        self.seen.retain(|(l, ..)| l.id != look.id);
        self.seen.push((look.clone(), doing.to_string(), now));
    }

    fn announce(&mut self, look: &Look, done: &str, now: Instant) {
        self.announced
            .retain(|(_, at)| now.saturating_duration_since(*at) < ANNOUNCE_GAP);
        if self.announced.iter().any(|(id, _)| *id == look.id) {
            return;
        }
        self.announced.push((look.id.clone(), now));
        self.announcement = crate::agents::sentence(look, done);
    }
}

/// The page's action of `op`, as the overlay's `presence` op names it, and the element it acts
/// on; `None` for an op that doesn't act on the page.
fn page_action(op: &BrowserOp) -> Option<(&'static str, Option<&Target>)> {
    Some(match op {
        BrowserOp::Click { target, .. } => ("click", Some(target)),
        BrowserOp::Hover { target } => ("hover", Some(target)),
        BrowserOp::Type { target, .. } => ("type", Some(target)),
        BrowserOp::Select { target, .. } => ("select", Some(target)),
        BrowserOp::Scroll { target, .. } => ("scroll", Some(target)),
        BrowserOp::Press { .. } => ("press", None),
        _ => return None,
    })
}

/// The overlay's op that shows `look`'s cursor for `op`, signed with the page's `token`, the
/// action following `lead` later; `None` for an op that doesn't act on the page. What the agent
/// types never goes into it: the caret shows where the text goes, not the text.
pub(super) fn presence_op(
    op: &BrowserOp,
    look: &Look,
    token: &str,
    lead: Duration,
) -> Option<Value> {
    let (action, target) = page_action(op)?;
    let mut presence = json!({
        "op": "presence",
        "token": token,
        "agent": look.page_json(),
        "action": action,
        "lead": lead.as_millis() as u64,
    });
    if let Some(target) = target {
        if let Some(reference) = &target.reference {
            presence["ref"] = json!(reference);
        }
        if let Some(selector) = &target.selector {
            presence["selector"] = json!(selector);
        }
    }
    match op {
        BrowserOp::Select { values, .. } => presence["values"] = json!(values),
        BrowserOp::Scroll { dx, dy, .. } => {
            presence["dx"] = json!(dx);
            presence["dy"] = json!(dy);
        }
        BrowserOp::Press { key } => presence["key"] = json!(key),
        BrowserOp::Click { double, .. } => presence["double"] = json!(double),
        _ => {}
    }
    Some(presence)
}

/// The overlay's op that takes every agent's cursor off the page.
pub(super) fn clear_op(token: &str) -> Value {
    json!({ "op": "presence", "token": token, "action": "clear" })
}

impl BrowserView {
    /// Runs agent `identity`'s op, as [`Self::apply`] runs it, and shows the agent on the page:
    /// its cursor glides to the element the op acts on, [`LEAD`] ahead of the action. Opening a
    /// page shows who opens what in the toolbar until it has loaded. The answer is the op's own.
    pub fn apply_as(
        &mut self,
        op: BrowserOp,
        identity: Option<&AgentIdentity>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<OpResult> {
        let look = crate::agents::meet(identity, cx);
        let now = cx.background_executor().now();
        let words = model::browser_action(&op);
        let says = model::browser_op_changes_the_page(&op);
        self.agents.saw(&look, &words.doing, now);
        if let BrowserOp::Navigate { url } = &op {
            self.agents.opening = Some((look.clone(), model::page_name(url)));
            let opening = format!("is opening {}", model::page_name(url));
            self.agents.announce(&look, &opening, now);
        }
        let shows = crate::agents::shown(cx) && self.engine.is_some() && self.page_shown();
        let lead = match crate::agents::reduce_motion(cx) {
            true => Duration::ZERO,
            false => LEAD,
        };
        let cursor = shows
            .then(|| presence_op(&op, &look, &self.token, lead))
            .flatten();
        let Some(cursor) = cursor else {
            let answer = self.apply(op, cx);
            let done = says.then_some(words.done);
            return self.answer_as(look, done, answer, cx);
        };
        // The overlay names the element ("clicking Sign up"); its words win once it answers.
        let named = self.page_op(cursor);
        let (reply, answer) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let started = cx.background_executor().now();
            let page = named.await.ok().and_then(Result::ok);
            let doing = (page.as_ref()).and_then(|w| w["doing"].as_str().map(str::to_string));
            let done = (page.as_ref()).and_then(|w| w["done"].as_str().map(str::to_string));
            let waited = cx
                .background_executor()
                .now()
                .saturating_duration_since(started);
            if let Some(rest) = lead.checked_sub(waited).filter(|rest| !rest.is_zero()) {
                cx.background_executor().timer(rest).await;
            }
            let Ok(result) = this.update(cx, |this, cx| {
                if let Some(doing) = &doing {
                    let now = cx.background_executor().now();
                    this.agents.saw(&look, doing, now);
                }
                this.apply(op, cx)
            }) else {
                return;
            };
            let result = result
                .await
                .unwrap_or_else(|_| Err("The browser closed before answering.".into()));
            if result.is_ok() {
                let _ = this.update(cx, |this, cx| {
                    let now = cx.background_executor().now();
                    this.agents
                        .announce(&look, done.as_deref().unwrap_or(&words.done), now);
                    cx.notify();
                });
            }
            let _ = reply.send(result);
        })
        .detach();
        answer
    }

    /// `answer`, once it comes, announced as `look` having done `done` when it went through and
    /// there is something to say.
    fn answer_as(
        &mut self,
        look: Look,
        done: Option<String>,
        answer: oneshot::Receiver<OpResult>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<OpResult> {
        let (reply, forwarded) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let result = answer
                .await
                .unwrap_or_else(|_| Err("The browser closed before answering.".into()));
            if let (Ok(_), Some(done)) = (&result, done) {
                let _ = this.update(cx, |this, cx| {
                    let now = cx.background_executor().now();
                    this.agents.announce(&look, &done, now);
                    cx.notify();
                });
            }
            let _ = reply.send(result);
        })
        .detach();
        forwarded
    }

    /// The agents' global changed: when that is their cursors turned on or off, the toolbar's
    /// toggle redraws, and turned off, the page's cursors are cleared. Meeting an agent changes
    /// the global too, which changes nothing here.
    pub(super) fn agents_setting_changed(&mut self, cx: &mut Context<Self>) {
        let shown = crate::agents::shown(cx);
        if std::mem::replace(&mut self.agents.shown, shown) == shown {
            return;
        }
        if !shown {
            drop(self.page_op(clear_op(&self.token)));
        }
        cx.notify();
    }

    /// The page an agent opened has loaded: the toolbar no longer says it is opening.
    pub(super) fn agent_page_loaded(&mut self) {
        self.agents.opening = None;
    }

    /// The agents at work in this browser, as `app_state.agents` lists them.
    pub fn describe_agents(&self, cx: &App) -> Vec<Value> {
        let now = cx.background_executor().now();
        (self.agents.seen.iter())
            .filter(|(.., at)| now.saturating_duration_since(*at) < GONE_AFTER)
            .map(|(look, doing, at)| {
                json!({
                    "id": look.id,
                    "name": look.label,
                    "color": look.hex,
                    "surface": "browser",
                    "project": self.project,
                    "action": doing,
                    "seconds_ago": now.saturating_duration_since(*at).as_secs(),
                })
            })
            .collect()
    }

    /// The polite status line that says what agents did in the browser.
    pub(super) fn render_agent_status(&self) -> impl IntoElement + use<> {
        div()
            .id("browser-agent-activity")
            .role(Role::Status)
            .aria_label(self.agents.announcement.clone())
            .absolute()
            .top_0()
            .left_0()
            .size(px(1.0))
            .overflow_hidden()
    }

    /// While an agent's page opens: "Claude Code is opening /checkout…" in the agent's colour,
    /// over the address field's end, from the moment it asked until the page has loaded.
    pub(super) fn render_agent_opening(&self) -> Option<impl IntoElement + use<>> {
        let (look, page) = self.agents.opening.as_ref().filter(|_| self.loading)?;
        let text = format!("{} is opening {page}…", look.label);
        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(4.0))
                .flex()
                .items_center()
                .child(
                    div()
                        .id("browser-agent-opening")
                        .debug_selector(|| "browser-agent-opening".into())
                        .max_w(px(280.0))
                        .h(px(20.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_full()
                        .border_1()
                        .border_color(white())
                        .bg(look.color)
                        .text_color(white())
                        .text_xs()
                        .truncate()
                        .child(text),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sawit_core::browser::{BrowserOp, Target};
    use serde_json::json;

    use super::{Agents, LEAD, clear_op, presence_op};
    use crate::agents::Look;

    fn look() -> Look {
        Look::for_tests("a")
    }

    /// The cursor's op names the agent, the action, the element and what the action needs to say
    /// what it does, signed with the page's token; never what is typed.
    #[test]
    fn the_cursors_op_says_who_does_what_where() {
        let click = BrowserOp::Click {
            target: Target {
                reference: Some("e12".into()),
                selector: None,
            },
            double: false,
            button: None,
        };
        assert_eq!(
            presence_op(&click, &look(), "T0K", LEAD),
            Some(json!({
                "op": "presence", "token": "T0K", "action": "click", "lead": 120,
                "agent": {"id": "a", "name": "Agent a", "color": sawit_core::agents::PALETTE[0]},
                "ref": "e12", "double": false,
            }))
        );
        let typed = BrowserOp::Type {
            target: Target {
                reference: None,
                selector: Some("#email".into()),
            },
            text: "hunter2".into(),
            clear: false,
            submit: false,
        };
        let op = presence_op(&typed, &look(), "T0K", Duration::ZERO).unwrap();
        assert_eq!(op["selector"], "#email");
        assert_eq!(op["lead"], 0, "no lead under reduced motion");
        assert!(!op.to_string().contains("hunter2"), "{op}");
        let press = presence_op(
            &BrowserOp::Press {
                key: "Enter".into(),
            },
            &look(),
            "T",
            LEAD,
        );
        assert_eq!(press.unwrap()["key"], "Enter");
        assert_eq!(presence_op(&BrowserOp::Snapshot, &look(), "T", LEAD), None);
        let navigate = BrowserOp::Navigate {
            url: "http://localhost:5173/".into(),
        };
        assert_eq!(presence_op(&navigate, &look(), "T", LEAD), None);
        assert_eq!(clear_op("T")["action"], "clear");
    }

    /// The same agent is announced once in a while, two agents each in turn.
    #[test]
    fn announcements_are_polite() {
        let mut agents = Agents::default();
        let t0 = Instant::now();
        let other = Look::for_tests("b");
        agents.announce(&look(), "clicked Sign up", t0);
        agents.announce(&look(), "typed in Email", t0 + Duration::from_millis(300));
        assert_eq!(agents.announcement.as_ref(), "Agent a clicked Sign up");
        agents.announce(&other, "scrolled", t0 + Duration::from_millis(400));
        assert_eq!(agents.announcement.as_ref(), "Agent b scrolled");
    }
}
