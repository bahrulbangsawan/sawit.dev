//! Which part of the page a frame renders, and where an earlier frame is painted.

use super::*;

#[test]
fn render_view_adds_overscan() {
    let frame = render_view(&centred(), VP, 1.0).unwrap();
    assert_eq!(frame.origin, (-656.0, -556.0));
    assert_eq!(frame.scale, 2.0);
    assert_eq!((frame.width, frame.height), (2624, 2224));
}

#[test]
fn render_view_is_none_for_empty_viewport() {
    let no_width = ViewState {
        vw: 0.0,
        ..centred()
    };
    assert!(render_view(&no_width, VP, 1.0).is_none());
    // The same for the view's height, and for the canvas's own size: under one logical px
    // either way, or not a number.
    for (vw, vh) in [(800.0, 0.0), (0.5, 600.0), (f64::NAN, 600.0)] {
        let v = view(0.0, 0.0, 1.0, vw, vh);
        assert!(render_view(&v, VP, 1.0).is_none(), "view {vw} x {vh}");
    }
    for (width, height) in [
        (0.0, 600.0),
        (800.0, 0.0),
        (0.5, 600.0),
        (800.0, 0.99),
        (f64::NAN, 600.0),
        (800.0, f64::NAN),
    ] {
        let vp = Viewport {
            width,
            height,
            ..VP
        };
        assert!(
            render_view(&centred(), vp, 1.0).is_none(),
            "canvas {width} x {height}"
        );
    }
    // One logical px is enough to draw.
    let one = Viewport {
        width: 1.0,
        height: 1.0,
        ..VP
    };
    let v = view(0.0, 0.0, 1.0, 1.0, 1.0);
    assert!(render_view(&v, one, 1.0).is_some());
}

#[test]
fn render_view_is_none_for_a_scale_that_makes_no_frame() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
        let vp = Viewport {
            scale_factor: bad,
            ..VP
        };
        assert!(render_view(&centred(), vp, 1.0).is_none(), "sf {bad}");
        assert!(render_view(&centred(), VP, bad).is_none(), "quality {bad}");
        let v = ViewState {
            zoom: bad,
            ..centred()
        };
        assert!(render_view(&v, VP, 1.0).is_none(), "zoom {bad}");
    }
}

#[test]
fn render_view_is_none_or_a_finite_frame_for_any_numbers() {
    let mut drawn = 0;
    for &zoom in &NUMBERS {
        for &size in &NUMBERS {
            for &scale_factor in &NUMBERS {
                for &quality in &NUMBERS {
                    for &centre in &NUMBERS {
                        let v = view(centre, centre, zoom, size, size);
                        let vp = Viewport {
                            width: size,
                            height: size,
                            scale_factor,
                        };
                        let Some(frame) = render_view(&v, vp, quality) else {
                            continue;
                        };
                        drawn += 1;
                        assert!(
                            frame.origin.0.is_finite()
                                && frame.origin.1.is_finite()
                                && frame.scale.is_finite()
                                && frame.scale > 0.0
                                && frame.width >= 1
                                && frame.height >= 1,
                            "zoom {zoom}, size {size}, sf {scale_factor}, quality {quality}, \
                                 centre {centre}: {frame:?}"
                        );
                    }
                }
            }
        }
    }
    // The sweep does reach the frames.
    assert!(drawn > 0);
}

#[test]
fn a_preview_frame_covers_the_same_logical_area_with_fewer_pixels() {
    let v = centred();
    let preview = render_view(&v, VP, PREVIEW_QUALITY).unwrap();
    assert_eq!(preview.origin, (-656.0, -556.0));
    assert_eq!(preview.scale, 1.0);
    assert_eq!((preview.width, preview.height), (1312, 1112));
    assert_eq!(placement(&preview, &v), [-256.0, -256.0, 1312.0, 1112.0]);
}

#[test]
fn render_view_follows_the_zoom_and_rounds_sizes_up() {
    let v = view(10.0, 20.0, 2.0, 801.0, 601.0);
    let vp = Viewport {
        width: 801.0,
        height: 601.0,
        scale_factor: 1.5,
    };
    let frame = render_view(&v, vp, 1.0).unwrap();
    // Half the canvas is 200.25 page units at 200%, and the overscan 128.
    assert_eq!(frame.origin, (10.0 - 200.25 - 128.0, 20.0 - 150.25 - 128.0));
    assert_eq!(frame.scale, 3.0);
    // 1313 × 1.5 and 1113 × 1.5 are half a pixel short of a whole one.
    assert_eq!((frame.width, frame.height), (1970, 1670));
}

#[test]
fn placement_identity() {
    let v = centred();
    let frame = render_view(&v, VP, 1.0).unwrap();
    assert_eq!(placement(&frame, &v), [-256.0, -256.0, 1312.0, 1112.0]);
}

#[test]
fn placement_follows_pan() {
    let mut v = centred();
    let frame = render_view(&v, VP, 1.0).unwrap();
    pan(&mut v, 10.0, 0.0);
    assert_eq!(placement(&frame, &v)[0], -246.0);
}

#[test]
fn placement_follows_zoom() {
    let mut v = centred();
    let frame = render_view(&v, VP, 1.0).unwrap();
    zoom_at(&mut v, 2.0, (400.0, 300.0));
    assert_eq!(placement(&frame, &v), [-912.0, -812.0, 2624.0, 2224.0]);
}

#[test]
fn placement_is_always_finite_and_empty_for_a_frame_without_a_scale() {
    let no_scale = View {
        origin: (0.0, 0.0),
        scale: 0.0,
        width: 10,
        height: 10,
    };
    assert_eq!(placement(&no_scale, &centred()), [0.0; 4]);
    for &origin in &NUMBERS {
        for &scale in &NUMBERS {
            for &zoom in &NUMBERS {
                for &centre in &NUMBERS {
                    let frame = View {
                        origin: (origin, origin),
                        scale,
                        width: 1312,
                        height: 1112,
                    };
                    let now = view(centre, centre, zoom, 800.0, 600.0);
                    assert!(
                        placement(&frame, &now).iter().all(|n| n.is_finite()),
                        "origin {origin}, scale {scale}, zoom {zoom}, centre {centre}"
                    );
                }
            }
        }
    }
}
