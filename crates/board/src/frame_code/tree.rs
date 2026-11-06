//! The walk of a frame's layer tree into elements and their CSS ([`build`]): what both the HTML
//! and the React output are made of.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use figcraft_color::{ImageScaleMode, PaintKind};
use figcraft_doc::{Document, LayoutMode, Node, NodeId, NodeKind, Sizing, TextAutoResize};
use figcraft_engine::codegen::{self, CodeLang};
use figcraft_engine::components::variant_props;

use super::layers::{data_url, is_vector, px, slug};
use crate::design::export::{ExportRow, export_bytes};
use crate::doc::{BoardDoc, BoardError};

/// CSS declarations, in order.
pub(super) type Decls = Vec<(String, String)>;

/// An element of the output.
pub(super) enum Item {
    /// A frame, a group, a shape: a box with its children.
    Box {
        class: String,
        name: String,
        children: Vec<Item>,
    },
    /// A text layer; in a component, `key` is the text's name among its props.
    Text {
        class: String,
        name: String,
        text: String,
        key: Option<String>,
    },
    /// A vector layer or an icon, as an SVG document.
    Svg {
        class: String,
        name: String,
        svg: String,
    },
    /// An instance, in React: a call to component `component` with its props.
    Call {
        component: usize,
        class: Option<String>,
        name: String,
        props: Vec<(String, String)>,
        texts: Vec<(String, String)>,
    },
}

/// A component the React output declares: its name, its variant props, the variants the frame
/// uses (the default first) and the names of the texts in them.
pub(super) struct Component {
    pub name: String,
    pub props: Vec<Prop>,
    pub variants: Vec<(BTreeMap<String, String>, Item)>,
    pub texts: Vec<String>,
}

/// A variant property: its name in the code (`size`), on the canvas (`Size`), and its values.
pub(super) struct Prop {
    pub ident: String,
    pub label: String,
    pub values: Vec<String>,
    pub default: String,
}

/// A frame's tree, walked.
pub(super) struct Built {
    pub title: String,
    pub root: Item,
    /// Each class's declarations, in the order their elements come.
    pub rules: Vec<(String, Decls)>,
    /// The classes that only place an instance in its parent: after the rest, so they win.
    pub placements: Vec<(String, Decls)>,
    /// Custom properties for the variables bound to layers: name and value.
    pub tokens: Vec<(String, String)>,
    pub components: Vec<Component>,
    pub notes: Vec<String>,
}

/// Walks frame `frame` of `board`; `react` makes instances calls to components.
pub(super) fn build(board: &mut BoardDoc, frame: NodeId, react: bool) -> Result<Built, BoardError> {
    let doc = board.doc()?;
    let node = doc
        .node(frame)
        .ok_or_else(|| BoardError::Engine(format!("no layer {frame}")))?;
    if !node.kind.is_frame_like() && !matches!(node.kind, NodeKind::Group) {
        return Err(BoardError::Engine(format!("“{}” isn't a frame", node.name)));
    }
    let title = node.name.clone();
    let mut walk = Walk {
        board,
        doc: doc.clone(),
        react,
        used: Vec::new(),
        rules: Vec::new(),
        placements: Vec::new(),
        tokens: Vec::new(),
        components: Vec::new(),
        by_key: HashMap::new(),
        built: HashSet::new(),
        pending: Vec::new(),
        notes: Vec::new(),
        in_component: false,
    };
    let root = walk.node(frame, None, true).unwrap_or(Item::Box {
        class: "frame".into(),
        name: title.clone(),
        children: Vec::new(),
    });
    walk.in_component = true;
    // First in, first built: a component's default variant comes first.
    while !walk.pending.is_empty() {
        let (component, variant) = walk.pending.remove(0);
        if let Some(item) = walk.node(variant, None, true) {
            let pairs = walk
                .doc
                .node(variant)
                .map(|n| variant_props(&n.name))
                .unwrap_or_default();
            walk.components[component].variants.push((pairs, item));
        }
    }
    Ok(Built {
        title,
        root,
        rules: walk.rules,
        placements: walk.placements,
        tokens: walk.tokens,
        components: walk.components,
        notes: walk.notes,
    })
}

pub(super) struct Walk<'a> {
    pub(super) board: &'a mut BoardDoc,
    pub(super) doc: Arc<Document>,
    pub(super) react: bool,
    pub(super) used: Vec<String>,
    pub(super) rules: Vec<(String, Decls)>,
    pub(super) placements: Vec<(String, Decls)>,
    pub(super) tokens: Vec<(String, String)>,
    pub(super) components: Vec<Component>,
    /// Each component by its set (or itself, alone).
    pub(super) by_key: HashMap<NodeId, usize>,
    /// The variants built or waiting to be.
    pub(super) built: HashSet<NodeId>,
    pub(super) pending: Vec<(usize, NodeId)>,
    pub(super) notes: Vec<String>,
    /// Building a component's variant: its texts are props.
    pub(super) in_component: bool,
}

impl Walk<'_> {
    fn node(&mut self, id: NodeId, parent: Option<&Node>, root: bool) -> Option<Item> {
        let doc = self.doc.clone();
        let n = doc.node(id)?;
        if !n.visible {
            return None;
        }
        let name = n.name.clone();
        let class = self.class(&name, n.kind.label());
        let placed = self.placement(n, parent, root);
        if self.react
            && !root
            && let NodeKind::Instance { component, .. } = &n.kind
            && let Some(call) = self.call(n, *component, &placed, &name)
        {
            return Some(call);
        }
        let mut decls = placed;
        decls.extend(self.own(n, parent, root));
        if is_vector(&doc, n) {
            return Some(match self.svg(n) {
                Some(svg) => {
                    self.rules.push((class.clone(), decls));
                    Item::Svg { class, name, svg }
                }
                None => {
                    self.rules.push((class.clone(), decls));
                    Item::Box {
                        class,
                        name,
                        children: Vec::new(),
                    }
                }
            });
        }
        if let NodeKind::Text(text) = &n.kind {
            self.rules.push((class.clone(), decls));
            let key = self.in_component.then(|| name.clone());
            return Some(Item::Text {
                class,
                name,
                text: text.characters.clone(),
                key,
            });
        }
        self.rules.push((class.clone(), decls));
        let kids = n.children.clone();
        let children = kids
            .iter()
            .filter_map(|k| self.node(*k, Some(n), false))
            .collect();
        Some(Item::Box {
            class,
            name,
            children,
        })
    }

    /// A class for layer `name` no other element has: its name's words (`Card / Title` →
    /// `card-title`), else its kind's.
    pub(super) fn class(&mut self, name: &str, kind: &str) -> String {
        let mut base = slug(name);
        if base.is_empty() {
            base = slug(kind);
        }
        if base.starts_with(|c: char| c.is_ascii_digit()) {
            base = format!("layer-{base}");
        }
        let mut class = base.clone();
        let mut n = 2;
        while self.used.contains(&class) {
            class = format!("{base}-{n}");
            n += 1;
        }
        self.used.push(class.clone());
        class
    }

    /// How `n` sits in `parent`: in its flex flow, or placed at its place in it.
    fn placement(&self, n: &Node, parent: Option<&Node>, root: bool) -> Decls {
        let mut out = Decls::new();
        let put = |out: &mut Decls, k: &str, v: String| out.push((k.to_string(), v));
        if root {
            put(&mut out, "position", "relative".into());
            return out;
        }
        let Some(parent) = parent else {
            return out;
        };
        match parent.layout.as_ref().filter(|_| !n.child_layout.absolute) {
            Some(layout) => {
                let vertical = layout.mode == LayoutMode::Vertical;
                let (main, cross) = match vertical {
                    true => (n.child_layout.sizing_v, n.child_layout.sizing_h),
                    false => (n.child_layout.sizing_h, n.child_layout.sizing_v),
                };
                if main == Sizing::Fill {
                    put(&mut out, "flex", "1 1 0".into());
                    put(
                        &mut out,
                        if vertical { "min-height" } else { "min-width" },
                        "0".into(),
                    );
                }
                if cross == Sizing::Fill {
                    put(&mut out, "align-self", "stretch".into());
                }
                if n.children.is_empty() || n.layout.is_none() {
                    put(&mut out, "flex-shrink", "0".into());
                }
            }
            None => {
                let (own, box_) = (
                    self.doc.world_bounds(n.id),
                    self.doc.world_bounds(parent.id),
                );
                let (x, y) = match (own, box_) {
                    (Some(own), Some(box_)) => (own.x0 - box_.x0, own.y0 - box_.y0),
                    _ => (0.0, 0.0),
                };
                put(&mut out, "position", "absolute".into());
                put(&mut out, "left", px(x));
                put(&mut out, "top", px(y));
            }
        }
        out
    }

    /// `n`'s own look and size: Copy as CSS's declarations, without the sizes it hugs or fills,
    /// with its images embedded and its variables noted.
    fn own(&mut self, n: &Node, parent: Option<&Node>, root: bool) -> Decls {
        let css = codegen::code_for(&self.doc, n.id, CodeLang::Css);
        let body = css
            .split_once("*/\n")
            .map_or(css.as_str(), |(_, rest)| rest);
        let mut out: Decls = body
            .lines()
            .filter_map(|line| {
                let (k, v) = line.split_once(':')?;
                Some((
                    k.trim().to_string(),
                    v.trim().trim_end_matches(';').trim().to_string(),
                ))
            })
            .collect();
        let (mut drop_w, mut drop_h) = match &n.kind {
            NodeKind::Text(t) => match t.auto_resize {
                TextAutoResize::WidthAndHeight => (true, true),
                TextAutoResize::Height => (false, true),
                _ => (false, false),
            },
            _ if n.layout.is_some() => (
                n.child_layout.sizing_h == Sizing::Hug,
                n.child_layout.sizing_v == Sizing::Hug,
            ),
            _ => (false, false),
        };
        let in_flow = parent.is_some_and(|p| p.layout.is_some()) && !n.child_layout.absolute;
        if in_flow {
            let fill_w = n.child_layout.sizing_h == Sizing::Fill;
            let fill_h = n.child_layout.sizing_v == Sizing::Fill;
            drop_w |= fill_w;
            drop_h |= fill_h;
        }
        if root {
            (drop_w, drop_h) = (false, false);
        }
        out.retain(|(k, _)| !(drop_w && k == "width" || drop_h && k == "height"));
        for (k, v) in &mut out {
            if k == "font-family" {
                v.push_str(", system-ui, sans-serif");
            }
            if v.contains("url(image.png)") {
                *v = self.image(n, v);
            }
            self.note_tokens(v);
        }
        match &n.kind {
            NodeKind::Text(t) => {
                out.push(("margin".into(), "0".into()));
                match t.auto_resize {
                    TextAutoResize::WidthAndHeight => {
                        out.push(("white-space".into(), "pre".into()));
                        // Auto line height is the font's own on the canvas, and `normal` the
                        // browser's: a box that hugs its lines gives the canvas's.
                        if !out.iter().any(|(k, _)| k == "line-height") {
                            let lines = t.characters.split('\n').count().max(1) as f64;
                            out.push(("line-height".into(), px(n.height / lines)));
                        }
                    }
                    TextAutoResize::Truncate => {
                        out.push(("overflow".into(), "hidden".into()));
                        out.push(("text-overflow".into(), "ellipsis".into()));
                        out.push(("white-space".into(), "nowrap".into()));
                    }
                    _ if t.characters.contains('\n') => {
                        out.push(("white-space".into(), "pre-wrap".into()));
                    }
                    _ => {}
                }
            }
            _ if n.clip && !n.children.is_empty() => out.push(("overflow".into(), "hidden".into())),
            _ => {}
        }
        out
    }

    /// `value` (a `background` with `url(image.png)`) with `n`'s image in it as a `data:` URL.
    fn image(&mut self, n: &Node, value: &str) -> String {
        let paint = n.fills.iter().rev().find_map(|p| match &p.kind {
            PaintKind::Image(image) if p.visible => Some(image),
            _ => None,
        });
        let Some(paint) = paint else {
            return value.to_string();
        };
        let Some(data) = self.doc.images.get(&paint.image) else {
            self.notes
                .push(format!("“{}”: its image isn't in the file", n.name));
            return value.to_string();
        };
        let url = format!("url(\"{}\")", data_url(&data.bytes));
        let value = value.replace("url(image.png)", &url);
        match paint.scale_mode {
            ImageScaleMode::Fit => value.replace("cover", "contain"),
            ImageScaleMode::Tile => value.replace("center / cover", "repeat"),
            _ => value,
        }
    }

    /// Notes the custom properties `value` uses (`var(--name, value)`), for `:root`.
    fn note_tokens(&mut self, value: &str) {
        let mut rest = value;
        while let Some(at) = rest.find("var(--") {
            let inner = &rest[at + 4..];
            let Some(comma) = inner.find(',') else {
                break;
            };
            let name = inner[..comma].trim().to_string();
            let mut depth = 1;
            let mut end = inner.len();
            for (i, c) in inner[comma + 1..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = comma + 1 + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let fallback = inner[comma + 1..end].trim().to_string();
            if !self.tokens.iter().any(|(n, _)| *n == name) {
                self.tokens.push((name, fallback));
            }
            rest = &inner[end.min(inner.len())..];
        }
    }

    /// `n` as an SVG document, exported by the engine.
    fn svg(&mut self, n: &Node) -> Option<String> {
        let row = ExportRow {
            format: "svg".to_string(),
            scale: 1.0,
        };
        match export_bytes(self.board, &[n.id], &row) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).ok()?;
                let at = text.find("<svg")?;
                Some(text[at..].trim().to_string())
            }
            Err(error) => {
                self.notes.push(format!(
                    "“{}” couldn't be drawn as SVG: {}",
                    n.name,
                    error.for_user()
                ));
                None
            }
        }
    }
}
