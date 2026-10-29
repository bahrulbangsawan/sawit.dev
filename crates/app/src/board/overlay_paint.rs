//! Painting the overlay over the canvas: the shapes `sawit_board::overlay` lists (the selection,
//! handles, names, guides and previews), with `paint_path` and `paint_quad`, and their labels'
//! text shaped by the window's text system. Where everything goes is decided in `sawit_board`;
//! this only measures the labels and paints.

use figcraft_doc::NodeId;
use figcraft_doc::geom::Point as PagePoint;
use figcraft_engine::ViewState;
use gpui_kit::*;
use sawit_board::overlay::{self, LABEL_LINE, LABEL_TEXT, Shape};

/// How round a square handle's corners are, and a label's pill (px).
const HANDLE_RADIUS: f32 = 1.0;
const PILL_RADIUS: f32 = 3.0;
/// The dashes and gaps of a dashed line (px).
const DASH: [f32; 2] = [4.0, 3.0];

/// The overlay of one paint, its labels shaped and placed: worked out in the prepaint, painted
/// over the frame.
pub(super) struct Overlay(Vec<Item>);

enum Item {
    /// A shape, painted as it is listed.
    Shape(Shape),
    /// A label: its box from the canvas's top-left, the colour of its pill, and its text shaped
    /// (boxed: a shaped line keeps its runs inline, and is large).
    Label {
        bounds: Bounds<Pixels>,
        pill: Option<Hsla>,
        text: Box<ShapedLine>,
    },
}

/// The frames' names as the engine hit-tests them (`ToolState::labels`): each frame, and the page
/// quad its name covers, in the order they are drawn.
pub(super) type Names = Vec<(NodeId, [PagePoint; 4])>;

/// Shapes the text of the labels in `shapes` in the font `family` and places them, on a canvas
/// whose view is `view`; the other shapes are kept as they are. Also returns where the frames'
/// names are on the page, as painted.
pub(super) fn layout(
    shapes: Vec<Shape>,
    family: &SharedString,
    view: &ViewState,
    window: &Window,
) -> (Overlay, Names) {
    let mut names = Vec::new();
    let mut items = Vec::with_capacity(shapes.len());
    for shape in shapes {
        if let Shape::Label {
            text,
            color,
            background,
            node,
            ..
        } = &shape
        {
            let line = shape_text(text, *color, background.is_some(), family, window);
            if let Some((top_left, size)) = shape.label_box(f64::from(line.width)) {
                if let Some(id) = node {
                    names.push((*id, overlay::label_quad(top_left, size, view)));
                }
                items.push(Item::Label {
                    bounds: bounds(top_left, size),
                    pill: background.map(to_hsla),
                    text: Box::new(line),
                });
            }
            continue;
        }
        items.push(Item::Shape(shape));
    }
    (Overlay(items), names)
}

/// `text` on one line in the labels' size, in `color`: semibold on a pill, regular otherwise.
fn shape_text(
    text: &str,
    color: overlay::Rgba,
    on_pill: bool,
    family: &SharedString,
    window: &Window,
) -> ShapedLine {
    // A name with line breaks reads on one line.
    let text = SharedString::from(text.replace(['\n', '\r'], " "));
    let mut face = font(family.clone());
    if on_pill {
        face.weight = FontWeight::SEMIBOLD;
    }
    let run = TextRun {
        len: text.len(),
        font: face,
        color: to_hsla(color),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text, px(LABEL_TEXT as f32), &[run], None)
}

/// Paints `overlay` over the canvas whose top-left is at `origin` in the window, back to front.
pub(super) fn paint(origin: Point<Pixels>, overlay: Overlay, window: &mut Window, cx: &mut App) {
    for item in overlay.0 {
        match item {
            Item::Shape(shape) => paint_shape(origin, shape, window),
            Item::Label { bounds, pill, text } => {
                let bounds = Bounds::new(origin + bounds.origin, bounds.size);
                if let Some(pill) = pill {
                    window.paint_quad(fill(bounds, pill).corner_radii(px(PILL_RADIUS)));
                }
                // The text, centred in its box.
                let line = px(LABEL_LINE as f32);
                let at = point(
                    bounds.origin.x + (bounds.size.width - text.width) / 2.,
                    bounds.origin.y + (bounds.size.height - line) / 2.,
                );
                if let Err(err) = text.paint(at, line, TextAlign::Left, None, window, cx) {
                    log::error!("a canvas label couldn't be painted: {err}");
                }
            }
        }
    }
}

fn paint_shape(origin: Point<Pixels>, shape: Shape, window: &mut Window) {
    match shape {
        Shape::Outline {
            points,
            closed,
            color,
            width,
            dashed,
        } => {
            let mut path = PathBuilder::stroke(px(width as f32));
            if dashed {
                path = path.dash_array(&DASH.map(px));
            }
            paint_points(path, origin, &points, closed, color, window);
        }
        Shape::Fill { points, color } => {
            paint_points(PathBuilder::fill(), origin, &points, true, color, window);
        }
        Shape::Handle {
            center,
            size: across,
            round,
            fill: inside,
            stroke,
        } => {
            let side = px(across as f32);
            let bounds = Bounds::centered_at(at(origin, center), size(side, side));
            let radius = if round { side / 2. } else { px(HANDLE_RADIUS) };
            window.paint_quad(quad(
                bounds,
                radius,
                to_hsla(inside),
                px(1.),
                to_hsla(stroke),
                BorderStyle::Solid,
            ));
        }
        Shape::Caret { top, bottom, color } => {
            let path = PathBuilder::stroke(px(1.));
            paint_points(path, origin, &[top, bottom], false, color, window);
        }
        // `layout` made each label an `Item::Label`.
        Shape::Label { .. } => {}
    }
}

/// Paints the line through `points` (closed when `closed`), or the polygon for a fill, as `path`
/// styles it, `points` being from the canvas's top-left at `origin`. Nothing for fewer than two
/// points, or a point off any screen (not finite as a pixel), which the path builder refuses.
fn paint_points(
    mut path: PathBuilder,
    origin: Point<Pixels>,
    points: &[(f64, f64)],
    closed: bool,
    color: overlay::Rgba,
    window: &mut Window,
) {
    let finite = |(x, y): &(f64, f64)| (*x as f32).is_finite() && (*y as f32).is_finite();
    if points.len() < 2 || !points.iter().all(finite) {
        return;
    }
    let points: Vec<Point<Pixels>> = points.iter().map(|p| at(origin, *p)).collect();
    path.add_polygon(&points, closed);
    if let Ok(path) = path.build() {
        window.paint_path(path, to_hsla(color));
    }
}

/// The point `(x, y)` from the canvas's top-left at `origin`, in the window.
fn at(origin: Point<Pixels>, (x, y): (f64, f64)) -> Point<Pixels> {
    point(origin.x + px(x as f32), origin.y + px(y as f32))
}

fn bounds((x, y): (f64, f64), (width, height): (f64, f64)) -> Bounds<Pixels> {
    Bounds::new(
        point(px(x as f32), px(y as f32)),
        size(px(width as f32), px(height as f32)),
    )
}

fn to_hsla([r, g, b, a]: overlay::Rgba) -> Hsla {
    let channel = |c: u8| f32::from(c) / 255.;
    Rgba {
        r: channel(r),
        g: channel(g),
        b: channel(b),
        a: channel(a),
    }
    .into()
}
