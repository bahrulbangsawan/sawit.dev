//! Replacing a gradient, an image or a pattern with a solid.

use figcraft_color::{BlendMode, PaintKind as Held};

use super::*;
use crate::design::Mixed;
use crate::design::lists::whole_numbers;

#[test]
fn replace_with_solid_turns_a_gradient_into_its_first_colour() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    // The stops are listed out of order: the first colour is the one at position 0.
    node_set(
        &mut board,
        &[r],
        json!({"fills": [
            "#112233",
            {"type": "linearGradient", "opacity": 0.5, "blend": "multiply", "visible": false, "stops": [
                {"position": 1.0, "color": "#0000FF"},
                {"position": 0.0, "color": "#FF000080"},
            ]},
            "#445566",
        ]}),
    );
    let before = held(&board, r, "fills");

    let edit = replace_with_solid(&read(&board, &[r]), "fills", 1);
    apply(&mut board, &edit);

    let after = held(&board, r, "fills");
    assert_eq!(after[0], before[0]);
    assert_eq!(after[2], before[2]);
    assert!(matches!(after[1].kind, Held::Solid { .. }));
    assert_eq!(after[1].blend, BlendMode::Multiply, "the blend stays");
    assert!(!after[1].visible, "the eye stays");
    let rows = rows_of(&board, &[r], "fills");
    let PaintKind::Solid { hex, opacity } = &rows[1].kind else {
        panic!("{:?}", rows[1]);
    };
    assert_eq!(hex, "#FF0000");
    // The paint's opacity and the alpha of the stop.
    assert!((opacity - 0.5 * 128.0 / 255.0).abs() < 1e-6, "{opacity}");
    assert!(!rows[1].visible);
}

#[test]
fn replace_with_solid_turns_an_image_or_a_pattern_into_the_default_colour() {
    let mut board = design_file();
    let image = image_layer(&mut board);
    let source = rectangle(&mut board, "Tile", 300.0);
    let r = rectangle(&mut board, "R", 600.0);
    let image_paint = read(&board, &[image]).get("fills");
    let Mixed::Same(image_paints) = image_paint else {
        panic!("{image_paint:?}");
    };
    node_set(
        &mut board,
        &[r],
        json!({
            "fills": [{"type": "pattern", "source": source, "opacity": 0.5}, "#112233"],
            "strokes": whole_numbers(image_paints),
        }),
    );

    let edit = replace_with_solid(&read(&board, &[r]), "fills", 0);
    apply(&mut board, &edit);
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [solid("#D9D9D9", 0.5), solid("#112233", 1.0)]
    );
    // A stroke takes the colour of a new stroke.
    assert_eq!(rows_of(&board, &[r], "strokes")[0].kind, PaintKind::Image);
    let edit = replace_with_solid(&read(&board, &[r]), "strokes", 0);
    apply(&mut board, &edit);
    assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#000000", 1.0)]);
}

#[test]
fn replace_with_solid_leaves_a_solid_as_it_is() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": [{"type": "solid", "color": "#FF0000", "opacity": 0.5}]}),
    );
    let before = held(&board, r, "fills");
    let edit = replace_with_solid(&read(&board, &[r]), "fills", 0);
    apply(&mut board, &edit);
    assert_eq!(held(&board, r, "fills"), before);
}
