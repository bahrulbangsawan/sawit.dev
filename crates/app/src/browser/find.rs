//! ⌘F in the browser: the find bar above the page searches the text the page shows. ⌘F reaches it
//! even while the page has the keyboard (a ⌘ key reaches the app first, as a key equivalent), and
//! the page's own ⌘F handler never runs. The search runs in Sawit's own script world, so the page's
//! scripts neither see nor change it: every match is highlighted with the CSS Custom Highlight API,
//! the current one more strongly, and scrolled to the middle of the page. WebKit's
//! `WKWebView.find(_:configuration:)` was the other way: it marks only the match it lands on, and
//! counts none, so the bar couldn't say "3 of 12". Closing the bar takes the highlights away and
//! gives the page back the keyboard if it had it.

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::find::{FindState, pattern};
use serde_json::{Value, json};

use super::BrowserView;
use crate::find::{FindEvent, Finder, Supports};

/// The page's side of find, run in Sawit's world with `json`: `find` (with `pattern` and `flags`,
/// a JavaScript `RegExp`), `step` (by `by`) or `clear`. Answers `{total, index}`, or `{error}`.
const SCRIPT: &str = r#"
const a = JSON.parse(json);
const S = (globalThis.__sawitFind ||= { ranges: [], index: -1 });
const hl = globalThis.CSS && CSS.highlights;
const clear = () => { if (hl) { hl.delete('sawit-find'); hl.delete('sawit-find-current'); } };
if (a.op === 'clear') {
  clear();
  S.ranges = []; S.index = -1;
  document.getElementById('sawit-find-style')?.remove();
  return JSON.stringify({ total: 0, index: -1 });
}
if (a.op === 'find') {
  clear();
  S.ranges = []; S.index = -1;
  let re;
  try { re = new RegExp(a.pattern, a.flags); } catch (e) { return JSON.stringify({ error: 'Invalid regular expression' }); }
  const root = document.body || document.documentElement;
  const skip = 'script,style,noscript,template,head';
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(n) {
      const p = n.parentElement;
      if (!p || p.closest(skip) || !p.getClientRects().length) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  const top = window.scrollY;
  for (let n; (n = walker.nextNode()) && S.ranges.length < 10000;) {
    re.lastIndex = 0;
    for (let m; (m = re.exec(n.data));) {
      if (!m[0].length) { re.lastIndex++; continue; }
      const r = new Range();
      r.setStart(n, m.index);
      r.setEnd(n, m.index + m[0].length);
      S.ranges.push(r);
    }
  }
  // The first match from where the page is scrolled to.
  if (S.ranges.length) {
    const at = S.ranges.findIndex((r) => r.getBoundingClientRect().top + window.scrollY >= top);
    S.index = at < 0 ? 0 : at;
  }
}
if (a.op === 'step' && S.ranges.length) {
  S.index = ((S.index < 0 ? 0 : S.index + a.by) % S.ranges.length + S.ranges.length) % S.ranges.length;
}
if (!document.getElementById('sawit-find-style')) {
  const style = document.createElement('style');
  style.id = 'sawit-find-style';
  style.textContent = '::highlight(sawit-find){background:rgba(250,190,40,.45);color:inherit}' +
    '::highlight(sawit-find-current){background:rgb(255,140,30);color:#000}';
  (document.head || document.documentElement).append(style);
}
const current = S.ranges[S.index];
if (hl) {
  hl.set('sawit-find', new Highlight(...S.ranges));
  if (current) hl.set('sawit-find-current', new Highlight(current));
} else if (current) {
  const sel = getSelection(); sel.removeAllRanges(); sel.addRange(current);
}
if (current) current.startContainer.parentElement?.scrollIntoView({ block: 'center', inline: 'nearest' });
return JSON.stringify({ total: S.ranges.length, index: S.index });
"#;

/// The browser's find: its bar, and whether the page had the keyboard when it opened.
#[derive(Default)]
pub(super) struct BrowserFind {
    pub(super) finder: Finder,
    page_had_keyboard: bool,
}

impl BrowserView {
    /// `el` with the find keys: ⌘F, ⌘G, ⇧⌘G and the bar's Escape. The bar goes above the page,
    /// not over it: the page is a native view, which GPUI can't draw on.
    pub(super) fn with_find_keys(el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &crate::find::Find, w, cx| this.open_find(w, cx)))
            .on_action(
                cx.listener(|this, _: &crate::find::FindNext, _, cx| this.find_step(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::FindPrevious, _, cx| this.find_step(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::CloseFind, w, cx| this.close_find(w, cx)),
            )
    }

    /// ⌘F: the bar above the page, its query the text selected in the page. The page gives the
    /// keyboard to the bar's field while it has it.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if !self.find.finder.is_open() {
            self.find.page_had_keyboard = engine.has_keyboard();
        }
        engine.focus_parent();
        let bar = self.find.finder.open(
            Some(Supports::ALL),
            None,
            false,
            Self::on_find_event,
            window,
            cx,
        );
        // The page's selection (it is the page's, in every script world) seeds the query.
        let (tx, rx) = oneshot::channel();
        engine.world_eval("String(getSelection())", move |selected| {
            let _ = tx.send(selected);
        });
        let bar = bar.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(text))) = rx.await else { return };
            if text.trim().is_empty() {
                return;
            }
            let _ = cx.update(|window, cx| {
                if let Some(bar) = bar.upgrade() {
                    bar.update(cx, |bar, cx| bar.open(Some(&text), false, window, cx));
                }
            });
        })
        .detach();
    }

    fn on_find_event(&mut self, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => self.run_find(json!({ "op": "find" }), cx),
            FindEvent::Step(forward) => self.find_step(*forward, cx),
            FindEvent::Close => self.close_find(window, cx),
            FindEvent::ReplaceOne | FindEvent::ReplaceAll => {}
        }
    }

    /// ⌘G, ⇧⌘G, ↩ and ⇧↩ in the bar, its arrows.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.find.finder.is_open() {
            cx.propagate();
            return;
        }
        let by = if forward { 1 } else { -1 };
        self.run_find(json!({ "op": "step", "by": by }), cx);
    }

    /// Escape in the bar or its ✕: the highlights go, and the keyboard goes back to the page if
    /// it had it, else to where it was.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        if !self.find.finder.close(&focus, window, cx) {
            cx.propagate();
            return;
        }
        if let Some(engine) = &self.engine {
            engine.world_call(SCRIPT, &json!({ "op": "clear" }).to_string(), |_| {});
        }
        if std::mem::take(&mut self.find.page_had_keyboard) {
            self.give_page_keyboard(window, cx);
        }
        cx.notify();
    }

    /// A page loaded while the bar is open: its matches are found again.
    pub(super) fn find_after_load(&mut self, cx: &mut Context<Self>) {
        if self.find.finder.is_open() {
            self.run_find(json!({ "op": "find" }), cx);
        }
    }

    /// Runs the page's side of find with `op`, the bar's query and options added, and shows what
    /// it answers in the bar.
    fn run_find(&mut self, mut op: Value, cx: &mut Context<Self>) {
        let (Some(engine), Some(bar)) = (self.engine.clone(), self.find.finder.bar().cloned())
        else {
            return;
        };
        let (query, options) = bar.read_with(cx, |bar, cx| (bar.query(cx), bar.options()));
        if query.is_empty() {
            op = json!({ "op": "clear" });
        }
        op["pattern"] = pattern(&query, options).into();
        op["flags"] = if options.match_case { "g" } else { "gi" }.into();
        let (tx, rx) = oneshot::channel();
        engine.world_call(SCRIPT, &op.to_string(), move |answer| {
            let _ = tx.send(answer);
        });
        let bar = bar.downgrade();
        cx.spawn(async move |_, cx| {
            let answer: Value = match rx.await {
                Ok(Ok(Some(text))) => serde_json::from_str(&text).unwrap_or_default(),
                _ => Value::Null,
            };
            let problem = answer["error"].as_str().map(str::to_string);
            let total = answer["total"].as_u64().unwrap_or(0) as usize;
            let index = answer["index"]
                .as_i64()
                .filter(|i| *i >= 0)
                .map(|i| i as usize);
            let found = FindState {
                total,
                current: index.filter(|_| total > 0),
            };
            cx.update(|cx| {
                if let Some(bar) = bar.upgrade() {
                    bar.update(cx, |bar, cx| bar.set_found(found, problem, cx));
                }
            });
        })
        .detach();
    }
}
