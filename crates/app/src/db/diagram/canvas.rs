//! The canvas: the dot grid and the foreign-key curves painted under the cards, the visible
//! cards, the zoom and layout controls, and the minimap.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::viewport::minimap_inner;
use super::{CARD_PAD, DETAIL_ZOOM, DiagramView, Drag, HEADER_H, MAX_ZOOM, MIN_ZOOM, ROW_H};

impl DiagramView {
    // Drawing.

    /// Tables related to the selected one, itself included.
    fn focus_set(&self) -> Option<Vec<bool>> {
        let t = self.selected?;
        let mut set = vec![false; self.erd.tables.len()];
        set[t] = true;
        for e in &self.erd.edges {
            if e.from == t {
                set[e.to] = true;
            }
            if e.to == t {
                set[e.from] = true;
            }
        }
        Some(set)
    }

    pub(super) fn render_canvas(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.needs_fit {
            if self.viewport.get().size.width > px(0.) {
                self.fit(cx);
            } else {
                window.request_animation_frame();
            }
        }
        let theme = cx.theme();
        let focus = self.focus_set();
        let searching = !self.hits.is_empty();
        let view = self.viewport.get();
        let visible = Bounds::new(point(px(0.), px(0.)), view.size);

        // Edges, in viewport coordinates, painted under the cards.
        let edges: Vec<EdgeShape> = self
            .erd
            .edges
            .iter()
            .map(|e| {
                let active = [self.selected, self.hovered]
                    .iter()
                    .any(|t| *t == Some(e.from) || *t == Some(e.to));
                let row = |t: usize, cols: &[usize]| {
                    let p = self.pos[t];
                    match cols.first() {
                        Some(&c) if self.zoom >= DETAIL_ZOOM => {
                            p.y + HEADER_H + CARD_PAD / 2. + ROW_H * (c as f32 + 0.5)
                        }
                        _ => p.y + HEADER_H / 2.,
                    }
                };
                let (a, b) = (e.from, e.to);
                let (ay, by) = (row(a, &e.from_columns), row(b, &e.to_columns));
                let (pa, sa, pb, sb) = (self.pos[a], self.sizes[a], self.pos[b], self.sizes[b]);
                // Leave from the side facing the other table.
                let (ax, a_dir, bx, b_dir) = if pa.x > pb.x + sb.width {
                    (pa.x, -1., pb.x + sb.width, 1.)
                } else if pb.x > pa.x + sa.width {
                    (pa.x + sa.width, 1., pb.x, -1.)
                } else {
                    // Stacked: loop out on the right.
                    (pa.x + sa.width, 1., pb.x + sb.width, 1.)
                };
                EdgeShape {
                    from: self.to_screen(point(ax, ay)) - view.origin,
                    from_dir: a_dir,
                    to: self.to_screen(point(bx, by)) - view.origin,
                    to_dir: b_dir,
                    optional: e.optional,
                    color: if active {
                        theme.primary
                    } else if focus.is_some() || searching {
                        theme.muted_foreground.opacity(0.25)
                    } else {
                        theme.muted_foreground.opacity(0.6)
                    },
                    width: if active { 2. } else { 1.25 },
                }
            })
            .collect();
        let dot = theme.border;
        let zoom = self.zoom;
        let offset = self.offset;
        let viewport = self.viewport.clone();
        let canvas_layer = canvas(
            move |bounds, _, _| viewport.set(bounds),
            move |bounds, _, window, _| {
                paint_grid(bounds, offset, zoom, dot, window);
                for e in &edges {
                    paint_edge(bounds, e, zoom, window);
                }
            },
        )
        .absolute()
        .size_full();

        let cards: Vec<AnyElement> = (0..self.erd.tables.len())
            .filter(|&i| {
                let at = self.to_screen(self.pos[i]) - view.origin;
                let s = size(
                    px(self.sizes[i].width * zoom),
                    px(self.sizes[i].height * zoom),
                );
                // Before the first paint the viewport is unknown: draw all.
                view.size.width == px(0.) || Bounds::new(at, s).intersects(&visible)
            })
            .map(|i| {
                let dim = match &focus {
                    Some(set) => !set[i],
                    None => searching && !self.has_hit(i),
                };
                self.render_card(i, dim, cx)
            })
            .collect();

        let panning = matches!(self.drag, Some(Drag::Pan { moved: true, .. }));
        let moving = matches!(self.drag, Some(Drag::Table { moved: true, .. }));
        div()
            .id("erd-canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .cursor(if panning || moving {
                CursorStyle::ClosedHand
            } else if self.hovered.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::OpenHand
            })
            .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| this.on_scroll(e, cx)))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.zoom_at(this.zoom * (1. + e.delta), e.position, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.blur(cx);
                    this.on_mouse_down(e, cx)
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.on_mouse_move(e, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
            .child(canvas_layer)
            .children(cards)
            .child(self.render_controls(cx))
            .when_some(self.render_minimap(cx), |el, m| el.child(m))
            .into_any_element()
    }

    /// Zoom, fit, layout and minimap buttons, bottom-left.
    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .id("erd-controls")
            .absolute()
            .left(px(12.))
            .bottom(px(12.))
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .text_xs()
            .text_color(theme.muted_foreground)
            // Clicks here shouldn't start a pan underneath.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("erd-zoom-out")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ZoomOut)
                    .tooltip("Zoom Out (⌘ Scroll)")
                    .disabled(self.zoom <= MIN_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / 1.25, cx))),
            )
            .child(
                Button::new("erd-zoom-reset")
                    .ghost()
                    .xsmall()
                    .label(format!("{}%", (self.zoom * 100.).round()))
                    .tooltip("Zoom to 100%")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let z = 1. / this.zoom;
                        this.zoom_by(z, cx)
                    })),
            )
            .child(
                Button::new("erd-zoom-in")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ZoomIn)
                    .tooltip("Zoom In (⌘ Scroll)")
                    .disabled(self.zoom >= MAX_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(theme.border))
            .child(
                Button::new("erd-fit")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Maximize)
                    .tooltip("Fit to Screen")
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(
                Button::new("erd-layout")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::LayoutGrid)
                    .tooltip("Auto Layout (forgets moved tables)")
                    .on_click(cx.listener(|this, _, _, cx| this.auto_layout(cx))),
            )
            .child(
                Button::new("erd-minimap")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Map)
                    .selected(self.show_minimap)
                    .tooltip("Minimap")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_minimap = !this.show_minimap;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_minimap(&self, cx: &App) -> Option<AnyElement> {
        let b = self.minimap_bounds()?;
        let (world, scale) = self.minimap_frame()?;
        let theme = cx.theme();
        let view = self.viewport.get();
        let seen = Bounds::from_corners(
            self.to_world(view.origin),
            self.to_world(view.origin + point(view.size.width, view.size.height)),
        );
        let rects: Vec<(Bounds<f32>, bool)> = self
            .pos
            .iter()
            .zip(&self.sizes)
            .enumerate()
            .map(|(i, (p, s))| (Bounds::new(*p, *s), self.selected == Some(i)))
            .collect();
        let (fill_c, sel_c, frame_c) = (
            theme.muted_foreground.opacity(0.35),
            theme.primary,
            theme.primary,
        );
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let inner = minimap_inner(bounds, world, scale);
                    let map = |r: Bounds<f32>| {
                        Bounds::new(
                            point(
                                inner.x + px((r.origin.x - world.origin.x) * scale),
                                inner.y + px((r.origin.y - world.origin.y) * scale),
                            ),
                            size(
                                px((r.size.width * scale).max(2.)),
                                px((r.size.height * scale).max(2.)),
                            ),
                        )
                    };
                    for (r, sel) in &rects {
                        window.paint_quad(
                            fill(map(*r), if *sel { sel_c } else { fill_c }).corner_radii(px(1.5)),
                        );
                    }
                    window.paint_quad(
                        outline(map(seen.intersect(&world)), frame_c, BorderStyle::Solid)
                            .corner_radii(px(2.)),
                    );
                },
            )
            .absolute()
            .left(b.origin.x - view.origin.x)
            .top(b.origin.y - view.origin.y)
            .w(b.size.width)
            .h(b.size.height)
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .into_any_element(),
        )
    }
}

struct EdgeShape {
    from: Point<Pixels>,
    /// -1 when the edge leaves to the left, 1 to the right.
    from_dir: f32,
    to: Point<Pixels>,
    to_dir: f32,
    optional: bool,
    color: Hsla,
    width: f32,
}

/// One foreign key: a curve with a crow's foot ("many") on the referencing
/// table and a bar ("one") on the referenced one; dashed when optional.
/// Nothing is built for one wholly off the canvas `bounds`, which clips it anyway.
fn paint_edge(bounds: Bounds<Pixels>, e: &EdgeShape, zoom: f32, window: &mut Window) {
    let z = zoom.clamp(0.4, 1.5);
    let (a, b) = (bounds.origin + e.from, bounds.origin + e.to);
    let stub = px(16. * z);
    let (a1, b1) = (
        point(a.x + stub * e.from_dir, a.y),
        point(b.x + stub * e.to_dir, b.y),
    );
    let reach = ((b1.x - a1.x).abs() / 2.).max(px(40. * z));
    let (c1, c2) = (
        point(a1.x + reach * e.from_dir, a1.y),
        point(b1.x + reach * e.to_dir, b1.y),
    );
    let spread = px(6. * z);
    // The curve stays within its control points' box; the crow's foot and the bar lie between
    // its ends and the stubs, `spread` above and below.
    if !edge_box(&[a, a1, c1, c2, b1, b], spread + px(e.width)).intersects(&bounds) {
        return;
    }
    let mut path = PathBuilder::stroke(px(e.width));
    if e.optional {
        path = path.dash_array(&[px(5.), px(4.)]);
    }
    path.move_to(a);
    path.line_to(a1);
    path.cubic_bezier_to(b1, c1, c2);
    path.line_to(b);
    if let Ok(path) = path.build() {
        window.paint_path(path, e.color);
    }
    let line = |from: Point<Pixels>, to: Point<Pixels>, window: &mut Window| {
        let mut p = PathBuilder::stroke(px(e.width));
        p.move_to(from);
        p.line_to(to);
        if let Ok(p) = p.build() {
            window.paint_path(p, e.color);
        }
    };
    // Crow's foot: three prongs meeting a short way out from the card.
    let toe = point(a.x + px(10. * z) * e.from_dir, a.y);
    for dy in [-spread, px(0.), spread] {
        line(toe, point(a.x, a.y + dy), window);
    }
    // One: a bar across the line near the referenced card.
    let bar_x = b.x + px(8. * z) * e.to_dir;
    line(
        point(bar_x, b.y - spread),
        point(bar_x, b.y + spread),
        window,
    );
}

/// The box around `points`, `pad` wider on every side.
fn edge_box(points: &[Point<Pixels>], pad: Pixels) -> Bounds<Pixels> {
    let (mut min, mut max) = (points[0], points[0]);
    for p in &points[1..] {
        min = point(min.x.min(p.x), min.y.min(p.y));
        max = point(max.x.max(p.x), max.y.max(p.y));
    }
    Bounds::from_corners(
        point(min.x - pad, min.y - pad),
        point(max.x + pad, max.y + pad),
    )
}

/// A dot grid that moves with the canvas; hidden when too dense.
fn paint_grid(
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    zoom: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut step = 24. * zoom;
    while step < 12. {
        step *= 2.;
    }
    let step = px(step);
    let r = px(1.);
    let start = |o: Pixels| o - (o / step).floor() * step;
    let mut x = bounds.origin.x + start(offset.x);
    while x < bounds.origin.x + bounds.size.width {
        let mut y = bounds.origin.y + start(offset.y);
        while y < bounds.origin.y + bounds.size.height {
            window.paint_quad(
                fill(
                    Bounds::centered_at(point(x, y), size(r * 2., r * 2.)),
                    color,
                )
                .corner_radii(r),
            );
            y += step;
        }
        x += step;
    }
}
