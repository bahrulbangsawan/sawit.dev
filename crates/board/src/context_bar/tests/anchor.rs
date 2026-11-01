//! Where the bar floats over the selection, and how it is kept inside the canvas.

use super::*;

#[test]
fn anchor_is_clamped_inside_canvas() {
    let (bar, canvas) = ((300.0, 36.0), (800.0, 600.0));
    // Past the top-left corner: the bar's centre is held 8 px plus half its width from the
    // left, and its top 8 px from the top.
    assert_eq!(clamp_anchor((5.0, -40.0), bar, canvas), (158.0, 8.0));
    // Past the bottom-right corner: its right edge and its bottom are 8 px in.
    assert_eq!(clamp_anchor((790.0, 590.0), bar, canvas), (642.0, 556.0));
    // A bar that fits is where it was put.
    assert_eq!(clamp_anchor((400.0, 300.0), bar, canvas), (400.0, 300.0));
    assert_eq!(clamp_anchor((158.0, 8.0), bar, canvas), (158.0, 8.0));
    assert_eq!(clamp_anchor((642.0, 556.0), bar, canvas), (642.0, 556.0));

    // However far out the anchor is, the bar ends up inside.
    let far = [
        -1e9,
        -300.0,
        0.0,
        5.0,
        400.0,
        799.0,
        1e9,
        f64::NEG_INFINITY,
        f64::INFINITY,
    ];
    for x in far {
        for y in far {
            let (cx, top) = clamp_anchor((x, y), bar, canvas);
            let (left, right, bottom) = (cx - bar.0 / 2.0, cx + bar.0 / 2.0, top + bar.1);
            assert!(left >= 8.0 && right <= canvas.0 - 8.0, "({x}, {y}) -> {cx}");
            assert!(
                top >= 8.0 && bottom <= canvas.1 - 8.0,
                "({x}, {y}) -> {top}"
            );
        }
    }

    // A canvas too small for the bar and its margins has no good place for it. It must not
    // panic; the margins at the left and the top win.
    assert_eq!(clamp_anchor((50.0, 50.0), bar, (200.0, 30.0)), (158.0, 8.0));
    assert_eq!(
        clamp_anchor((900.0, 900.0), bar, (200.0, 30.0)),
        (158.0, 8.0)
    );
    assert_eq!(clamp_anchor((0.0, 0.0), bar, (0.0, 0.0)), (158.0, 8.0));
    // Wide enough but not tall enough, and the other way round.
    assert_eq!(
        clamp_anchor((400.0, 300.0), bar, (800.0, 30.0)),
        (400.0, 8.0)
    );
    assert_eq!(
        clamp_anchor((400.0, 300.0), bar, (200.0, 600.0)),
        (158.0, 300.0)
    );
}

#[test]
fn anchor_floats_above_the_middle_of_the_selections_top() {
    let mut b = board();
    let sticky = sticky(&mut b);
    // The sticky is 200 wide at the page origin, which is at (300, 200): its top-centre is
    // (400, 200), and the bar's bottom-centre 12 px above it.
    assert_eq!(bar(&b).anchor, (400.0, 188.0));

    // Zoomed to 200 % about the middle of the canvas, the page origin is at (200, 100).
    b.execute("view.set", &json!({"zoom": 2})).unwrap();
    assert_eq!(bar(&b).anchor, (400.0, 88.0));
    b.execute("view.set", &json!({"zoom": 1})).unwrap();

    // Several layers: the top of the box around all of them. The shape is 160 wide at x 300.
    let shape = shape(&mut b);
    select(&mut b, &[sticky, shape]);
    assert_eq!(bar(&b).anchor, (530.0, 188.0));
}

#[test]
fn a_turned_selection_floats_above_its_highest_corner() {
    let mut b = board();
    let shape = shape(&mut b);
    // Turned 45 degrees about its middle, (380, 80), the square's corners are 80 * sqrt(2) from
    // it: one points straight up.
    b.execute("transform.rotate", &json!({"ids": [shape], "degrees": 45}))
        .unwrap();
    let (x, y) = bar(&b).anchor;
    let top = 200.0 + 80.0 - 80.0 * 2f64.sqrt();
    assert!((x - 680.0).abs() < 1e-6, "{x}");
    assert!((y - (top - 12.0)).abs() < 1e-6, "{y}");
}

#[test]
fn an_off_screen_selection_still_has_a_bar_that_lands_inside_the_canvas() {
    let mut b = board();
    make(&mut b, "board.sticky", json!({"x": -5000, "y": -5000}));
    let bar = bar(&b);
    assert_eq!(bar.anchor, (-4600.0, -4812.0));
    // A 300 × 36 bar sits above its anchor; the canvas is 800 × 600.
    let (x, y) = bar.anchor;
    assert_eq!(
        clamp_anchor((x, y - 36.0), (300.0, 36.0), (800.0, 600.0)),
        (158.0, 8.0)
    );
}
