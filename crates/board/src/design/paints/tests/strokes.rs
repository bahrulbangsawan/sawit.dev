//! The strokes, edited as the fills are, and the Stroke section's other fields.

use super::*;
use crate::design::Mixed;

#[test]
fn strokes_are_edited_like_fills() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    let edit = add_paint(&read(&board, &[r]), "strokes");
    apply(&mut board, &edit);
    let fills = held(&board, r, "fills");

    let edit = set_solid(&read(&board, &[r]), "strokes", 0, "#336699", 0.75).unwrap();
    apply(&mut board, &edit);
    assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#336699", 0.75)]);
    let edit = set_visible(&read(&board, &[r]), "strokes", 0, false);
    apply(&mut board, &edit);
    assert!(!rows_of(&board, &[r], "strokes")[0].visible);
    let edit = remove_paint(&read(&board, &[r]), "strokes", 0);
    apply(&mut board, &edit);
    assert_eq!(rows_of(&board, &[r], "strokes"), []);
    assert_eq!(
        held(&board, r, "fills"),
        fills,
        "the fills were not touched"
    );
}

#[test]
fn stroke_fields_are_the_keys_the_section_reads_and_writes() {
    assert_eq!(stroke_fields(), ["strokeWeight", "strokeAlign", "dashes"]);

    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    // Without a stroke there is no weight or alignment to show, and the dashes are none.
    let sel = read(&board, &[r]);
    assert_eq!(sel.get("strokeWeight"), Mixed::Absent);
    assert_eq!(sel.get("strokeAlign"), Mixed::Absent);
    assert_eq!(sel.get("dashes"), Mixed::Same(json!([])));

    let edit = add_paint(&sel, "strokes");
    apply(&mut board, &edit);
    for key in stroke_fields() {
        let value = read(&board, &[r]).get(key);
        assert!(matches!(value, Mixed::Same(_)), "{key}: {value:?}");
    }
    assert_eq!(
        read(&board, &[r]).get("strokeWeight"),
        Mixed::Same(json!(1.0))
    );

    // Each is written back under the name it is read under.
    for (key, value) in [
        ("strokeWeight", json!(3.0)),
        ("strokeAlign", json!("center")),
        ("dashes", json!([4.0, 4.0])),
        ("dashes", json!([])),
    ] {
        let edit = read(&board, &[r]).set(key, value.clone());
        apply(&mut board, &edit);
        assert_eq!(read(&board, &[r]).get(key), Mixed::Same(value), "{key}");
    }
}
