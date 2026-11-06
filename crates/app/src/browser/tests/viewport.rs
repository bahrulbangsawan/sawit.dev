//! Device sizes: framed below the caption, captioned, and rotated.

use sawit_core::browser::ViewportMode;

use crate::browser::engine::Frame;
use crate::browser::viewport::{
    caption, caption_moves, device_ratio, device_scale, page_frame, rotated,
};

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
    // The scale gives up a hair so the height is exact too: the view may be points short, and
    // stays centered.
    assert!(
        scale <= 384. / 852. && scale > 0.97 * 384. / 852.,
        "{scale}"
    );
    assert!(
        f.x >= 26. && f.y == (60. + (384. - f.height) / 2.).round(),
        "{f:?}"
    );
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

/// A phone scaled to fit keeps its CSS size: the view is the device size times the scale, so
/// the page, laid out at the view's size over the scale (WebKit's view scale), sees the device's
/// width and height whatever the panel. The scale is under 1 only when the device doesn't fit.
#[test]
fn a_scaled_device_lays_out_at_its_own_size() {
    let phone = ViewportMode::Fixed {
        name: Some("iPhone 15".into()),
        width: 393,
        height: 852,
    };
    for (w, h) in [(1200., 900.), (800., 600.), (500., 420.), (300., 300.)] {
        let area = Frame {
            x: 0.,
            y: 0.,
            width: w,
            height: h,
        };
        let (f, scale) = page_frame(area, &phone);
        assert!(scale > 0. && scale < 1., "{w}×{h}: {scale}");
        // The page's viewport, as WebKit sizes it: the view's points over the scale.
        let (css_w, css_h) = (f64::from(f.width) / scale, f64::from(f.height) / scale);
        assert_eq!(css_w.floor(), 393., "{w}×{h}: width {css_w}");
        assert_eq!(css_h.floor(), 852., "{w}×{h}: height {css_h}");
        // Whole points, inside the area, below the caption.
        assert_eq!(f.width, f.width.round());
        assert!(f.x >= 0. && f.x + f.width <= w && f.y >= 24. && f.y + f.height <= h);
    }
    let (_, full) = page_frame(
        Frame {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 1200.,
        },
        &phone,
    );
    assert_eq!(full, 1.0);
}

/// Phones and tablets draw at their own pixel ratio, either way up; computers, sizes of no
/// device's and responsive mode keep the screen's.
#[test]
fn devices_bring_their_pixel_ratio() {
    let fixed = |name: Option<&str>, width, height| ViewportMode::Fixed {
        name: name.map(str::to_string),
        width,
        height,
    };
    assert_eq!(device_ratio(&fixed(Some("iPhone 15"), 393, 852)), Some(3.));
    assert_eq!(device_ratio(&fixed(Some("iPhone 15"), 852, 393)), Some(3.));
    assert_eq!(device_ratio(&fixed(Some("Pixel 8"), 412, 915)), Some(2.625));
    assert_eq!(device_ratio(&fixed(Some("iPad mini"), 744, 1133)), Some(2.));
    assert_eq!(device_ratio(&fixed(Some("Desktop"), 1440, 900)), None);
    assert_eq!(device_ratio(&fixed(Some("Laptop"), 1280, 800)), None);
    assert_eq!(device_ratio(&fixed(None, 500, 700)), None);
    assert_eq!(device_ratio(&ViewportMode::Responsive), None);
}

/// WebKit gives the page its device scale factor times the view scale: the factor is the ratio
/// over the scale, so an iPhone drawn at 44 % still reads 3.
#[test]
fn the_ratio_survives_the_scale() {
    assert_eq!(device_scale(3., 1.), 3.);
    assert!((device_scale(3., 0.4374) * 0.4374 - 3.).abs() < 1e-9);
    assert!((device_scale(2., 0.25) - 8.).abs() < 1e-9);
}
