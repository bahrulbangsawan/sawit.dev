//! Screens built live (30e): after an agent writes a screen and its page has settled, a
//! blueprint of the rendered page is taken inside the screen's own web view ([`SCRIPT`], Sawit's
//! own, small and dependency-free): the boxes with a fill or a border, the text blocks, the
//! images and SVGs, each repeated sibling tied to its master. It is compared with the screen's
//! last blueprint ([`changed`]), and only what changed plays, over the screen's frame, with the
//! canvas's element animations ([`tasks`]); each element's cover then fades to the live page.

use std::collections::HashMap;

use figcraft_doc::NodeId;
use serde::Deserialize;

use super::script::{Kind, Task, fit};

/// The blueprint script, run in a screen's web view; it answers JSON (as a string).
pub(in crate::board) const SCRIPT: &str = r#"(() => {
  const items = [], masters = new Map(), keys = new Map();
  const sx = window.scrollX, sy = window.scrollY;
  const walk = (el, path) => {
    if (items.length >= 300) return;
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || +cs.opacity === 0) return;
    const r = el.getBoundingClientRect();
    const tag = el.tagName.toLowerCase();
    const own = [...el.childNodes].filter(c => c.nodeType === 3).map(c => c.textContent).join(' ').trim();
    const bg = cs.backgroundColor;
    const filled = bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent';
    const bordered = parseFloat(cs.borderTopWidth) > 0;
    const img = ['img', 'svg', 'video', 'canvas', 'picture'].includes(tag);
    keys.set(el, path);
    if (r.width > 1 && r.height > 1 && (own || filled || bordered || img)) {
      const sig = tag + '.' + el.className + '#' + el.children.length + '@' + Math.round(r.width) + 'x' + Math.round(r.height);
      const parent = el.parentElement;
      let dup = null;
      if (parent) {
        const seen = masters.get(parent) || new Map();
        if (seen.has(sig)) dup = seen.get(sig); else seen.set(sig, path);
        masters.set(parent, seen);
      }
      items.push({ k: path, r: [r.left + sx, r.top + sy, r.width, r.height],
        bg: filled ? bg : null, text: own ? own.slice(0, 80) : null, img, dup });
    }
    if (tag === 'svg') return;
    let i = 0;
    for (const child of el.children) walk(child, path + '/' + child.tagName.toLowerCase() + i++);
  };
  if (document.body) walk(document.body, 'body');
  return JSON.stringify({ w: window.innerWidth, h: document.documentElement.scrollHeight, items });
})()"#;

/// A page's blueprint: its viewport width, its height, and its elements in document order.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(in crate::board) struct Blueprint {
    pub w: f64,
    #[allow(dead_code)]
    pub h: f64,
    pub items: Vec<Item>,
}

/// One element: its key (its path in the page), its box (CSS px from the page's top-left), its
/// fill, its own text, whether it is a picture, and the key of the sibling it repeats.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(in crate::board) struct Item {
    pub k: String,
    pub r: [f64; 4],
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub img: bool,
    #[serde(default)]
    pub dup: Option<String>,
}

impl Blueprint {
    /// The blueprint in what [`SCRIPT`] answered (WebKit hands back the JSON string as JSON).
    pub fn parse(json: &str) -> Option<Self> {
        let text = serde_json::from_str::<String>(json).unwrap_or_else(|_| json.to_string());
        serde_json::from_str(&text).ok()
    }
}

/// The elements of `new` that `old` didn't have, or had otherwise (moved, resized, refilled,
/// retyped), in document order. Without an `old`, all of them.
pub(in crate::board) fn changed<'a>(old: Option<&Blueprint>, new: &'a Blueprint) -> Vec<&'a Item> {
    let Some(old) = old else {
        return new.items.iter().collect();
    };
    let before: HashMap<&str, &Item> = old.items.iter().map(|i| (i.k.as_str(), i)).collect();
    let near = |a: &[f64; 4], b: &[f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5);
    (new.items.iter())
        .filter(|item| match before.get(item.k.as_str()) {
            None => true,
            Some(was) => {
                !near(&was.r, &item.r)
                    || was.bg != item.bg
                    || was.text != item.text
                    || was.img != item.img
            }
        })
        .collect()
}

/// The tasks that play `items` (of `blueprint`) over screen frame `frame`, at page rect `rect`.
pub(in crate::board) fn tasks(
    items: &[&Item],
    blueprint: &Blueprint,
    frame: NodeId,
    rect: [f64; 4],
) -> Vec<Task> {
    let scale = (rect[2] - rect[0]) / blueprint.w.max(1.0);
    let on_page = |r: &[f64; 4]| {
        let x0 = rect[0] + r[0] * scale;
        let y0 = rect[1] + r[1] * scale;
        [
            x0,
            y0,
            (x0 + r[2] * scale).min(rect[2]),
            (y0 + r[3] * scale).min(rect[3]),
        ]
    };
    let by_key: HashMap<&str, &Item> = blueprint.items.iter().map(|i| (i.k.as_str(), i)).collect();
    let mut out: Vec<Task> = (items.iter().take(80))
        .map(|item| on_page(&item.r))
        .zip(items.iter())
        .filter(|(r, _)| r[1] < rect[3] && r[2] > r[0] && r[3] > r[1])
        .map(|(page, item)| {
            let master = item.dup.as_deref().and_then(|k| by_key.get(k));
            let kind = match (master, item.img, &item.text) {
                (Some(m), ..) => Kind::Duplicate {
                    from: on_page(&m.r),
                },
                (None, true, _) => Kind::Image,
                (None, false, Some(t)) if item.bg.is_none() => Kind::Text {
                    chars: t.chars().count(),
                    retype: false,
                },
                _ => Kind::Shape,
            };
            let mut task = Task::new(frame, kind, page, None);
            task.cover = true;
            task
        })
        .collect();
    fit(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(k: &str, r: [f64; 4], text: Option<&str>) -> Item {
        Item {
            k: k.into(),
            r,
            bg: None,
            text: text.map(Into::into),
            img: false,
            dup: None,
        }
    }

    /// A second write that changes one heading and adds a card plays only those two; the first
    /// plays everything; a repeated card is the master's copy.
    #[test]
    fn only_what_changed_plays_and_repeats_are_copies() {
        let first = Blueprint {
            w: 400.0,
            h: 800.0,
            items: vec![
                item("body/h10", [16.0, 16.0, 200.0, 32.0], Some("Plans")),
                item("body/p1", [16.0, 60.0, 300.0, 20.0], Some("Pick one")),
            ],
        };
        assert_eq!(changed(None, &first).len(), 2);
        let mut second = first.clone();
        second.items[0].text = Some("Pricing".into());
        let mut card = item("body/div2", [16.0, 100.0, 180.0, 200.0], None);
        card.bg = Some("rgb(255, 255, 255)".into());
        let mut copy = item("body/div3", [216.0, 100.0, 180.0, 200.0], None);
        copy.bg = card.bg.clone();
        copy.dup = Some("body/div2".into());
        second.items.extend([card, copy]);
        let changed = changed(Some(&first), &second);
        let keys: Vec<&str> = changed.iter().map(|i| i.k.as_str()).collect();
        assert_eq!(keys, ["body/h10", "body/div2", "body/div3"]);
        let tasks = tasks(&changed, &second, NodeId(7), [1000.0, 0.0, 1800.0, 1600.0]);
        assert_eq!(tasks.len(), 3);
        assert!(matches!(tasks[0].kind, Kind::Text { chars: 7, .. }));
        assert_eq!(tasks[1].kind, Kind::Shape);
        assert_eq!(
            tasks[2].kind,
            Kind::Duplicate {
                from: [1032.0, 200.0, 1392.0, 600.0]
            }
        );
        assert!(tasks.iter().all(|t| t.cover && t.node == NodeId(7)));
        assert_eq!(
            tasks[1].rect,
            [1032.0, 200.0, 1392.0, 600.0],
            "CSS px at the frame's scale"
        );
        assert_eq!(
            Blueprint::parse("\"{\\\"w\\\":1,\\\"h\\\":2,\\\"items\\\":[]}\"")
                .unwrap()
                .w,
            1.0
        );
    }
}
