//! The machine checks of an artboard: a frame built of vector layers. Text is measured against
//! the solid fills under it (its ancestors'), controls are found by their names (or their main
//! component's), and auto layout gaps and paddings are read from the document.

use figcraft_color::PaintKind;
use figcraft_doc::{Document, Node, NodeId, NodeKind};

use super::{
    Finding, MIN_TARGET, MIN_TARGET_TOUCH, MIN_TEXT, Rgba, TOUCH_WIDTH, contrast, contrast_fix,
    dedup, needed_contrast, on_scale, size_fix, spacing_fix, target_fix,
};

/// Words in a layer's name that make it a control, which needs a target's size.
const CONTROL_WORDS: &[&str] = &[
    "button",
    "btn",
    "link",
    "tab",
    "toggle",
    "switch",
    "checkbox",
    "radio",
    "chip",
    "close",
    "menu item",
    "icon button",
    "cta",
];

/// The failures of frame `frame` in `doc`: one per rule and layer, top to bottom.
pub fn check(doc: &Document, frame: NodeId) -> Vec<Finding> {
    let Some(root) = doc.node(frame) else {
        return Vec::new();
    };
    let touch = root.width < TOUCH_WIDTH;
    let mut out = Vec::new();
    walk(
        doc,
        root,
        Background::Color(Rgba::WHITE),
        1.0,
        touch,
        &mut out,
    );
    dedup(out)
}

/// What is painted behind a layer.
#[derive(Clone, Copy)]
enum Background {
    Color(Rgba),
    /// An image or a gradient: text over it is measured at its worst point by eye.
    Picture,
}

fn walk(
    doc: &Document,
    node: &Node,
    behind: Background,
    opacity: f64,
    touch: bool,
    out: &mut Vec<Finding>,
) {
    if !node.visible {
        return;
    }
    let opacity = opacity * f64::from(node.opacity);
    let id = node.id.to_string();
    if let NodeKind::Text(text) = &node.kind {
        let size = text.style.font_size;
        if size < MIN_TEXT - 1e-6 && !text.characters.trim().is_empty() {
            out.push(Finding::at_layer(
                "typ.11",
                &node.name,
                id.clone(),
                size_fix(size, MIN_TEXT),
            ));
        }
        if let (Background::Color(bg), Some(fg)) = (behind, solid(node, opacity)) {
            let weight = text.style.weight() as u32;
            let ratio = contrast(fg, bg);
            let needed = needed_contrast(size, weight);
            if ratio < needed {
                let fix = contrast_fix(fg, bg, ratio, needed);
                out.push(Finding::at_layer("col.10", &node.name, id.clone(), fix));
            }
        }
        escapes(doc, node, out);
        return;
    }
    if is_control(doc, node) {
        let floor = if touch { MIN_TARGET_TOUCH } else { MIN_TARGET };
        if node.width < floor - 0.01 || node.height < floor - 0.01 {
            let fix = target_fix(node.width, node.height, floor);
            out.push(Finding::at_layer("a11y.2", &node.name, id.clone(), fix));
        }
    }
    if let Some(layout) = &node.layout {
        let sides = [
            "Top padding",
            "Right padding",
            "Bottom padding",
            "Left padding",
        ];
        let mut lengths = vec![("The gap", layout.gap)];
        lengths.extend(sides.iter().copied().zip(layout.padding));
        if let Some((what, px)) = lengths.into_iter().find(|(_, px)| !on_scale(*px)) {
            out.push(Finding::at_layer(
                "spc.1",
                &node.name,
                id.clone(),
                spacing_fix(what, px),
            ));
        }
    }
    let behind = match fill(node) {
        Some(Fill::Picture) => Background::Picture,
        Some(Fill::Color(c)) => match behind {
            Background::Color(under) => Background::Color(c.over(under)),
            Background::Picture if c.a >= 1.0 => Background::Color(c),
            Background::Picture => Background::Picture,
        },
        None => behind,
    };
    for child in &node.children {
        if let Some(child) = doc.node(*child) {
            walk(doc, child, behind, opacity, touch, out);
        }
    }
}

/// A layer's top visible fill.
enum Fill {
    Color(Rgba),
    Picture,
}

fn fill(node: &Node) -> Option<Fill> {
    let paint = node.fills.iter().rev().find(|p| p.visible)?;
    Some(match &paint.kind {
        PaintKind::Solid { color } => Fill::Color(Rgba {
            r: f64::from(color.r),
            g: f64::from(color.g),
            b: f64::from(color.b),
            a: f64::from(color.a) * f64::from(paint.opacity),
        }),
        _ => Fill::Picture,
    })
}

/// A text's colour, with the opacity of it and its ancestors.
fn solid(node: &Node, opacity: f64) -> Option<Rgba> {
    match fill(node)? {
        Fill::Color(c) => Some(Rgba {
            a: c.a * opacity,
            ..c
        }),
        Fill::Picture => None,
    }
}

/// Whether `node` is a control by its name, or its main component's.
fn is_control(doc: &Document, node: &Node) -> bool {
    let named = |name: &str| {
        let name = name.to_lowercase();
        let words: Vec<&str> = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        CONTROL_WORDS.iter().any(|w| match w.contains(' ') {
            true => name.contains(w),
            false => words.contains(w),
        })
    };
    if !matches!(
        node.kind,
        NodeKind::Frame
            | NodeKind::Instance { .. }
            | NodeKind::Component { .. }
            | NodeKind::Group
            | NodeKind::Rectangle
    ) {
        return false;
    }
    if named(&node.name) {
        return true;
    }
    match &node.kind {
        NodeKind::Instance { component, .. } => {
            let main = doc.node(*component);
            let set = main.and_then(|m| m.parent).and_then(|p| doc.node(p));
            let set = set.filter(|s| matches!(s.kind, NodeKind::ComponentSet { .. }));
            main.is_some_and(|m| named(&m.name)) || set.is_some_and(|s| named(&s.name))
        }
        _ => false,
    }
}

/// lay.18: a text whose box reaches out of its parent's.
fn escapes(doc: &Document, node: &Node, out: &mut Vec<Finding>) {
    let parent = node.parent.and_then(|p| doc.node(p));
    let Some(parent) = parent.filter(|p| p.kind.is_frame_like()) else {
        return;
    };
    let (Some(own), Some(box_)) = (doc.world_bounds(node.id), doc.world_bounds(parent.id)) else {
        return;
    };
    let past = (own.x1 - box_.x1)
        .max(own.y1 - box_.y1)
        .max(box_.x0 - own.x0)
        .max(box_.y0 - own.y0);
    if past > 1.0 {
        let fix = format!(
            "The text reaches {}px out of “{}”: let it wrap at the frame's width with its height \
             hugging, or shorten it.",
            past.round(),
            parent.name
        );
        out.push(Finding::at_layer(
            "lay.18",
            &node.name,
            node.id.to_string(),
            fix,
        ));
    }
}
