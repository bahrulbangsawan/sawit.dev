//! From a page's [`Capture`] to the [`Layer`]s that draw it: which elements become frames, which
//! are lifted out of the way (a wrapper that draws nothing around one child), which text, images
//! and pictures go where, and where auto layout holds the page's own spacing.

use super::{Capture, Item, Style};

/// Within this many CSS px, two edges or two gaps are the same.
const NEAR: f64 = 1.5;

/// What a layer is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Frame,
    /// A text layer and what it says.
    Text(String),
    /// A raster picture, as a `data:` URL.
    Image(String),
    /// An SVG picture, imported as editable vectors.
    Svg(String),
    /// A picture that couldn't be read: an empty slot at its size.
    Slot,
}

/// How a layer is sized in its parent's auto layout, along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sizing {
    #[default]
    Fixed,
    Fill,
    Hug,
}

/// A frame's auto layout.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoLayout {
    pub vertical: bool,
    pub gap: f64,
    /// Top, right, bottom, left.
    pub padding: [f64; 4],
    /// `min`, `center`, `max` or `spaceBetween`.
    pub main: &'static str,
    /// `min`, `center` or `max`.
    pub cross: &'static str,
}

/// A layer to make: its box on the page (CSS px from the page's top-left), its looks, and its
/// children.
#[derive(Clone, Debug)]
pub struct Layer {
    pub kind: Kind,
    pub name: Option<String>,
    /// The element's tag, which names a frame the page gave no name.
    pub tag: Option<String>,
    pub rect: [f64; 4],
    pub style: Style,
    pub layout: Option<AutoLayout>,
    pub sizing: (Sizing, Sizing),
    /// Out of its parent's auto layout, where it is.
    pub absolute: bool,
    pub kids: Vec<Layer>,
}

impl Layer {
    fn new(kind: Kind, item: &Item) -> Self {
        Layer {
            kind,
            name: item.nm.clone(),
            tag: Some(item.t.clone()),
            rect: item.r,
            style: item.s.clone(),
            layout: None,
            sizing: (Sizing::Fixed, Sizing::Fixed),
            absolute: false,
            kids: Vec::new(),
        }
    }

    /// Every layer of this tree, this one first.
    pub fn count(&self) -> usize {
        1 + self.kids.iter().map(Layer::count).sum::<usize>()
    }

    /// Whether this is a text that fits on one line.
    pub fn one_line(&self) -> bool {
        let line = self.style.lh.unwrap_or(self.style.fs.max(1.0) * 1.25);
        matches!(self.kind, Kind::Text(_)) && self.rect[3] < line * 1.6
    }
}

/// The layers that draw `capture`: one frame the page's size (its full height), filled with the
/// page's background, holding the body's layers.
pub fn plan(capture: &Capture) -> Layer {
    let body = capture.root.clone().unwrap_or_default();
    let mut root = Layer::new(Kind::Frame, &body);
    root.name = None;
    root.rect = [0.0, 0.0, capture.w.max(1.0), capture.h.max(1.0)];
    root.style.bg = capture.bg.clone().or(root.style.bg);
    root.style.bd = None;
    root.style.sh = None;
    root.style.rd = None;
    root.style.op = None;
    root.kids = body.k.iter().flat_map(lower).collect();
    // The body lays out like a block inside the page, whatever its margins.
    root.layout = auto_layout(&root, &body.s);
    size_kids(&mut root, &body.s);
    root
}

/// `item`'s layers: none (it draws nothing), its children's (a wrapper that draws nothing and
/// holds one thing, or inline content), or one layer.
fn lower(item: &Item) -> Vec<Layer> {
    if item.t == "#text" {
        let text = item.tx.clone().unwrap_or_default();
        return match text.trim().is_empty() {
            true => Vec::new(),
            false => vec![Layer::new(Kind::Text(text), item)],
        };
    }
    if let Some(svg) = &item.svg {
        return vec![Layer::new(Kind::Svg(svg.clone()), item)];
    }
    let media = matches!(item.t.as_str(), "img" | "canvas" | "video" | "picture");
    if media {
        let kind = item.img.clone().map_or(Kind::Slot, Kind::Image);
        let mut layer = Layer::new(kind, item);
        layer.absolute = item.s.out_of_flow();
        return vec![layer];
    }
    let kids: Vec<Layer> = item.k.iter().flat_map(lower).collect();
    let draws = item.s.draws() || item.s.op.is_some_and(|o| o < 0.99);
    let container = item
        .s
        .d
        .as_deref()
        .is_some_and(|d| d.contains("flex") || d.contains("grid"));
    let inline = item
        .s
        .d
        .as_deref()
        .is_some_and(|d| d == "inline" || d == "contents");
    if !draws && !item.s.out_of_flow() {
        if kids.is_empty() {
            return Vec::new();
        }
        // A wrapper that draws nothing: its one child stands for it; inline content, all of it.
        if (kids.len() == 1 && !(container && has_padding(&item.s))) || inline {
            return kids;
        }
    }
    let mut layer = Layer::new(Kind::Frame, item);
    layer.absolute = item.s.out_of_flow();
    layer.kids = kids;
    layer.layout = auto_layout(&layer, &item.s);
    size_kids(&mut layer, &item.s);
    vec![layer]
}

fn has_padding(s: &Style) -> bool {
    s.inset().iter().any(|v| *v > 0.5)
}

/// The start and end of `rect` along an axis (vertical: y), and across it.
fn span(rect: &[f64; 4], vertical: bool) -> (f64, f64) {
    match vertical {
        true => (rect[1], rect[1] + rect[3]),
        false => (rect[0], rect[0] + rect[2]),
    }
}

/// The auto layout that places `frame`'s children where the page has them, if one does: a flex
/// row or column (not reversed, on one line), or a block's children stacked top to bottom, at one
/// gap (or spread, `space-between`), all aligned to the start, the centre or the end across it.
pub fn auto_layout(frame: &Layer, s: &Style) -> Option<AutoLayout> {
    let flow: Vec<&Layer> = frame.kids.iter().filter(|k| !k.absolute).collect();
    if flow.is_empty() {
        return None;
    }
    let display = s.d.as_deref().unwrap_or("block");
    let flex = display.contains("flex");
    let vertical = match (flex, s.fd.as_deref()) {
        (true, Some("row")) | (true, None) => false,
        (true, Some("column")) => true,
        (true, _) => return None,
        (false, _) if display.contains("grid") || display.starts_with("inline") => {
            match (stacked(&flow, true), stacked(&flow, false)) {
                (true, _) => true,
                (false, true) => false,
                _ => return None,
            }
        }
        (false, _) => true,
    };
    if !stacked(&flow, vertical) {
        return None;
    }
    let gaps: Vec<f64> = flow
        .windows(2)
        .map(|w| span(&w[1].rect, vertical).0 - span(&w[0].rect, vertical).1)
        .collect();
    let spread = flex
        && flow.len() > 1
        && matches!(
            s.jc.as_deref(),
            Some("space-between" | "space-around" | "space-evenly")
        );
    let even = gaps.iter().all(|g| (g - gaps[0]).abs() <= NEAR);
    if (!even && !spread) || gaps.iter().any(|g| *g < -0.5) {
        return None;
    }
    let gap = gaps.first().copied().unwrap_or(0.0).max(0.0).round();
    let inset = s.inset();
    // Along the axis: CSS's own justify for a flex box, else the first child's offset.
    let (start, end) = span(&frame.rect, vertical);
    let main = match (flex, s.jc.as_deref()) {
        _ if spread => "spaceBetween",
        (true, Some("center")) => "center",
        (true, Some("flex-end" | "end")) => "max",
        _ => "min",
    };
    let (lead, trail) = match vertical {
        true => (0, 2),
        false => (3, 1),
    };
    let mut pad_main = (inset[lead], inset[trail]);
    if main == "min" {
        pad_main.0 = (span(&flow[0].rect, vertical).0 - start).max(0.0);
    }
    // Across it: where every child sits in the frame's inner box.
    let (c_start, c_end) = span(&frame.rect, !vertical);
    let (c_lead, c_trail) = match vertical {
        true => (3, 1),
        false => (0, 2),
    };
    let (inner0, inner1) = (c_start + inset[c_lead], c_end - inset[c_trail]);
    let at = |k: &&Layer| span(&k.rect, !vertical);
    let all = |f: &dyn Fn((f64, f64)) -> bool| flow.iter().map(at).all(f);
    let centred = |(a, b): (f64, f64)| ((a + b) / 2.0 - (inner0 + inner1) / 2.0).abs() <= NEAR;
    let ends = |(_, b): (f64, f64)| (b - inner1).abs() <= NEAR;
    // A flex box says how it aligns; where the children fit either way, its word wins.
    let said = match (flex, s.ai.as_deref()) {
        (true, Some("center")) if all(&centred) => Some("center"),
        (true, Some("flex-end" | "end")) if all(&ends) => Some("max"),
        _ => None,
    };
    let (cross, pad_cross) = if let Some(said) = said {
        (said, (inset[c_lead], inset[c_trail]))
    } else if all(&|(a, _)| (a - inner0).abs() <= NEAR) {
        ("min", (inset[c_lead], inset[c_trail]))
    } else if all(&centred) {
        ("center", (inset[c_lead], inset[c_trail]))
    } else if all(&ends) {
        ("max", (inset[c_lead], inset[c_trail]))
    } else {
        // Every child the same way in from the start (a margin): that is the padding.
        let first = at(&flow[0]).0;
        if !all(&|(a, _)| (a - first).abs() <= NEAR) {
            return None;
        }
        ("min", ((first - c_start).max(0.0), inset[c_trail]))
    };
    let _ = end;
    let mut padding = [0.0; 4];
    padding[lead] = pad_main.0.round();
    padding[trail] = pad_main.1.round();
    padding[c_lead] = pad_cross.0.round();
    padding[c_trail] = pad_cross.1.round();
    Some(AutoLayout {
        vertical,
        gap,
        padding,
        main,
        cross,
    })
}

/// Whether `kids` follow each other along the axis in their order, none overlapping the next.
fn stacked(kids: &[&Layer], vertical: bool) -> bool {
    kids.windows(2)
        .all(|w| span(&w[1].rect, vertical).0 >= span(&w[0].rect, vertical).1 - NEAR)
}

/// How `frame`'s children are sized in its auto layout: across it, one as wide (or tall) as the
/// inner box fills it; along it, a flex item that grows fills it; a one-line text hugs it.
/// Children out of the flow keep their place.
fn size_kids(frame: &mut Layer, s: &Style) {
    let Some(layout) = frame.layout.clone() else {
        return;
    };
    let p = layout.padding;
    let inner = match layout.vertical {
        true => frame.rect[2] - p[1] - p[3],
        false => frame.rect[3] - p[0] - p[2],
    };
    let flex = s.d.as_deref().is_some_and(|d| d.contains("flex"));
    for kid in frame.kids.iter_mut().filter(|k| !k.absolute) {
        let across = match layout.vertical {
            true => kid.rect[2],
            false => kid.rect[3],
        };
        let fills_across = (across - inner).abs() <= NEAR && inner > 0.0;
        let grows = flex && kid.style.grow > 0.0;
        let text = matches!(kid.kind, Kind::Text(_));
        let along = match (grows, text && kid.one_line() && !layout.vertical) {
            (true, _) => Sizing::Fill,
            (false, true) => Sizing::Hug,
            _ => Sizing::Fixed,
        };
        let cross = match (fills_across, text && kid.one_line() && layout.vertical) {
            (true, _) => Sizing::Fill,
            (false, true) => Sizing::Hug,
            _ => Sizing::Fixed,
        };
        kid.sizing = match layout.vertical {
            true => (cross, along),
            false => (along, cross),
        };
        // A text's height follows its lines, whatever the font the canvas draws it in.
        if text {
            kid.sizing.1 = Sizing::Hug;
        }
    }
}
