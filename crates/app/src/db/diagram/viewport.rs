//! The viewport: world and screen coordinates, zooming and fitting, flying to a table or a
//! search match, panning and dragging tables with the mouse, and the minimap's frame.

use gpui_kit::*;
use sawit_core::db::diagram;

use super::{CARD_PAD, DiagramView, Drag, HEADER_H, MAX_ZOOM, MIN_ZOOM, MINIMAP, ROW_H};

impl DiagramView {
    // Viewport.

    pub(super) fn to_screen(&self, p: Point<f32>) -> Point<Pixels> {
        let origin = self.viewport.get().origin;
        point(
            origin.x + self.offset.x + px(p.x * self.zoom),
            origin.y + self.offset.y + px(p.y * self.zoom),
        )
    }

    pub(super) fn to_world(&self, p: Point<Pixels>) -> Point<f32> {
        let origin = self.viewport.get().origin;
        point(
            f32::from(p.x - origin.x - self.offset.x) / self.zoom,
            f32::from(p.y - origin.y - self.offset.y) / self.zoom,
        )
    }

    /// Zooms to `zoom`, keeping the world point under `anchor` (window
    /// coordinates) where it is.
    pub(super) fn zoom_at(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let world = self.to_world(anchor);
        self.zoom = zoom;
        let origin = self.viewport.get().origin;
        self.offset = point(
            anchor.x - origin.x - px(world.x * zoom),
            anchor.y - origin.y - px(world.y * zoom),
        );
        cx.notify();
    }

    pub(super) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = self.viewport.get().center();
        self.zoom_at(self.zoom * factor, center, cx);
    }

    /// The world rectangle every table fits in.
    fn world_bounds(&self) -> Option<Bounds<f32>> {
        let mut it = self.pos.iter().zip(&self.sizes);
        let (p, s) = it.next()?;
        let (mut min, mut max) = (*p, point(p.x + s.width, p.y + s.height));
        for (p, s) in it {
            min = point(min.x.min(p.x), min.y.min(p.y));
            max = point(max.x.max(p.x + s.width), max.y.max(p.y + s.height));
        }
        Some(Bounds::from_corners(min, max))
    }

    /// Centers the world point `p` at `zoom`.
    fn center_on(&mut self, p: Point<f32>, zoom: f32) {
        let view = self.viewport.get().size;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = point(
            view.width / 2. - px(p.x * self.zoom),
            view.height / 2. - px(p.y * self.zoom),
        );
    }

    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        let view = self.viewport.get().size;
        if let Some(b) = self.world_bounds()
            && view.width > px(0.)
        {
            let pad = 48.;
            let zoom = ((f32::from(view.width) - pad * 2.).max(1.) / b.size.width)
                .min((f32::from(view.height) - pad * 2.).max(1.) / b.size.height)
                .min(1.);
            self.center_on(b.center(), zoom);
            self.needs_fit = false;
        }
        cx.notify();
    }

    /// Flies to a table (and a column row in it).
    fn focus(&mut self, table: usize, column: Option<usize>, cx: &mut Context<Self>) {
        let (p, s) = (self.pos[table], self.sizes[table]);
        let y = match column {
            Some(c) => p.y + HEADER_H + CARD_PAD / 2. + ROW_H * (c as f32 + 0.5),
            None => p.y + s.height / 2.,
        };
        let zoom = if self.zoom < 0.8 { 1. } else { self.zoom };
        self.center_on(point(p.x + s.width / 2., y), zoom);
        self.selected = Some(table);
        cx.notify();
    }

    // Search.

    pub(super) fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value();
        self.hits = diagram::search(&self.erd, &query);
        self.sorted_hits = self.hits.clone();
        self.sorted_hits.sort_unstable();
        self.hit = 0;
        if let Some(&(t, c)) = self.hits.first() {
            self.focus(t, c, cx);
        } else {
            cx.notify();
        }
    }

    /// `hit` (a table, or a column of one) is one of the search's matches.
    pub(super) fn is_hit(&self, hit: (usize, Option<usize>)) -> bool {
        self.sorted_hits.binary_search(&hit).is_ok()
    }

    /// The search matches `table`'s name or one of its columns.
    pub(super) fn has_hit(&self, table: usize) -> bool {
        let at = self.sorted_hits.partition_point(|h| h.0 < table);
        self.sorted_hits.get(at).is_some_and(|h| h.0 == table)
    }

    pub(super) fn step_hit(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() {
            return;
        }
        let n = self.hits.len() as isize;
        self.hit = ((self.hit as isize + step).rem_euclid(n)) as usize;
        let (t, c) = self.hits[self.hit];
        self.focus(t, c, cx);
    }

    // Mouse.

    pub(super) fn on_scroll(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(20.));
        if e.modifiers.platform || e.modifiers.control {
            let factor = (-f32::from(delta.y) * 0.01).exp();
            self.zoom_at(self.zoom * factor, e.position, cx);
        } else {
            self.offset += delta;
            cx.notify();
        }
    }

    fn table_at(&self, p: Point<Pixels>) -> Option<usize> {
        let w = self.to_world(p);
        (0..self.pos.len()).rev().find(|&i| {
            let (o, s) = (self.pos[i], self.sizes[i]);
            w.x >= o.x && w.x <= o.x + s.width && w.y >= o.y && w.y <= o.y + s.height
        })
    }

    pub(super) fn on_mouse_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        if self
            .minimap_bounds()
            .is_some_and(|b| b.contains(&e.position))
        {
            self.drag = Some(Drag::Minimap);
            self.minimap_jump(e.position, cx);
            return;
        }
        self.drag = Some(match self.table_at(e.position) {
            Some(ix) => {
                let w = self.to_world(e.position);
                Drag::Table {
                    ix,
                    grab: point(w.x - self.pos[ix].x, w.y - self.pos[ix].y),
                    moved: false,
                }
            }
            None => Drag::Pan {
                last: e.position,
                moved: false,
            },
        });
    }

    pub(super) fn on_mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if self.drag.is_some() {
                self.end_drag(cx);
            }
            let hovered = self.table_at(e.position);
            if hovered != self.hovered {
                self.hovered = hovered;
                cx.notify();
            }
            return;
        }
        let w = self.to_world(e.position);
        match &mut self.drag {
            Some(Drag::Pan { last, moved }) => {
                let d = e.position - *last;
                *last = e.position;
                *moved = true;
                self.offset += d;
            }
            Some(Drag::Table { ix, grab, moved }) => {
                *moved = true;
                self.pos[*ix] = point(w.x - grab.x, w.y - grab.y);
            }
            Some(Drag::Minimap) => self.minimap_jump(e.position, cx),
            None => return,
        }
        cx.notify();
    }

    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Table { moved: true, .. }) => self.save_layout(),
            Some(Drag::Table {
                ix, moved: false, ..
            }) => {
                self.selected = (self.selected != Some(ix)).then_some(ix);
            }
            Some(Drag::Pan { moved: false, .. }) => self.selected = None,
            _ => {}
        }
        cx.notify();
    }

    // Minimap.

    pub(super) fn minimap_bounds(&self) -> Option<Bounds<Pixels>> {
        if !self.show_minimap || self.pos.is_empty() {
            return None;
        }
        let v = self.viewport.get();
        let size = size(px(MINIMAP.0), px(MINIMAP.1));
        Some(Bounds::new(
            point(
                v.origin.x + v.size.width - size.width - px(12.),
                v.origin.y + v.size.height - size.height - px(12.),
            ),
            size,
        ))
    }

    /// World bounds the minimap shows (the tables, padded) and its scale.
    pub(super) fn minimap_frame(&self) -> Option<(Bounds<f32>, f32)> {
        let tables = self.world_bounds()?;
        let pad = 40.;
        let world = Bounds::new(
            point(tables.origin.x - pad, tables.origin.y - pad),
            size(tables.size.width + pad * 2., tables.size.height + pad * 2.),
        );
        let scale =
            ((MINIMAP.0 - 12.) / world.size.width).min((MINIMAP.1 - 12.) / world.size.height);
        Some((world, scale))
    }

    fn minimap_jump(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(b), Some((world, scale))) = (self.minimap_bounds(), self.minimap_frame()) else {
            return;
        };
        let inner = minimap_inner(b, world, scale);
        let p = point(
            world.origin.x + f32::from(at.x - inner.x) / scale,
            world.origin.y + f32::from(at.y - inner.y) / scale,
        );
        self.center_on(p, self.zoom);
        cx.notify();
    }
}

/// Top-left (in pixels) where the minimap's world frame starts, centered in `b`.
pub(super) fn minimap_inner(b: Bounds<Pixels>, world: Bounds<f32>, scale: f32) -> Point<Pixels> {
    point(
        b.origin.x + (b.size.width - px(world.size.width * scale)) / 2.,
        b.origin.y + (b.size.height - px(world.size.height * scale)) / 2.,
    )
}
