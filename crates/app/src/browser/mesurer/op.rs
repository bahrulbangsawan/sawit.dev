//! Agents' `mesurer` op (`BrowserOp::Mesurer`, `sawit-mcp`'s `browser_mesurer`): `state` here,
//! the other actions in lane 2's `agents.rs`; and what `app_state` shows of mesurer.

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::mesurer::{
    self as core, MESURER_COMMIT, MESURER_VERSION, chrome_of, origin_key, page_artifacts, page_key,
};
use serde_json::{Map, Value, json};

use super::super::{BrowserView, OpResult};
use super::{agents, e2e_run, store};

impl BrowserView {
    /// Runs an agent's `mesurer` op; it needs no page while mesurer isn't mounted.
    pub(in crate::browser) fn mesurer_apply(
        &mut self,
        action: &str,
        args: &Map<String, Value>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<OpResult> {
        if action == "state" {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Ok(self.mesurer_state(args)));
            return rx;
        }
        agents::op(self, action, args, cx)
    }

    /// Whether mesurer is mounted, which, on what page key and origin, what it has there (the
    /// summary, the session chrome, the page's markup), and where that is kept. Comments' HTML
    /// previews are left out unless `html: true`.
    fn mesurer_state(&self, args: &Map<String, Value>) -> Value {
        let url = self.current_url();
        let origin = origin_key(&url);
        let key = page_key(&url);
        let envelope = self.mesurer.store.workspace(&origin);
        let mut page = envelope.map_or(Value::Null, |e| page_artifacts(e, &key));
        if args.get("html") != Some(&Value::Bool(true)) {
            strip_html_previews(&mut page);
        }
        let mut state = json!({
            "mounted": self.mesurer.mounted,
            "version": MESURER_VERSION,
            "commit": MESURER_COMMIT,
            "world": core::WORLD,
            "url": url,
            "page_key": key,
            "origin": origin,
            "summary": self.mesurer.summary,
            "chrome": envelope.map_or(Value::Null, chrome_of),
            "page": page,
            "ready": self.mesurer.ready,
            "captures": self.mesurer.captures,
            "store_path": store::workspace_file(self.project.as_deref(), &origin),
        });
        if e2e_run() {
            state["last_clipboard"] = json!(self.mesurer.last_clipboard);
        }
        state
    }

    /// mesurer in `app_state.browser`.
    pub(in crate::browser) fn mesurer_describe(&self) -> Value {
        json!({ "mounted": self.mesurer.mounted, "tool": self.mesurer.summary.tool })
    }
}

/// Comment threads' `htmlPreview`s (markup the page made, often long) left out of `page`.
fn strip_html_previews(page: &mut Value) {
    let Some(comments) = page.get_mut("comments").and_then(Value::as_array_mut) else {
        return;
    };
    for thread in comments {
        if let Some(target) = thread.get_mut("target").and_then(Value::as_object_mut) {
            target.remove("htmlPreview");
        }
        if let Some(thread) = thread.as_object_mut() {
            thread.remove("htmlPreview");
        }
    }
}
