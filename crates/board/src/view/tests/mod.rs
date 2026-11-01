//! The view's tests: the canvas and the numbers they run on.

use super::*;

mod frame;
mod navigation;
mod pixels;

/// 800 × 600 logical px on a 2× screen.
const VP: Viewport = Viewport {
    width: 800.0,
    height: 600.0,
    scale_factor: 2.0,
};

/// A view of `vw` × `vh` logical px centred on (`cx`, `cy`).
fn view(cx: f64, cy: f64, zoom: f64, vw: f64, vh: f64) -> ViewState {
    ViewState {
        cx,
        cy,
        zoom,
        vw,
        vh,
        ..ViewState::default()
    }
}

/// The page origin in the middle of [`VP`], at 100%.
fn centred() -> ViewState {
    view(0.0, 0.0, 1.0, VP.width, VP.height)
}

/// A document whose first page holds a rectangle at each (x, y, width, height).
fn page_with(rects: &[(f64, f64, f64, f64)]) -> (Document, NodeId) {
    let mut doc = Document::new("File");
    let page = doc.first_page().unwrap();
    for &(x, y, w, h) in rects {
        doc.create(page, NodeKind::Rectangle, "Rectangle", x, y, w, h)
            .unwrap();
    }
    (doc, page)
}

/// What a trackpad, a layout pass or a damaged file can hand in: not numbers, zero, negative,
/// tiny, huge, and ordinary ones.
const NUMBERS: [f64; 12] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.0,
    -1.0,
    1e-300,
    1e300,
    f64::MAX,
    f64::MIN,
    0.5,
    2.0,
    800.0,
];
