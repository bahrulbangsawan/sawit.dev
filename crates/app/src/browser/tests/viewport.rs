//! Device sizes: framed below the caption, captioned, and rotated.

use sawit_core::browser::ViewportMode;

use crate::browser::engine::Frame;
use crate::browser::viewport::{caption, caption_moves, page_frame, rotated};

#[test]
fn devices_are_centered_below_the_caption() {
    let bounds = Frame {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 1000.,
    };
    let phone = ViewportMode::Fixed {
        name: Some("iPhone 15".into()),
        width: 393,
        height: 852,
    };
    assert_eq!(
        page_frame(bounds, &phone),
        (
            Frame {
                x: 204.,
                y: 86.,
                width: 393.,
                height: 852.
            },
            1.0
        )
    );
    assert_eq!(page_frame(bounds, &ViewportMode::Responsive), (bounds, 1.0));
    // Scaled to fit a small area; offset by the bounds' origin.
    let small = Frame {
        x: 10.,
        y: 20.,
        width: 232.,
        height: 440.,
    };
    let (f, scale) = page_frame(small, &phone);
    assert!((scale - 384. / 852.).abs() < 1e-3 && f.x >= 26. && (f.y - 60.).abs() < 1e-3);
}

#[test]
fn captions() {
    assert_eq!(
        caption(Some("iPhone 15"), 393, 852, 0.75),
        "iPhone 15 · 393 × 852 · 75%"
    );
    assert_eq!(caption(None, 800, 600, 1.), "800 × 600");
}

#[test]
fn rotating_swaps_the_size() {
    let phone = ViewportMode::Fixed {
        name: Some("iPhone 15".into()),
        width: 393,
        height: 852,
    };
    assert_eq!(
        rotated(&phone),
        Some(ViewportMode::Fixed {
            name: Some("iPhone 15".into()),
            width: 852,
            height: 393,
        })
    );
    assert_eq!(rotated(&ViewportMode::Responsive), None);
}

/// A device's caption says the zoom its page area gives it, which the layout tells only as the
/// page is placed: a new area draws the view again after that frame, in device mode only. (The
/// placing itself needs a webview, which a test window can't have.)
#[test]
fn only_a_devices_caption_follows_the_page_area() {
    use gpui_kit::{Bounds, point, px, size};
    let area = |width: f32| Bounds::new(point(px(0.), px(36.)), size(px(width), px(600.)));
    let phone = ViewportMode::Fixed {
        name: Some("iPhone 15".into()),
        width: 393,
        height: 852,
    };
    assert!(caption_moves(None, area(800.), &phone), "placed first");
    assert!(
        caption_moves(Some(area(800.)), area(400.), &phone),
        "the area shrank"
    );
    assert!(
        !caption_moves(Some(area(800.)), area(800.), &phone),
        "the same area"
    );
    assert!(!caption_moves(None, area(800.), &ViewportMode::Responsive));
    assert!(!caption_moves(
        Some(area(800.)),
        area(400.),
        &ViewportMode::Responsive
    ));
}
