//! Preview as a prototype: the screen works, and walks to the screens it leads to.
//!
//! - **It goes where it links.** A link to another screen of the page (`href="sawit-screen://<screen>/"`,
//!   `../<screen>/`, or `data-screen="<screen>"`) opens that screen in Preview, at the same size,
//!   zoom and frame. So does a link or button that goes nowhere (`href="#"`, no `href`) whose words
//!   name another screen: the tab bar's Cards goes to "Bank Cards / Mobile" without being wired.
//!   Back, a link or button saying so (or `data-back`), goes back the way it came.
//! - **Its controls work**, where the page's own script doesn't make them: a switch (`role="switch"`,
//!   or a class `switch`, `toggle`, `sw`) flips, its row's click too; a tab, segment, chip or item
//!   whose siblings mark one of them selected (`on`, `active`, `selected`, `current`,
//!   `aria-selected`, `aria-current`) takes the mark. Only Preview does this: the canvas's pictures
//!   show each screen as it was made.

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::screens;
use serde_json::json;

use crate::board::BoardView;

/// A screen Preview can walk to: its folder's name, its frame, and the words that name it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Linked {
    pub name: String,
    pub frame: NodeId,
    pub keys: Vec<String>,
}

/// The words a screen called `title` answers to, lower case: its name without the device
/// ("Bank Cards / Mobile": "bank cards"), and that name less its first words ("cards"), but never
/// a word of two letters ("in" of "Sign in").
pub(in crate::board) fn keys_of(title: &str) -> Vec<String> {
    let base = title.split(" / ").next().unwrap_or(title);
    let words: Vec<String> = base.split_whitespace().map(|w| w.to_lowercase()).collect();
    (0..words.len())
        .map(|i| words[i..].join(" "))
        .filter(|key| key.chars().count() > 2)
        .collect()
}

/// The screens `(name, frame, title)` with their words, a word two screens answer to left out:
/// a link that could mean either goes to neither.
pub(in crate::board) fn linked(screens: Vec<(String, NodeId, String)>) -> Vec<Linked> {
    let all: Vec<Linked> = screens
        .into_iter()
        .map(|(name, frame, title)| Linked {
            keys: keys_of(&title),
            name,
            frame,
        })
        .collect();
    let shared = |key: &String| all.iter().filter(|s| s.keys.contains(key)).count() > 1;
    all.iter()
        .map(|s| Linked {
            keys: s.keys.iter().filter(|k| !shared(k)).cloned().collect(),
            ..s.clone()
        })
        .collect()
}

/// The script Preview's page runs before its own, for the screen `here` among `screens`.
pub(in crate::board) fn script(here: &str, screens: &[Linked]) -> String {
    let list: Vec<_> = screens
        .iter()
        .map(|s| json!({"name": s.name, "keys": s.keys}))
        .collect();
    SCRIPT
        .replace("__HERE__", &json!(here).to_string())
        .replace("__SCREENS__", &json!(list).to_string())
}

const SCRIPT: &str = r##"(() => {
  const post = (m) => { if (window.ipc) window.ipc.postMessage(m); };
  const HERE = __HERE__, SCREENS = __SCREENS__;
  const known = (name) => name && name !== HERE && SCREENS.some(s => s.name === name);
  const words = (el) => (el.getAttribute('aria-label') || el.textContent || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const NOWHERE = /^(#.*)?$/;
  const goesTo = (el) => {
    const marked = el.closest('[data-screen]');
    if (marked) return known(marked.dataset.screen) ? marked.dataset.screen : null;
    const link = el.closest('a[href]');
    const href = link ? link.getAttribute('href').trim() : '';
    if (link && !NOWHERE.test(href)) {
      const m = href.match(/^sawit-screen:\/\/([^\/?#]+)/i) || href.match(/^(?:\.\.\/)+([^\/?#]+)\/?(?:index\.html)?(?:[?#].*)?$/);
      return m && known(m[1]) ? m[1] : null;
    }
    const item = el.closest('a, button, [role=button], [role=link], [role=tab], [role=menuitem]');
    if (!item) return null;
    const said = words(item);
    const hit = said && SCREENS.find(s => s.name !== HERE && s.keys.includes(said));
    return hit ? hit.name : null;
  };
  const goesBack = (el) => {
    const item = el.closest('[data-back], a, button, [role=button], [role=link]');
    if (!item) return false;
    if (item.hasAttribute('data-back')) return true;
    const link = item.closest('a[href]');
    if (link && !NOWHERE.test(link.getAttribute('href').trim())) return false;
    return /^(back|‹|<|←)$/.test(words(item));
  };

  const ON = ['on', 'active', 'selected', 'current', 'checked', 'is-on', 'is-active', 'is-selected', 'is-checked'];
  const marked = (el) => ON.some(t => el.classList.contains(t)) || el.getAttribute('aria-selected') === 'true' || (el.hasAttribute('aria-current') && el.getAttribute('aria-current') !== 'false');
  const isSwitch = (el) => el.getAttribute('role') === 'switch' || [...el.classList].some(t => /^(switch|toggle|sw)$|[-_](switch|toggle)$/.test(t));
  const switchIn = (el) => {
    for (let n = el; n && n !== document.body; n = n.parentElement) if (isSwitch(n)) return n;
    // A row with one switch flips it, as a settings row does; not a click on another control.
    if (el.closest('a[href]:not([href="#"]), button, input, select, textarea, [role=tab]')) return null;
    for (let n = el, up = 0; n && n !== document.body && up < 3; n = n.parentElement, up++) {
      const found = [...n.querySelectorAll('*')].filter(isSwitch);
      if (found.length === 1) return found[0];
      if (found.length > 1) return null;
    }
    return null;
  };
  const state = (el) => el ? el.className + '|' + el.getAttribute('aria-checked') + '|' + el.getAttribute('aria-selected') + '|' + el.getAttribute('aria-current') : '';
  const flip = (sw) => {
    const on = sw.getAttribute('aria-checked');
    if (on !== null) sw.setAttribute('aria-checked', on === 'true' ? 'false' : 'true');
    if (sw.classList.contains('off')) { sw.classList.remove('off'); if (sw.dataset.sawitOn) sw.classList.add(...sw.dataset.sawitOn.split(' ')); return; }
    const had = ON.filter(t => sw.classList.contains(t));
    if (had.length) { sw.dataset.sawitOn = had.join(' '); sw.classList.remove(...had); sw.classList.add('off'); return; }
    if (on === null) sw.classList.toggle('on');
  };
  // The clicked item, or one of the few elements around it, takes the selected mark from the
  // sibling that has it: a tab bar's, a segmented control's, a row of chips.
  const select = (el) => {
    for (let item = el, up = 0; item && item !== document.body && up < 4; item = item.parentElement, up++) {
      if (marked(item)) return;
      const parent = item.parentElement;
      if (!parent) return;
      const from = [...parent.children].find(c => c !== item && c.tagName === item.tagName && marked(c));
      if (!from) continue;
      for (const t of ON) if (from.classList.contains(t)) { from.classList.remove(t); item.classList.add(t); }
      if (from.getAttribute('aria-selected') === 'true') { from.setAttribute('aria-selected', 'false'); item.setAttribute('aria-selected', 'true'); }
      if (from.hasAttribute('aria-current')) { item.setAttribute('aria-current', from.getAttribute('aria-current')); from.removeAttribute('aria-current'); }
      return;
    }
  };

  let before = null;
  addEventListener('click', (e) => {
    const t = e.target instanceof Element ? e.target : null;
    if (!t) return;
    const to = goesTo(t);
    if (to) { e.preventDefault(); e.stopImmediatePropagation(); post('sawit-web:go:' + to); return; }
    if (goesBack(t)) { e.preventDefault(); e.stopImmediatePropagation(); post('sawit-web:back'); return; }
    const sw = switchIn(t);
    before = { sw, sws: state(sw) };
  }, true);
  addEventListener('click', (e) => {
    const t = e.target instanceof Element ? e.target : null;
    const was = before; before = null;
    // The page's own script handled it: leave it be.
    if (!t || !was || e.defaultPrevented) return;
    const link = t.closest('a[href]');
    if (link && NOWHERE.test(link.getAttribute('href').trim())) e.preventDefault();
    if (was.sw && state(was.sw) !== was.sws) return;
    if (was.sw) { flip(was.sw); return; }
    if (!t.closest('input, select, textarea, label')) select(t);
  });
  const mark = () => {
    for (const el of document.querySelectorAll('a, button, [role=button], [role=link], [role=tab], [data-screen], [data-back]'))
      if (goesTo(el) || goesBack(el)) el.style.cursor = 'pointer';
  };
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', mark); else mark();
})();"##;

impl BoardView {
    /// The screens on the canvas's page, as Preview walks among them.
    pub(in crate::board) fn page_screens(&self) -> Vec<Linked> {
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return Vec::new();
        };
        let list = super::super::web::web_nodes(&ds.doc, ds.page)
            .iter()
            .filter_map(|node| {
                let found = ds.doc.node(node.id)?;
                let data = screens::screen_of(found)?;
                Some((data.screen, node.id, found.name.clone()))
            })
            .collect();
        linked(list)
    }

    /// A Preview page's click went to screen `name`: Preview shows it at the next draw, at the same
    /// size, zoom and frame, with the way back.
    pub(in crate::board) fn preview_go(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(to) = self.page_screens().into_iter().find(|s| s.name == name) else {
            return;
        };
        let _ = self.preview_walk_to(to.frame, cx);
    }

    /// Preview walks to screen frame `frame` at the next draw, with the way back: a prototype's
    /// link followed, by the page or by an agent (`canvas_preview`).
    pub(in crate::board) fn preview_walk_to(
        &mut self,
        frame: NodeId,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let preview = (self.screen_ui.preview.as_ref())
            .ok_or("Preview isn't open: open a screen in it first")?;
        if preview.frame == frame {
            return Err("Preview already shows that screen".into());
        }
        let mut back = preview.back.clone();
        back.push(preview.frame);
        self.screen_ui.walk = Some((frame, back));
        cx.notify();
        Ok(())
    }

    /// Back, for an agent: an error when Preview isn't open or came from no other screen.
    pub(in crate::board) fn preview_back_op(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match self.screen_ui.preview.as_ref() {
            None => Err("Preview isn't open".into()),
            Some(p) if p.back.is_empty() => Err("Preview came from no other screen".into()),
            Some(_) => {
                self.preview_back(cx);
                Ok(())
            }
        }
    }

    /// Back: the screen Preview came from, if it came from one.
    pub(in crate::board) fn preview_back(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.screen_ui.preview.as_ref() else {
            return;
        };
        let mut back = preview.back.clone();
        let Some(to) = back.pop() else {
            return;
        };
        self.screen_ui.walk = Some((to, back));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;

    use super::{keys_of, linked, script};

    #[test]
    fn a_screen_answers_to_its_name_and_its_name_less_the_app() {
        assert_eq!(keys_of("Bank Cards / Mobile"), ["bank cards", "cards"]);
        assert_eq!(
            keys_of("Bank Sign in / Mobile"),
            ["bank sign in", "sign in"]
        );
        assert_eq!(keys_of("Today"), ["today"]);
    }

    /// A word two screens answer to names neither: "Transfer" could be any of three.
    #[test]
    fn a_shared_word_links_nowhere() {
        let list = linked(vec![
            (
                "a".into(),
                NodeId(1),
                "Bank Transfer amount / Mobile".into(),
            ),
            ("b".into(), NodeId(2), "Bank Transfer sent / Mobile".into()),
            ("c".into(), NodeId(3), "Bank Cards / Mobile".into()),
        ]);
        assert_eq!(
            list[0].keys,
            ["bank transfer amount", "transfer amount", "amount"]
        );
        assert_eq!(list[2].keys, ["bank cards", "cards"]);
        assert!(list.iter().all(|s| !s.keys.contains(&"bank".to_string())));
    }

    #[test]
    fn the_script_names_the_screens() {
        let list = linked(vec![("c".into(), NodeId(3), "Cards".into())]);
        let js = script("here", &list);
        assert!(js.contains(r#"HERE = "here""#), "{js}");
        assert!(
            js.contains(r#"SCREENS = [{"name":"c","keys":["cards"]}]"#),
            "{js}"
        );
        assert!(!js.contains("__"), "every placeholder filled");
    }
}
