//! Panning, zooming and zooming to fit, whatever numbers come in.

use super::*;
use crate::doc::BoardDoc;

fn all_finite(v: &ViewState) -> bool {
    [v.cx, v.cy, v.zoom, v.vw, v.vh]
        .iter()
        .all(|f| f.is_finite())
}

#[test]
fn pan_moves_the_content_with_the_fingers() {
    let mut v = view(0.0, 0.0, 2.0, 800.0, 600.0);
    let before = v.page_to_screen(30.0, 40.0);
    pan(&mut v, 10.0, -6.0);
    let after = v.page_to_screen(30.0, 40.0);
    assert_eq!((after.0 - before.0, after.1 - before.1), (10.0, -6.0));
}

#[test]
fn pan_ignores_deltas_that_are_not_finite_or_overflow() {
    // At 2%, a delta of f64::MAX is 50 times that in page units: more than a float holds.
    let mut v = view(5.0, 6.0, 0.02, 800.0, 600.0);
    let before = v.clone();
    for (dx, dy) in [
        (f64::NAN, 5.0),
        (5.0, f64::INFINITY),
        (f64::NEG_INFINITY, f64::NAN),
        (f64::MAX, 0.0),
        (0.0, f64::MIN),
    ] {
        pan(&mut v, dx, dy);
        assert_eq!(v, before, "pan({dx}, {dy})");
    }
}

#[test]
fn zoom_at_keeps_the_page_point_under_the_cursor() {
    let mut v = view(120.0, -40.0, 0.5, 800.0, 600.0);
    let cursor = (100.0, 450.0);
    let under = v.screen_to_page(cursor.0, cursor.1);
    // In, out, and into both ends of the zoom range.
    for factor in [2.0, 3.0, 0.1, 1.5, 1e9, 1e-9, 4.0] {
        zoom_at(&mut v, factor, cursor);
        let now = v.screen_to_page(cursor.0, cursor.1);
        assert!(
            (now.0 - under.0).abs() < 1e-9 && (now.1 - under.1).abs() < 1e-9,
            "factor {factor}: {now:?}, not {under:?}"
        );
    }
    assert_eq!(v.zoom, 0.02 * 4.0);
}

#[test]
fn extreme_input_leaves_the_view_finite() {
    let starts = [
        centred(),
        // Already far out, at the top zoom, and at the very edge of what a float holds.
        view(1e300, -1e300, 256.0, 800.0, 600.0),
        view(f64::MAX, f64::MIN, 0.02, 800.0, 600.0),
    ];
    for start in &starts {
        let from = (start.cx, start.cy, start.zoom);
        for &a in &NUMBERS {
            for &b in &NUMBERS {
                let mut v = start.clone();
                pan(&mut v, a, b);
                assert!(all_finite(&v), "pan({a}, {b}) from {from:?}");
                for &c in &NUMBERS {
                    let mut v = start.clone();
                    zoom_at(&mut v, a, (b, c));
                    assert!(all_finite(&v), "zoom_at({a}, ({b}, {c})) from {from:?}");
                    assert!(
                        (0.02..=256.0).contains(&v.zoom),
                        "zoom_at({a}, ({b}, {c})) from {from:?}: zoom {}",
                        v.zoom
                    );
                }
            }
        }
    }
}

#[test]
fn zoom_to_fit_shows_the_content_and_stops_at_100_percent() {
    // Wide content: zoomed out until it fills the canvas but for a 64 px margin each side.
    let (doc, page) = page_with(&[(0.0, 0.0, 4000.0, 1000.0)]);
    let mut v = centred();
    zoom_to_fit(&mut v, &doc, page);
    assert!((v.zoom - 672.0 / 4000.0).abs() < 1e-12, "{}", v.zoom);
    assert_eq!((v.cx, v.cy), (2000.0, 500.0));
    let (left, _) = v.page_to_screen(0.0, 0.0);
    let (right, _) = v.page_to_screen(4000.0, 1000.0);
    assert!((left - 64.0).abs() < 1e-9 && (right - 736.0).abs() < 1e-9);

    // Small content: centred, and not magnified past 100%.
    let (doc, page) = page_with(&[(100.0, 100.0, 200.0, 100.0)]);
    zoom_to_fit(&mut v, &doc, page);
    assert_eq!((v.zoom, v.cx, v.cy), (1.0, 200.0, 150.0));
}

#[test]
fn zoom_to_fit_an_empty_board_is_100_percent_on_the_origin() {
    let board = BoardDoc::new_board("Canvas").unwrap();
    let doc = board.doc().unwrap();
    let page = doc.first_page().unwrap();
    let mut v = view(500.0, -300.0, 0.5, 800.0, 600.0);
    zoom_to_fit(&mut v, &doc, page);
    assert_eq!((v.zoom, v.cx, v.cy), (1.0, 0.0, 0.0));
}

#[test]
fn zoom_to_fit_content_without_finite_bounds_is_100_percent_on_the_origin() {
    // An infinite width makes the bounds not a number.
    let infinite = page_with(&[(0.0, 0.0, f64::INFINITY, 10.0)]);
    // Finite bounds, but so far out that adding their edges for the centre overflows.
    let far = page_with(&[(1e308, 0.0, 7e307, 10.0)]);
    for (doc, page) in [infinite, far] {
        let mut v = view(500.0, -300.0, 0.5, 800.0, 600.0);
        zoom_to_fit(&mut v, &doc, page);
        assert_eq!((v.zoom, v.cx, v.cy), (1.0, 0.0, 0.0));
    }
}

#[test]
fn zoom_at_clamps_and_ignores_non_finite() {
    let mut v = centred();
    zoom_at(&mut v, 1e9, (400.0, 300.0));
    assert_eq!(v.zoom, 256.0);
    let before = v.clone();
    zoom_at(&mut v, f64::NAN, (400.0, 300.0));
    assert_eq!(v.zoom, before.zoom);
    assert!(all_finite(&v));

    // The rest of what is no zoom at all changes nothing: a factor that is not finite or not
    // above zero, and a point that is not finite.
    for factor in [f64::INFINITY, f64::NEG_INFINITY, 0.0, -2.0] {
        zoom_at(&mut v, factor, (400.0, 300.0));
        assert_eq!(v, before, "factor {factor}");
    }
    for at in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        zoom_at(&mut v, 2.0, (at, 300.0));
        zoom_at(&mut v, 2.0, (400.0, at));
        assert_eq!(v, before, "screen point {at}");
    }

    // The far end the other way, and a factor so big that its product overflows is still
    // "as far as it goes".
    zoom_at(&mut v, 1e-9, (400.0, 300.0));
    assert_eq!(v.zoom, 0.02);
    zoom_at(&mut v, 100.0, (400.0, 300.0));
    zoom_at(&mut v, f64::MAX, (400.0, 300.0));
    assert_eq!(v.zoom, 256.0);
    assert!(all_finite(&v));
}
