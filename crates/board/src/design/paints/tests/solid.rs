//! Setting a solid row's colour and opacity, and the colours the hex field takes.

use figcraft_color::{BlendMode, PaintKind as Held};

use super::*;

#[test]
fn set_solid_changes_one_row() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(&mut board, &[r], json!({"fills": ["#FF0000", "#0000FF"]}));
    let before = held(&board, r, "fills");

    let edit = set_solid(&read(&board, &[r]), "fills", 1, "#00ff00", 0.5).unwrap();
    assert_eq!(edit.command, "node.set");
    apply(&mut board, &edit);

    let after = held(&board, r, "fills");
    assert_eq!(after[0], before[0], "the other row is as it was");
    assert_ne!(after[1], before[1]);
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [solid("#FF0000", 1.0), solid("#00FF00", 0.5)]
    );
}

#[test]
fn set_solid_keeps_the_blend_and_the_eye_of_its_row() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": [{"type": "solid", "color": "#FF0000", "blend": "multiply", "visible": false}]}),
    );
    let edit = set_solid(&read(&board, &[r]), "fills", 0, "#0000FF", 0.25).unwrap();
    apply(&mut board, &edit);

    let after = &held(&board, r, "fills")[0];
    assert_eq!(after.blend, BlendMode::Multiply);
    assert!(!after.visible);
    assert_eq!(after.solid_color().unwrap().to_hex(), "0000FF");
    let rows = rows_of(&board, &[r], "fills");
    assert_eq!(rows[0].kind, solid("#0000FF", 0.25).kind);
    assert!(!rows[0].visible);
}

/// Review focus 4: a gradient or an image is never silently rewritten as a solid.
#[test]
fn editing_one_solid_keeps_gradient_neighbour() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": [{"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}, "#112233"]}),
    );
    let before = held(&board, r, "fills");
    assert!(matches!(before[0].kind, Held::Gradient(_)));
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [gradient("Linear"), solid("#112233", 1.0)]
    );

    let edit = set_solid(&read(&board, &[r]), "fills", 1, "#FF8800", 1.0).unwrap();
    apply(&mut board, &edit);

    let after = held(&board, r, "fills");
    assert_eq!(after[0], before[0], "the gradient is identical afterwards");
    assert!(matches!(after[0].kind, Held::Gradient(_)));
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [gradient("Linear"), solid("#FF8800", 1.0)]
    );

    // Adding a paint after it and removing one beside it leave the gradient as it is too.
    let edit = add_paint(&read(&board, &[r]), "fills");
    apply(&mut board, &edit);
    let edit = remove_paint(&read(&board, &[r]), "fills", 1);
    apply(&mut board, &edit);
    assert_eq!(held(&board, r, "fills")[0], before[0]);
}

/// An image's rotation and a pattern's source are integers in the engine, and the panel reads
/// every number as a float: they still go back as they were.
#[test]
fn image_and_pattern_neighbours_survive_an_edit() {
    let mut board = design_file();
    let image = image_layer(&mut board);
    let source = rectangle(&mut board, "Tile", 300.0);
    let patterned = rectangle(&mut board, "Patterned", 600.0);
    node_set(
        &mut board,
        &[patterned],
        json!({"fills": [{"type": "pattern", "source": source}, "#112233"]}),
    );
    assert_eq!(rows_of(&board, &[image], "fills")[0].kind, PaintKind::Image);
    assert_eq!(
        rows_of(&board, &[patterned], "fills")[0].kind,
        PaintKind::Other
    );

    // A solid is added after the image.
    let image_before = held(&board, image, "fills");
    let edit = add_paint(&read(&board, &[image]), "fills");
    apply(&mut board, &edit);
    let after = held(&board, image, "fills");
    assert_eq!(after.len(), 2);
    assert_eq!(after[0], image_before[0]);

    // A solid is edited after the pattern.
    let pattern_before = held(&board, patterned, "fills");
    let edit = set_solid(&read(&board, &[patterned]), "fills", 1, "#FF0000", 1.0).unwrap();
    apply(&mut board, &edit);
    let after = held(&board, patterned, "fills");
    assert_eq!(after[0], pattern_before[0]);
    assert!(matches!(after[0].kind, Held::Pattern(_)));
    assert_eq!(
        rows_of(&board, &[patterned], "fills")[1],
        solid("#FF0000", 1.0)
    );
}

#[test]
fn bad_hex_is_rejected() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    let sel = read(&board, &[r]);
    for bad in [
        "#12",
        "12",
        "",
        "#",
        "   ",
        "#GGGGGG",
        "#12345",
        "#1234567",
        // Alpha is the opacity's business.
        "#FF00",
        "#FF000080",
        "red",
        "#ff ff00",
        "0x112233",
        "##112233",
        "#11223",
        // Digits of another script are no hex digits.
        "#\u{ff11}\u{ff12}\u{ff13}\u{ff14}\u{ff15}\u{ff16}",
    ] {
        assert_eq!(set_solid(&sel, "fills", 0, bad, 1.0), None, "{bad:?}");
        assert_eq!(parse_hex(bad), None, "{bad:?}");
    }
    // Nothing was edited, and the row is as it was.
    assert_eq!(rows_of(&board, &[r], "fills"), [solid("#D9D9D9", 1.0)]);
}

#[test]
fn hex_comes_in_the_forms_a_designer_types() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    for typed in ["#ff8800", "FF8800", "#f80", "f80", "  #Ff8800 ", "\t#F80\n"] {
        let edit = set_solid(&read(&board, &[r]), "fills", 0, typed, 1.0).unwrap();
        apply(&mut board, &edit);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#FF8800", 1.0)],
            "{typed:?}"
        );
        assert_eq!(parse_hex(typed).as_deref(), Some("#FF8800"), "{typed:?}");
    }
    assert_eq!(parse_hex("#000"), Some("#000000".into()));
    assert_eq!(parse_hex("fFf"), Some("#FFFFFF".into()));
}

#[test]
fn opacity_is_kept_between_0_and_1_and_must_be_a_number() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    for (given, kept) in [(0.5, 0.5), (1.5, 1.0), (-0.5, 0.0), (0.0, 0.0), (1.0, 1.0)] {
        let edit = set_solid(&read(&board, &[r]), "fills", 0, "#FF0000", given).unwrap();
        apply(&mut board, &edit);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#FF0000", kept)],
            "{given}"
        );
    }
    let sel = read(&board, &[r]);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(set_solid(&sel, "fills", 0, "#FF0000", bad), None, "{bad}");
    }
}

#[test]
fn set_solid_edits_only_a_solid_row_that_is_there() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": [{"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}, "#112233"]}),
    );
    let sel = read(&board, &[r]);
    // The gradient is not a solid, so no colour turns it into one.
    assert_eq!(set_solid(&sel, "fills", 0, "#FF0000", 1.0), None);
    // There is no third row.
    assert_eq!(set_solid(&sel, "fills", 2, "#FF0000", 1.0), None);
    assert_eq!(set_solid(&sel, "fills", usize::MAX, "#FF0000", 1.0), None);
    assert!(set_solid(&sel, "fills", 1, "#FF0000", 1.0).is_some());

    // Nobody selected has no row either, and nor do layers that disagree.
    assert_eq!(
        set_solid(&read(&board, &[]), "fills", 0, "#FF0000", 1.0),
        None
    );
    let other = rectangle(&mut board, "Other", 300.0);
    let mixed = read(&board, &[r, other]);
    assert_eq!(set_solid(&mixed, "fills", 0, "#FF0000", 1.0), None);
}
