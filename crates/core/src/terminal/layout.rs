//! Split layout of one terminal tab: a binary tree of panes. Pure geometry,
//! no UI, so the splitting rules can be tested headlessly.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Identifies one pane. Random, so ids do not clash across processes.
pub type PaneId = u64;

/// Makes a fresh pane id.
pub fn new_pane_id() -> PaneId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.finish()
}

/// Where the new pane goes when a pane is split.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDir {
    /// Side by side: the new pane is on the right.
    Right,
    /// Stacked: the new pane is below.
    Down,
}

/// A direction to move focus in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Towards smaller x.
    Left,
    /// Towards larger x.
    Right,
    /// Towards smaller y.
    Up,
    /// Towards larger y.
    Down,
}

/// A rectangle in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width, never negative.
    pub w: f32,
    /// Height, never negative.
    pub h: f32,
}

/// The split tree. Leaves are panes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Layout {
    /// One pane.
    Pane(PaneId),
    /// Two subtrees sharing an area.
    Split {
        /// How the area is divided.
        dir: SplitDir,
        /// Share of the area given to `first`, between 0 and 1.
        ratio: f32,
        /// Left or top subtree.
        first: Box<Layout>,
        /// Right or bottom subtree.
        second: Box<Layout>,
    },
}

/// Route from the root to a split: `false` is the first child, `true` the second.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitPath(pub Vec<bool>);

/// The draggable boundary of one split.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    /// Which split this is.
    pub path: SplitPath,
    /// Direction of the split.
    pub dir: SplitDir,
    /// The x of a `Right` split's boundary, or the y of a `Down` split's.
    pub at: f32,
    /// The split's whole area.
    pub span: Rect,
}

/// What happened to the layout when a pane was removed.
#[derive(Debug, PartialEq, Eq)]
pub enum Removed {
    /// The pane is not in the layout.
    NotFound,
    /// It was the only pane. The layout is unchanged.
    Last,
    /// Removed. Focus this pane next.
    Focus(PaneId),
}

/// Smallest gap treated as a real overlap or a real side.
const EPS: f32 = 0.5;

fn safe_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Splits `area` into the two child areas of a split.
fn divide(dir: SplitDir, ratio: f32, area: Rect) -> (Rect, Rect) {
    let ratio = safe_ratio(ratio);
    let (w, h) = (area.w.max(0.0), area.h.max(0.0));
    match dir {
        SplitDir::Right => {
            let first = (w * ratio).max(0.0);
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w: first,
                    h,
                },
                Rect {
                    x: area.x + first,
                    y: area.y,
                    w: (w - first).max(0.0),
                    h,
                },
            )
        }
        SplitDir::Down => {
            let first = (h * ratio).max(0.0);
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w,
                    h: first,
                },
                Rect {
                    x: area.x,
                    y: area.y + first,
                    w,
                    h: (h - first).max(0.0),
                },
            )
        }
    }
}

impl Layout {
    /// Every pane, in reading order (first before second).
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_panes(&mut out);
        out
    }

    fn collect_panes(&self, out: &mut Vec<PaneId>) {
        match self {
            Layout::Pane(id) => out.push(*id),
            Layout::Split { first, second, .. } => {
                first.collect_panes(out);
                second.collect_panes(out);
            }
        }
    }

    fn first_pane(&self) -> PaneId {
        match self {
            Layout::Pane(id) => *id,
            Layout::Split { first, .. } => first.first_pane(),
        }
    }

    /// Splits `target` in two, with `new` after it. False if `target` is absent.
    pub fn split(&mut self, target: PaneId, dir: SplitDir, new: PaneId) -> bool {
        match self {
            Layout::Pane(id) if *id == target => {
                *self = Layout::Split {
                    dir,
                    ratio: 0.5,
                    first: Box::new(Layout::Pane(target)),
                    second: Box::new(Layout::Pane(new)),
                };
                true
            }
            Layout::Pane(_) => false,
            Layout::Split { first, second, .. } => {
                first.split(target, dir, new) || second.split(target, dir, new)
            }
        }
    }

    /// Removes `target`. Its sibling subtree takes the split's place.
    pub fn remove(&mut self, target: PaneId) -> Removed {
        match self {
            Layout::Pane(id) if *id == target => Removed::Last,
            Layout::Pane(_) => Removed::NotFound,
            Layout::Split { first, second, .. } => {
                if matches!(**first, Layout::Pane(id) if id == target) {
                    let sibling = std::mem::replace(&mut **second, Layout::Pane(0));
                    let focus = sibling.first_pane();
                    *self = sibling;
                    return Removed::Focus(focus);
                }
                if matches!(**second, Layout::Pane(id) if id == target) {
                    let sibling = std::mem::replace(&mut **first, Layout::Pane(0));
                    let focus = sibling.first_pane();
                    *self = sibling;
                    return Removed::Focus(focus);
                }
                match first.remove(target) {
                    Removed::NotFound => second.remove(target),
                    found => found,
                }
            }
        }
    }

    /// The area of every pane inside `area`. Sizes are never negative or NaN.
    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.collect_rects(area, &mut out);
        out
    }

    fn collect_rects(&self, area: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Layout::Pane(id) => out.push((
                *id,
                Rect {
                    w: area.w.max(0.0),
                    h: area.h.max(0.0),
                    ..area
                },
            )),
            Layout::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let (a, b) = divide(*dir, *ratio, area);
                first.collect_rects(a, out);
                second.collect_rects(b, out);
            }
        }
    }

    /// The boundary of every split inside `area`, parents before children.
    pub fn dividers(&self, area: Rect) -> Vec<Divider> {
        let mut out = Vec::new();
        self.collect_dividers(area, &mut Vec::new(), &mut out);
        out
    }

    fn collect_dividers(&self, area: Rect, path: &mut Vec<bool>, out: &mut Vec<Divider>) {
        if let Layout::Split {
            dir,
            ratio,
            first,
            second,
        } = self
        {
            let (a, b) = divide(*dir, *ratio, area);
            let at = match dir {
                SplitDir::Right => b.x,
                SplitDir::Down => b.y,
            };
            out.push(Divider {
                path: SplitPath(path.clone()),
                dir: *dir,
                at,
                span: area,
            });
            path.push(false);
            first.collect_dividers(a, path, out);
            path.pop();
            path.push(true);
            second.collect_dividers(b, path, out);
            path.pop();
        }
    }

    /// The pane next to `from` in `dir`. Among panes fully on that side that
    /// overlap `from` on the other axis, the closest edge wins; ties go to the
    /// one nearest `from`'s centre line.
    pub fn neighbor(&self, from: PaneId, dir: Direction, area: Rect) -> Option<PaneId> {
        let rects = self.rects(area);
        let me = rects.iter().find(|(id, _)| *id == from)?.1;
        let horizontal = matches!(dir, Direction::Left | Direction::Right);
        // Project a rect onto (along, across): `along` is the axis of travel.
        let axes = |r: Rect| {
            if horizontal {
                ((r.x, r.x + r.w), (r.y, r.y + r.h))
            } else {
                ((r.y, r.y + r.h), (r.x, r.x + r.w))
            }
        };
        let forward = matches!(dir, Direction::Right | Direction::Down);
        let (my_along, my_across) = axes(me);
        let centre = (my_across.0 + my_across.1) / 2.0;
        let mut best: Option<(PaneId, f32, f32, f32)> = None;
        for (id, r) in &rects {
            if *id == from {
                continue;
            }
            let (along, across) = axes(*r);
            let gap = if forward {
                along.0 - my_along.1
            } else {
                my_along.0 - along.1
            };
            if gap < -EPS {
                continue;
            }
            if across.1.min(my_across.1) - across.0.max(my_across.0) <= EPS {
                continue;
            }
            let off_centre = if centre < across.0 {
                across.0 - centre
            } else if centre > across.1 {
                centre - across.1
            } else {
                0.0
            };
            let off_middle = (centre - (across.0 + across.1) / 2.0).abs();
            let key = (gap.max(0.0), off_centre, off_middle);
            let better = match best {
                None => true,
                Some((_, g, o, m)) => (key.0, key.1, key.2) < (g, o, m),
            };
            if better {
                best = Some((*id, key.0, key.1, key.2));
            }
        }
        best.map(|(id, ..)| id)
    }

    fn node_mut(&mut self, path: &SplitPath) -> Option<&mut Layout> {
        let mut node = self;
        for step in &path.0 {
            match node {
                Layout::Split { first, second, .. } => {
                    node = if *step { second } else { first };
                }
                Layout::Pane(_) => return None,
            }
        }
        Some(node)
    }

    /// The smallest (width, height) this subtree fits in when every pane
    /// gets at least `pane_min`: side by side the widths add up, stacked
    /// the heights do; across the split the larger one counts.
    pub fn min_size(&self, pane_min: (f32, f32)) -> (f32, f32) {
        match self {
            Layout::Pane(_) => pane_min,
            Layout::Split {
                dir, first, second, ..
            } => {
                let (a, b) = (first.min_size(pane_min), second.min_size(pane_min));
                match dir {
                    SplitDir::Right => (a.0 + b.0, a.1.max(b.1)),
                    SplitDir::Down => (a.0.max(b.0), a.1 + b.1),
                }
            }
        }
    }

    /// Sets the split's ratio, keeping each side at least as long as its
    /// panes need (`min_size` along the split) out of `span_len`. If the
    /// span is too short for both sides, the ratio becomes 0.5.
    pub fn set_ratio(&mut self, path: &SplitPath, ratio: f32, span_len: f32, pane_min: (f32, f32)) {
        let Some(Layout::Split {
            dir,
            ratio: slot,
            first,
            second,
        }) = self.node_mut(path)
        else {
            return;
        };
        let along = |m: (f32, f32)| match dir {
            SplitDir::Right => m.0,
            SplitDir::Down => m.1,
        };
        let lo = along(first.min_size(pane_min));
        let hi = along(second.min_size(pane_min));
        *slot = if span_len.is_finite() && span_len > 0.0 && span_len >= lo + hi {
            safe_ratio(ratio).clamp(lo / span_len, 1.0 - hi / span_len)
        } else {
            0.5
        };
    }

    /// Evens the split's two sides, as far as their panes' minimums allow
    /// (see `set_ratio`).
    pub fn even(&mut self, path: &SplitPath, span_len: f32, pane_min: (f32, f32)) {
        self.set_ratio(path, 0.5, span_len, pane_min);
    }
}
