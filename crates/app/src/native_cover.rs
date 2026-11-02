//! What lies over a native view. The browser's page, the canvas's live web frames and a screen's
//! Preview are WebKit views above the window's drawing, so whatever GPUI draws over one (a menu, a
//! popover, a dialog, a notification) is hidden behind it: the account menu cut off under the
//! browser's page, the footer's Pages menu under a live frame. Every one of those blocks the
//! mouse, so GPUI's own hit test says where they are. [`watch`] leaves a hitbox over the native
//! view's place each frame and, once that frame is on screen, asks whether anything that blocks
//! the mouse lies above it ([`covered`]); when the answer changes, the view that holds the native
//! view is told, and it puts a picture of its page in the view's place until nothing covers it.

use gpui_kit::*;

/// The points tested are this far inside the area's edges: a resize handle that reaches a few
/// pixels over a pane's edge isn't a menu.
const INSET: f32 = 6.;
/// And this far apart: every menu, popover, dialog and notification is wider and taller.
const STEP: f32 = 48.;

/// What [`watch`] knows of one native view's place: whether something covers it, and the hitbox
/// of the frame drawn last, the only one whose answer counts.
#[derive(Default)]
pub struct Cover {
    pub covered: bool,
    latest: Option<HitboxId>,
}

/// An element over the whole of its parent that watches it: put it last among the children of
/// the element where the native view is, so the view's own chrome over it (the canvas's toolbar
/// and context bar) is under the watch and doesn't count, while what GPUI draws above everything
/// (menus, popovers, dialogs, notifications) does. `cover` finds the view's [`Cover`]; `changed`
/// runs, after a frame, when whether something covers the place changed.
pub fn watch<V: 'static>(
    view: WeakEntity<V>,
    cover: fn(&mut V) -> &mut Cover,
    changed: fn(&mut V, bool, &mut Window, &mut Context<V>),
) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            let id = hitbox.id;
            if view.update(cx, |v, _| cover(v).latest = Some(id)).is_err() {
                return;
            }
            // Run before the next frame is drawn, so the frame on screen is this one.
            window.on_next_frame(move |window, cx| {
                let _ = view.update(cx, |v, cx| {
                    let place = cover(v);
                    if place.latest != Some(hitbox.id) {
                        return;
                    }
                    let now = covered(&hitbox, window);
                    if now != place.covered {
                        place.covered = now;
                        changed(v, now, window, cx);
                    }
                });
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Whether something that blocks the mouse covers part of `hitbox` in the frame on screen, which
/// must be the frame `hitbox` was inserted in.
pub fn covered(hitbox: &Hitbox, window: &Window) -> bool {
    let area = hitbox.bounds.intersect(&hitbox.content_mask.bounds);
    let xs = stops(f32::from(area.origin.x), f32::from(area.size.width));
    let ys = stops(f32::from(area.origin.y), f32::from(area.size.height));
    ys.iter()
        .any(|&y| (xs.iter()).any(|&x| !hitbox.is_hovered_at(point(px(x), px(y)), window)))
}

/// The coordinates tested along a side `len` long from `start`: [`INSET`] in from each end and
/// [`STEP`] apart between them, or the middle of a side too short for that. None for no side.
fn stops(start: f32, len: f32) -> Vec<f32> {
    if len <= 0. {
        return Vec::new();
    }
    if len <= 2. * INSET {
        return vec![start + len / 2.];
    }
    let end = start + len - INSET;
    let mut at = start + INSET;
    let mut stops = Vec::new();
    while at < end {
        stops.push(at);
        at += STEP;
    }
    stops.push(end);
    stops
}

#[cfg(test)]
mod tests {
    use gpui_kit::prelude::FluentBuilder as _;
    use gpui_kit::{
        Context, Hitbox, HitboxBehavior, InteractiveElement as _, IntoElement, ParentElement as _,
        Render, Styled as _, TestAppContext, VisualTestContext, Window, canvas, deferred, div, px,
    };

    use super::{STEP, covered, stops};

    /// The ends are tested, [`INSET`] in, and nothing between them is more than [`STEP`] from a
    /// point tested: a menu that reaches past the inset anywhere along a side is found.
    #[test]
    fn the_points_reach_both_ends_and_leave_no_gap_wider_than_a_step() {
        let at = stops(100., 1000.);
        assert_eq!(at.first(), Some(&106.));
        assert_eq!(at.last(), Some(&1094.));
        assert!(at.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] <= STEP));
        assert_eq!(stops(0., 8.), vec![4.]);
        assert!(stops(0., 0.).is_empty());
    }

    /// A place and, while `menu`, a menu over part of it, drawn above everything as GPUI defers
    /// menus; it blocks the mouse, as menus do. The place's hitbox of the frame drawn last is kept.
    struct Holder {
        menu: bool,
        hitbox: Option<Hitbox>,
    }

    impl Render for Holder {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity().downgrade();
            div()
                .size_full()
                .child(
                    div().relative().size(px(400.)).child(
                        canvas(
                            move |bounds, window, cx| {
                                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                                let _ = view.update(cx, |h, _| h.hitbox = Some(hitbox));
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    ),
                )
                .when(self.menu, |d| {
                    d.child(deferred(
                        div()
                            .id("menu")
                            .absolute()
                            .top(px(100.))
                            .left(px(300.))
                            .size(px(150.))
                            .occlude(),
                    ))
                })
        }
    }

    /// A menu over part of the place is found in the frame that draws it, and not once it is
    /// gone; what the place's own frame draws, with nothing over it, isn't.
    #[gpui_kit::test]
    fn a_menu_over_part_of_the_place_covers_it(cx: &mut TestAppContext) {
        let (holder, cx) = cx.add_window_view(|_, _| Holder {
            menu: false,
            hitbox: None,
        });
        let covered_now = |cx: &mut VisualTestContext, menu: bool| {
            holder.update(cx, |h, cx| {
                h.menu = menu;
                cx.notify();
            });
            cx.run_until_parked();
            let hitbox = holder
                .read_with(cx, |h, _| h.hitbox.clone())
                .expect("drawn");
            cx.update(|window, _| covered(&hitbox, window))
        };
        assert!(!covered_now(cx, false), "nothing is over the place");
        assert!(covered_now(cx, true), "the menu is over its right side");
        assert!(!covered_now(cx, false), "the menu closed");
    }
}
