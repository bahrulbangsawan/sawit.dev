//! Lane 3 (mesurer-ui): mesurer's keys, links and theme.
//!
//! Keys: GPUI sees the ⌘ keys before the page. ⌘K is the workspace's chord prefix (`cmd-k w`, …),
//! which would hold it, and ⌘A the Edit menu's Select All, which would take it; so while mesurer is
//! mounted and the page has the keyboard, the browser's key context gains `mesurer` (and
//! `mesurer_select` in mesurer's annotation Select tool), whose bindings take ⌘K, ⇧⌘K and ⌘A and
//! hand them to mesurer (`SawitMesurer.call("key", …)`, `page/mesurer/keys.js`): copy the
//! comments for an agent, resolve them all, select every annotation. Elsewhere (the address
//! field, the side panel, mesurer unmounted) ⌘K's chords are the workspace's and ⌘A is Select All.
//!
//! What the glue posts here: `link.open` (mesurer's links: an https one opens in the default
//! browser) and `ui.state` (its tool and chrome, over the summary of its last save).

use gpui_kit::component::Theme;
use gpui_kit::*;
use sawit_core::browser::mesurer::{FromMesurer, calls, origin_key};
use serde_json::{Value, json};

use super::super::annotate::AnnotatePanel;
use super::super::engine::WorldReply;
use super::super::{
    BrowserView, CopyCommentsForAgent, ResolveAllComments, SelectAllAnnotations, ShowMesurerTools,
};
use super::{e2e_run, theme};

/// Lane 3's state: what was handed to mesurer, and Sawit's theme as mesurer was told it.
#[derive(Default)]
pub(in crate::browser) struct KeysState {
    /// The last key handed to mesurer (`{key, code, meta, shift}`), for tests.
    pub last_key: Option<Value>,
    /// The last link mesurer opened, kept instead of opening it under `SAWIT_E2E_RUN`.
    pub last_link: Option<String>,
    /// Sawit's theme as mesurer was last told it.
    theme: Option<&'static str>,
}

/// Answers a chrome kind ([`sawit_core::browser::mesurer::Lane::Keys`]).
pub(super) fn handle(
    view: &mut BrowserView,
    message: FromMesurer,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
) {
    match message {
        FromMesurer::LinkOpen { url } => match https_link(&url) {
            Some(url) => {
                if e2e_run() {
                    eprintln!("sawit: mesurer's link {url} (kept, not opened: SAWIT_E2E_RUN)");
                } else {
                    cx.open_url(&url);
                }
                view.mesurer.keys.last_link = Some(url.clone());
                reply.ok_value(&json!({ "opened": url }));
            }
            None => reply.err("Only https links open from mesurer, in the default browser."),
        },
        FromMesurer::UiState(state) => {
            view.mesurer_ui_state(&state, cx);
            reply.ok_value(&Value::Null);
        }
        other => reply.err(format!("“{}” isn't lane 3's.", other.kind())),
    }
}

/// `url` as it opens in the default browser: an absolute https address only.
fn https_link(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))?;
    let host = &url[rest.len()..];
    let valid = !host.is_empty()
        && !host.starts_with('/')
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    valid.then(|| format!("https://{host}"))
}

/// The key handed to mesurer for `action`, one of its shortcuts bound in the browser
/// ([`crate::browser::init`]): ⌘K copies the comments for an agent, ⇧⌘K resolves them all, ⌘A
/// in mesurer's Select tool selects every annotation.
fn key_for(action: &dyn Action) -> Option<Value> {
    let any = action.as_any();
    if any.is::<CopyCommentsForAgent>() {
        Some(json!({ "key": "k", "code": "KeyK", "meta": true }))
    } else if any.is::<ResolveAllComments>() {
        Some(json!({ "key": "k", "code": "KeyK", "meta": true, "shift": true }))
    } else if any.is::<SelectAllAnnotations>() {
        Some(json!({ "key": "a", "code": "KeyA", "meta": true }))
    } else {
        None
    }
}

impl BrowserView {
    /// mesurer's shortcuts, bound in the browser's key context while mesurer is on the page and
    /// the page has the keyboard ([`Self::mesurer_key_context`]).
    pub(in crate::browser) fn mesurer_bindings() -> [KeyBinding; 3] {
        [
            KeyBinding::new("cmd-k", CopyCommentsForAgent, Some("Browser && mesurer")),
            KeyBinding::new(
                "cmd-shift-k",
                ResolveAllComments,
                Some("Browser && mesurer"),
            ),
            KeyBinding::new(
                "cmd-a",
                SelectAllAnnotations,
                Some("Browser && mesurer_select"),
            ),
        ]
    }

    /// What the browser follows for mesurer: Sawit's theme (mesurer's default theme), and the
    /// Annotate panel's Show tools.
    pub(in crate::browser) fn mesurer_subscriptions(
        annotate: &Entity<AnnotatePanel>,
        cx: &mut Context<Self>,
    ) -> [Subscription; 2] {
        [
            cx.observe_global::<Theme>(|this, cx| this.mesurer_theme_changed(cx)),
            cx.subscribe(annotate, |this, _, _: &ShowMesurerTools, cx| {
                this.mesurer_show_tools(cx)
            }),
        ]
    }

    /// The browser's key context: `Browser`, with `mesurer` while mesurer is mounted and the page
    /// has the keyboard (the view itself, not the address field or the side panel), and
    /// `mesurer_select` while mesurer's annotation Select tool is on.
    pub(in crate::browser) fn mesurer_key_context(&self, window: &Window) -> KeyContext {
        let mut context = KeyContext::default();
        context.add("Browser");
        if self.mesurer.mounted && self.focus.is_focused(window) {
            context.add("mesurer");
            if self.mesurer.summary.tool == "selection" {
                context.add("mesurer_select");
            }
        }
        context
    }

    /// The browser's handlers of mesurer's shortcuts, on `el` (the view's root).
    pub(in crate::browser) fn with_mesurer_keys(el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(
            cx.listener(|this, a: &CopyCommentsForAgent, _, cx| this.mesurer_action(a, cx)),
        )
        .on_action(cx.listener(|this, a: &ResolveAllComments, _, cx| this.mesurer_action(a, cx)))
        .on_action(cx.listener(|this, a: &SelectAllAnnotations, _, cx| this.mesurer_action(a, cx)))
    }

    /// Hands the key of `action`, one of mesurer's shortcuts, to mesurer on the page.
    pub(in crate::browser) fn mesurer_action(
        &mut self,
        action: &dyn Action,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = key_for(action).filter(|_| self.mesurer.mounted) else {
            return;
        };
        self.mesurer.keys.last_key = Some(key.clone());
        let call = json!({ "name": "key", "args": key }).to_string();
        let answered = self.world_json(calls::CALL, call);
        cx.spawn(async move |_, _| {
            if let Ok(Err(e)) = answered.await {
                eprintln!("sawit: a key couldn't be handed to mesurer: {e}");
            }
        })
        .detach();
    }

    /// Sawit's theme changed: mesurer's default theme follows it (mesurer's own Appearance
    /// setting, once the user picks one, wins over it).
    pub(in crate::browser) fn mesurer_theme_changed(&mut self, cx: &mut Context<Self>) {
        let now = theme(cx);
        if self.mesurer.keys.theme.replace(now) == Some(now) || !self.mesurer.mounted {
            return;
        }
        let props = json!({ "theme": now }).to_string();
        drop(self.world_json(calls::UPDATE, props));
    }

    /// The panel's Show tools: mesurer, minimized, shows its toolbar again. Its workspace is
    /// applied again with `minimized: false` (and kept so), as a change made elsewhere would be.
    pub(in crate::browser) fn mesurer_show_tools(&mut self, cx: &mut Context<Self>) {
        if !self.mesurer.mounted {
            return;
        }
        let origin = origin_key(&self.current_url());
        // What is held, whether or not the files have it yet (a mount reads them first).
        if !self.mesurer.store.holds(&origin) {
            return;
        }
        let mut workspace = (self.mesurer.store.workspace(&origin).cloned())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        workspace["minimized"] = json!(false);
        workspace["enabled"] = json!(true);
        let snapshot = json!({ "settings": self.mesurer.store.settings(), "workspace": workspace });
        let project = self.project.clone();
        (self.mesurer.store).save_workspace(project.as_deref(), &origin, Some(workspace), cx);
        self.mesurer_summary_changed(cx);
        let args = json!({ "snapshot": snapshot, "source": { "workspace": true } }).to_string();
        drop(self.world_json(calls::APPLY, args));
    }

    /// mesurer said what its tool and chrome are now (`ui.state`: `tool`, `minimized`, `xray`,
    /// `rulers`): the summary, and the panel's status line, follow it before its next save.
    fn mesurer_ui_state(&mut self, state: &Value, cx: &mut Context<Self>) {
        let summary = &mut self.mesurer.summary;
        if let Some(tool) = state["tool"].as_str() {
            summary.tool = tool.to_string();
        }
        for (name, field) in [
            ("minimized", &mut summary.minimized),
            ("xray", &mut summary.xray),
            ("rulers", &mut summary.rulers),
        ] {
            if let Some(on) = state[name].as_bool() {
                *field = on;
            }
        }
        let summary = summary.clone();
        if self.mesurer.mounted {
            self.annotate
                .update(cx, |a, cx| a.set_mesurer_summary(Some(summary), cx));
        }
        cx.notify();
    }
}
