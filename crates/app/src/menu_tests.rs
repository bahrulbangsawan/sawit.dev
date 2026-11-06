//! The shared menu component's Sawit patch (vendor/gpui-component/SAWIT_PATCH.md): a long item
//! is cut on one line with an ellipsis inside the menu, keeps its right padding, and the menu
//! never runs past the window.

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    Bounds, Context, Entity, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, size,
};

/// A window's root that shows one menu at its content's width, as an anchored menu is.
struct Holder(Entity<PopupMenu>);

impl Render for Holder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().items_start().child(self.0.clone())
    }
}

/// A session title as long as the ones the user saw cut mid-letter (item 15, case A).
const LONG: &str = "Claude Code · ◑ FormValiditySummary card design review for the checkout page";

/// A window `width` wide holding a menu (`max` wide at most) with a short item and [`LONG`],
/// drawn; the bounds of the two labels.
fn draw(cx: &mut TestAppContext, width: f32, max: Option<f32>) -> (Bounds<Pixels>, Bounds<Pixels>) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(width), px(400.)), move |window, cx| {
        Holder(PopupMenu::build(window, cx, move |menu, _, _| {
            let menu = menu
                .item(PopupMenuItem::new("Copy"))
                .item(PopupMenuItem::new(LONG));
            match max {
                Some(max) => menu.max_w(px(max)),
                None => menu,
            }
        }))
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let short = cx
        .debug_bounds("popup-menu-label-0")
        .expect("the short label");
    let long = cx
        .debug_bounds("popup-menu-label-1")
        .expect("the long label");
    (short, long)
}

#[gpui_kit::test]
fn a_long_item_is_cut_inside_its_menu(cx: &mut TestAppContext) {
    let (short, long) = draw(cx, 900., Some(236.));
    // Both labels start at the same place; the long one stops where the short one's row does,
    // within the 236 px menu, its padding kept.
    assert_eq!(short.origin.x, long.origin.x);
    assert!(long.size.width <= px(236.) - px(16.), "{long:?}");
    assert!(
        long.size.width > px(100.),
        "it still shows most of the title: {long:?}"
    );
}

#[gpui_kit::test]
fn a_menu_is_never_wider_than_its_window(cx: &mut TestAppContext) {
    // No max of its own: 500 px by default, and here the window is only 300 px wide.
    let (_, long) = draw(cx, 300., None);
    assert!(long.right() <= px(300.) - px(8.), "{long:?}");
}

#[gpui_kit::test]
fn a_short_menu_fits_its_content(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
        Holder(PopupMenu::build(window, cx, |menu, _, _| {
            menu.item(PopupMenuItem::new("Copy"))
        }))
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let label = cx.debug_bounds("popup-menu-label-0").expect("the label");
    assert!(
        label.size.width < px(200.),
        "content-fit, not the 500 px cap: {label:?}"
    );
}
