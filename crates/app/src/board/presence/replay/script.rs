//! An op's diff as a script of element tasks ([`script`]): what was added, changed and taken off
//! a page, in the order a designer would make it. A parent comes before its children, a master
//! before its duplicates, and siblings in reading order (rows top to bottom, each left to right).
//! Each task has its kind ([`Kind`], what animation plays) and how long it takes at 1×; the op as
//! a whole fits a budget ([`OP_BUDGET`]).

use std::time::Duration;

use figcraft_doc::color::PaintKind;
use figcraft_doc::{BoardItem, Document, Node, NodeId, NodeKind};

/// What an element task plays (30d).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) enum Kind {
    /// A frame, an artboard, a screen's frame: a crosshair drag corner to corner.
    Frame,
    /// A rectangle, an ellipse, a polygon: a crosshair drag, then its look.
    Shape,
    /// Text typed, `chars` long; `retype` when it replaced other text.
    Text { chars: usize, retype: bool },
    /// An image: a placeholder, then the picture wiped in top to bottom.
    Image,
    /// A line, an arrow, a connector, a vector: the pen.
    Line,
    /// A copy of an earlier sibling `from` (page rect), ⌥-dragged into place.
    Duplicate { from: [f64; 4] },
    /// A sticky placed with a bounce.
    Sticky,
    /// A component or an instance landing, with its outline and badge.
    Component { instance: bool },
    /// A layer moved, resized or restyled from `from` (page rect).
    Update { from: [f64; 4] },
    /// A layer taken off, from `rect`.
    Delete,
    /// Under reduced motion: the element fades in, nothing more.
    Fade,
}

/// One element's task: the layer, what plays, its page rect, how long it takes at 1×, and for
/// an auto layout frame its padding and gap (shown as spacing marks when it lands).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Task {
    pub node: NodeId,
    pub kind: Kind,
    pub rect: [f64; 4],
    pub time: Duration,
    pub spacing: Option<([f64; 4], f64)>,
    /// A screen's element ([`super::blueprint`]): `node` is the screen's frame, which the drawn
    /// copy leaves alone; a cover over the element's box hides the live page until it plays.
    pub cover: bool,
}

impl Task {
    pub fn new(node: NodeId, kind: Kind, rect: [f64; 4], spacing: Option<([f64; 4], f64)>) -> Self {
        let time = kind.time();
        Task {
            node,
            kind,
            rect,
            time,
            spacing,
            cover: false,
        }
    }
}

/// The most one op plays for, at 1×: past it every task is shortened to fit.
pub(in crate::board) const OP_BUDGET: Duration = Duration::from_millis(2500);
/// The most one element plays for, at 1×.
pub(in crate::board) const ELEMENT_BUDGET: Duration = Duration::from_millis(900);
/// How long a typed character takes.
const PER_CHAR: Duration = Duration::from_millis(28);
/// How many tasks an op plays; past it (a large diagram, a pasted page) the rest just show.
const MAX_TASKS: usize = 80;

impl Kind {
    /// How long it plays at 1×.
    fn time(&self) -> Duration {
        let ms = match self {
            Kind::Frame => 420,
            Kind::Shape => 280,
            Kind::Text { chars, .. } => {
                (*chars as u64 * PER_CHAR.as_millis() as u64).clamp(150, 900)
            }
            Kind::Image => 380,
            Kind::Line => 320,
            Kind::Duplicate { .. } => 340,
            Kind::Sticky => 300,
            Kind::Component { .. } => 300,
            Kind::Update { .. } => 300,
            Kind::Delete => 220,
            Kind::Fade => 150,
        };
        Duration::from_millis(ms).min(ELEMENT_BUDGET)
    }
}

/// The page rect of `id` in `doc`.
fn rect(doc: &Document, id: NodeId) -> Option<[f64; 4]> {
    let b = doc.world_bounds(id)?;
    Some([b.x0, b.y0, b.x1, b.y1])
}

/// The task kind an added layer `n` plays.
fn added_kind(n: &Node) -> Kind {
    if matches!(n.board, Some(BoardItem::Sticky { .. })) {
        return Kind::Sticky;
    }
    let image = n
        .fills
        .iter()
        .any(|p| p.visible && matches!(p.kind, PaintKind::Image(_)));
    match &n.kind {
        NodeKind::Text(t) => Kind::Text {
            chars: t.characters.chars().count(),
            retype: false,
        },
        _ if image => Kind::Image,
        NodeKind::Component { .. } | NodeKind::ComponentSet { .. } => {
            Kind::Component { instance: false }
        }
        NodeKind::Instance { .. } => Kind::Component { instance: true },
        NodeKind::Frame | NodeKind::Section { .. } | NodeKind::Group => Kind::Frame,
        NodeKind::Vector { .. } | NodeKind::Line | NodeKind::BooleanOp { .. } => Kind::Line,
        _ => Kind::Shape,
    }
}

/// Whether `a` and `b` are the same thing made twice: the same kind, size and number of children.
fn same_shape(doc: &Document, a: NodeId, b: NodeId) -> bool {
    let (Some(x), Some(y)) = (doc.node(a), doc.node(b)) else {
        return false;
    };
    let near = |p: f64, q: f64| (p - q).abs() < 0.5;
    std::mem::discriminant(&x.kind) == std::mem::discriminant(&y.kind)
        && near(x.width, y.width)
        && near(x.height, y.height)
        && x.children.len() == y.children.len()
        && (!x.children.is_empty() || x.name == y.name)
}

/// The children of `id` in reading order: rows top to bottom (within 24 units counts as one row),
/// each left to right.
fn reading_order(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let mut kids: Vec<(i64, f64, NodeId)> = (doc.node(id).map(|n| n.children.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let r = rect(doc, c)?;
            Some(((r[1] / 24.0).round() as i64, r[0], c))
        })
        .collect();
    kids.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kids.into_iter().map(|(.., c)| c).collect()
}

/// The script of an op that changed `before` into `after`, on page `page`.
pub(in crate::board) fn script(before: &Document, after: &Document, page: NodeId) -> Vec<Task> {
    let mut tasks = Vec::new();
    // What was taken off: the topmost of it, first.
    let gone: Vec<NodeId> = before
        .subtree(page)
        .into_iter()
        .filter(|id| *id != page && !after.contains(*id))
        .collect();
    for id in &gone {
        let parent_gone = before
            .node(*id)
            .and_then(|n| n.parent)
            .is_some_and(|p| gone.contains(&p));
        if let (false, Some(r)) = (parent_gone, rect(before, *id)) {
            tasks.push(task(*id, Kind::Delete, r, None));
        }
    }
    walk(before, after, page, false, &mut tasks);
    tasks.truncate(MAX_TASKS);
    fit(&mut tasks);
    tasks
}

/// The tasks of `id`'s children in `after`, in order; `in_copy` when they are inside a
/// duplicate, which brings its shapes with it and only retypes its text.
fn walk(before: &Document, after: &Document, id: NodeId, in_copy: bool, tasks: &mut Vec<Task>) {
    let kids = reading_order(after, id);
    let mut made: Vec<NodeId> = Vec::new();
    for kid in kids {
        let Some(n) = after.node(kid) else { continue };
        if !n.visible {
            continue;
        }
        let Some(r) = rect(after, kid) else { continue };
        let old = before.node(kid);
        let is_new = old.is_none();
        let mut copy = in_copy;
        if is_new && !in_copy {
            let master = made.iter().find(|m| same_shape(after, **m, kid)).copied();
            let kind = match master.and_then(|m| rect(after, m)) {
                Some(from) => {
                    copy = true;
                    Kind::Duplicate { from }
                }
                None => added_kind(n),
            };
            let spacing = n.layout.as_ref().map(|l| (l.padding, l.gap));
            tasks.push(task(kid, kind, r, spacing));
            made.push(kid);
        } else if is_new && in_copy {
            if let NodeKind::Text(t) = &n.kind {
                let kind = Kind::Text {
                    chars: t.characters.chars().count(),
                    retype: true,
                };
                tasks.push(task(kid, kind, r, None));
            }
        } else if let Some(old) = old.filter(|old| !std::ptr::eq(*old, n)) {
            let changed_text = match (&old.kind, &n.kind) {
                (NodeKind::Text(a), NodeKind::Text(b)) if a.characters != b.characters => {
                    Some(b.characters.chars().count())
                }
                _ => None,
            };
            match (changed_text, rect(before, kid)) {
                (Some(chars), _) => tasks.push(task(
                    kid,
                    Kind::Text {
                        chars,
                        retype: true,
                    },
                    r,
                    None,
                )),
                (None, Some(from))
                    if from != r || old.fills != n.fills || old.strokes != n.strokes =>
                {
                    tasks.push(task(kid, Kind::Update { from }, r, None));
                }
                _ => {}
            }
        }
        walk(before, after, kid, copy, tasks);
    }
}

fn task(node: NodeId, kind: Kind, rect: [f64; 4], spacing: Option<([f64; 4], f64)>) -> Task {
    Task::new(node, kind, rect, spacing)
}

/// Shortens every task alike so the op plays within [`OP_BUDGET`].
pub(in crate::board) fn fit(tasks: &mut [Task]) {
    let total: Duration = tasks.iter().map(|t| t.time).sum();
    if total <= OP_BUDGET || total.is_zero() {
        return;
    }
    let scale = OP_BUDGET.as_secs_f64() / total.as_secs_f64();
    for task in tasks {
        task.time = task.time.mul_f64(scale);
    }
}
