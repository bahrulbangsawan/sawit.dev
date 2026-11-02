//! Where an agent's op lands: what it added, changed or took off the page, found by comparing the
//! document before and after it ([`target`]), and where a slow op's placeholder goes
//! ([`ghost_rect`]).

use figcraft_doc::{Document, NodeId};
use serde::Deserialize as _;
use serde_json::Value;

use super::{GHOST_SIZE, MAX_OUTLINES};

/// Where an agent's op landed on page `page`: what it added, changed or took off there, from the
/// document `before` and `after` it, each layer once: a sticky drawn with its text is the sticky.
/// Pages, and layers whose only change is what they hold (a frame given a new child), are left
/// out. `None` when nothing on the page changed.
pub(super) fn target(before: &Document, after: &Document, page: NodeId) -> Option<Target> {
    let on_page = |doc: &Document, id: NodeId| {
        doc.node(id).is_some_and(|n| !n.is_page()) && doc.page_of(id) == Some(page)
    };
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for node in after.nodes() {
        if !on_page(after, node.id) {
            continue;
        }
        match before.node(node.id) {
            None => added.push(node.id),
            Some(old) if !std::ptr::eq(old, node) && !only_children_differ(old, node) => {
                changed.push(node.id);
            }
            Some(_) => {}
        }
    }
    let removed: Vec<NodeId> = before
        .nodes()
        .filter(|n| on_page(before, n.id) && after.node(n.id).is_none())
        .map(|n| n.id)
        .collect();
    // The outermost of what changed: a layer inside one added or changed with it is part of it.
    let all: Vec<NodeId> = added.iter().chain(&changed).copied().collect();
    let outermost = |id: &NodeId| {
        !all.iter()
            .any(|other| other != id && after.is_ancestor(*other, *id))
    };
    added.retain(outermost);
    changed.retain(outermost);
    let bounds = |doc: &Document, ids: &[NodeId]| -> Vec<[f64; 4]> {
        ids.iter()
            .filter_map(|id| doc.world_bounds(*id))
            .filter(|r| {
                r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
            })
            .map(|r| [r.x0, r.y0, r.x1, r.y1])
            .collect()
    };
    let new = bounds(after, &added);
    let mut touched: Vec<[f64; 4]> = new.iter().copied().chain(bounds(after, &changed)).collect();
    if touched.len() > MAX_OUTLINES {
        touched = union(&touched).into_iter().collect();
    }
    if let Some([x0, y0, _, _]) = union(&new) {
        return Some(Target {
            point: (x0, y0),
            rects: touched,
        });
    }
    let gone = bounds(before, &removed);
    let [x0, y0, x1, y1] = union(&touched).or_else(|| union(&gone))?;
    Some(Target {
        point: ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        rects: touched,
    })
}

/// Whether `old` and `new` are one layer whose only change is the layers it holds.
fn only_children_differ(old: &figcraft_doc::Node, new: &figcraft_doc::Node) -> bool {
    if old.children == new.children {
        return false;
    }
    let (mut old, mut new) = (old.clone(), new.clone());
    old.children.clear();
    new.children.clear();
    old == new
}

/// Where an op landed: the point its pointer goes to, and the layers to outline.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Target {
    pub point: (f64, f64),
    pub rects: Vec<[f64; 4]>,
}

pub(super) fn union(rects: &[[f64; 4]]) -> Option<[f64; 4]> {
    rects
        .iter()
        .copied()
        .reduce(|[a, b, c, d], [e, f, g, h]| [a.min(e), b.min(f), c.max(g), d.max(h)])
}

/// The page rectangle where slow op `op`'s result will land, with the view's centre at
/// `centre`: what an `add` will draw (`sawit_board::skeleton::bounds`), else a diagram's or
/// picture's corner (`x`, `y`) when the op names one, else `corner`, where the op puts it when it
/// names none (a diagram's), else the view's centre; as big as it will be when that is known,
/// else [`GHOST_SIZE`].
pub(super) fn ghost_rect(op: &Value, centre: (f64, f64), corner: Option<(f64, f64)>) -> [f64; 4] {
    let (w, h) = GHOST_SIZE;
    if op.get("op").and_then(Value::as_str) == Some("add") {
        let items: Vec<sawit_board::skeleton::Skeleton> = op
            .get("elements")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| sawit_board::skeleton::Skeleton::deserialize(item).ok())
                    .collect()
            })
            .unwrap_or_default();
        if let Some((x0, y0, x1, y1)) = sawit_board::skeleton::bounds(&items) {
            return [x0, y0, x1.max(x0 + 24.0), y1.max(y0 + 24.0)];
        }
    }
    let params = op.get("params").unwrap_or(op);
    let number = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    match number("x").zip(number("y")).or(corner) {
        Some((x, y)) => {
            let w = number("width").filter(|w| *w > 0.0).unwrap_or(w);
            let h = number("height").filter(|h| *h > 0.0).unwrap_or(h);
            [x, y, x + w, y + h]
        }
        None => {
            let (cx, cy) = centre;
            [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
        }
    }
}
