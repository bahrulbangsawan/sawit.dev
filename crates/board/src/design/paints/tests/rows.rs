//! The rows a layer's paints read as: solids, gradients, and lists that differ.

use super::*;

#[test]
fn solid_fill_reads_hex_and_opacity() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    // Figma's default fill.
    assert_eq!(rows_of(&board, &[r], "fills"), [solid("#D9D9D9", 1.0)]);

    node_set(
        &mut board,
        &[r],
        json!({"fills": [
            {"type": "solid", "color": "#ff8800", "opacity": 0.5},
            {"type": "solid", "color": "#00FF0080"},
            {"type": "solid", "color": "#0000FF", "visible": false},
        ]}),
    );
    let rows = rows_of(&board, &[r], "fills");
    assert_eq!(rows[0], solid("#FF8800", 0.5));
    // A colour that carries alpha of its own shows it as opacity.
    let PaintKind::Solid { hex, opacity } = &rows[1].kind else {
        panic!("{:?}", rows[1]);
    };
    assert_eq!(hex, "#00FF00");
    assert!((opacity - 128.0 / 255.0).abs() < 1e-9, "{opacity}");
    assert!(rows[1].visible);
    // A hidden paint is listed, and says so.
    assert_eq!(rows[2].kind, solid("#0000FF", 1.0).kind);
    assert!(!rows[2].visible);
}

#[test]
fn gradient_rows_read_their_kind() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": [
            {"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]},
            {"type": "radialGradient", "stops": ["#FF0000", "#0000FF"]},
            {"type": "angularGradient", "stops": ["#FF0000", "#0000FF"]},
            {"type": "diamondGradient", "stops": ["#FF0000", "#0000FF"]},
        ]}),
    );
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [
            gradient("Linear"),
            gradient("Radial"),
            gradient("Angular"),
            gradient("Diamond")
        ]
    );
}

#[test]
fn mixed_paints_are_none() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    node_set(&mut board, &[r2], json!({"fills": ["#FF0000"]}));

    assert_eq!(paints(&read(&board, &[r1, r2]), "fills"), None);
    // Each on its own is a list, and so are layers that agree.
    assert_eq!(rows_of(&board, &[r1], "fills"), [solid("#D9D9D9", 1.0)]);
    assert_eq!(rows_of(&board, &[r2], "fills"), [solid("#FF0000", 1.0)]);
    node_set(&mut board, &[r1], json!({"fills": ["#FF0000"]}));
    assert_eq!(rows_of(&board, &[r1, r2], "fills"), [solid("#FF0000", 1.0)]);

    // A stroke on one of them makes the strokes differ, though the fills agree now.
    node_set(&mut board, &[r1], json!({"strokes": ["#000000"]}));
    assert_eq!(paints(&read(&board, &[r1, r2]), "strokes"), None);
    assert!(paints(&read(&board, &[r1, r2]), "fills").is_some());
}

/// FigCraft leaves an empty list out of a layer's JSON; the panel must not read that as mixed.
#[test]
fn layers_without_paints_hold_empty_lists() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    node_set(&mut board, &[r1, r2], json!({"fills": []}));
    let sel = read(&board, &[r1, r2]);
    assert_eq!(paints(&sel, "fills"), Some(vec![]));
    assert_eq!(paints(&sel, "strokes"), Some(vec![]));
    // Nothing selected has no paints.
    assert_eq!(paints(&read(&board, &[]), "fills"), Some(vec![]));
}
