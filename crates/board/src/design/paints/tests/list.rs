//! Adding and removing paints, showing and hiding one, on every selected layer, and the edits
//! that are refused.

use super::*;
use crate::design::lists::testing::{bind, bound, set_variable, variables};
use crate::doc::BoardError;

#[test]
fn add_and_remove_paint() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);

    // A fill goes after the fills there are.
    let edit = add_paint(&read(&board, &[r]), "fills");
    assert_eq!(edit.command, "node.set");
    apply(&mut board, &edit);
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [solid("#D9D9D9", 1.0), solid("#D9D9D9", 1.0)]
    );
    // A stroke where there was none, in black.
    assert_eq!(rows_of(&board, &[r], "strokes"), []);
    let edit = add_paint(&read(&board, &[r]), "strokes");
    apply(&mut board, &edit);
    assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#000000", 1.0)]);
    assert_eq!(rows_of(&board, &[r], "fills").len(), 2, "the fills stay");

    // A removed row leaves the rows around it.
    node_set(
        &mut board,
        &[r],
        json!({"fills": ["#FF0000", "#00FF00", "#0000FF"]}),
    );
    let edit = remove_paint(&read(&board, &[r]), "fills", 1);
    apply(&mut board, &edit);
    assert_eq!(
        rows_of(&board, &[r], "fills"),
        [solid("#FF0000", 1.0), solid("#0000FF", 1.0)]
    );
    // Until none is left: an empty list is not a mixed one.
    let edit = remove_paint(&read(&board, &[r]), "fills", 0);
    apply(&mut board, &edit);
    let edit = remove_paint(&read(&board, &[r]), "fills", 0);
    apply(&mut board, &edit);
    assert_eq!(paints(&read(&board, &[r]), "fills"), Some(vec![]));
    assert_eq!(rows_of(&board, &[r], "strokes").len(), 1);
}

/// Final review m1: removing a paint keeps the variable bindings of the rows after it, which
/// move down one with their rows, on every selected layer, as one undo step; for the fills
/// and the strokes. The variables then still colour the rows they were bound to.
#[test]
fn removing_a_paint_keeps_the_bindings_of_the_rows_after_it() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    node_set(
        &mut board,
        &[r1, r2],
        json!({"fills": ["#FF0000", "#00FF00", "#0000FF"], "strokes": ["#111111", "#222222"]}),
    );
    variables(
        &mut board,
        &[
            ("brand", "color", json!("#0000FF")),
            ("line", "color", json!("#222222")),
        ],
    );
    for id in [r1, r2] {
        bind(&mut board, id, "fills/2/color", "brand");
        bind(&mut board, id, "strokes/1/color", "line");
    }
    let steps = undo_steps(&board);

    let edit = remove_paint(&read(&board, &[r1, r2]), "fills", 0);
    assert_eq!(edit.command, "fill.remove");
    apply(&mut board, &edit);
    let edit = remove_paint(&read(&board, &[r1, r2]), "strokes", 0);
    assert_eq!(edit.command, "stroke.remove");
    apply(&mut board, &edit);
    assert_eq!(undo_steps(&board), steps + 2);
    set_variable(&mut board, "brand", json!("#ABCDEF"));
    set_variable(&mut board, "line", json!("#FEDCBA"));
    for id in [r1, r2] {
        assert_eq!(bound(&board, id), ["fills/1/color", "strokes/0/color"]);
        assert_eq!(
            rows_of(&board, &[id], "fills"),
            [solid("#00FF00", 1.0), solid("#ABCDEF", 1.0)]
        );
        assert_eq!(rows_of(&board, &[id], "strokes"), [solid("#FEDCBA", 1.0)]);
    }
}

#[test]
fn paint_edits_apply_to_every_selected_layer_in_one_undo_step() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    let sel = read(&board, &[r1, r2]);
    let steps = undo_steps(&board);

    let edit = set_solid(&sel, "fills", 0, "#FF0000", 0.5).unwrap();
    apply(&mut board, &edit);
    assert_eq!(undo_steps(&board), steps + 1);
    for id in [r1, r2] {
        assert_eq!(rows_of(&board, &[id], "fills"), [solid("#FF0000", 0.5)]);
    }

    let edit = add_paint(&read(&board, &[r1, r2]), "strokes");
    apply(&mut board, &edit);
    assert_eq!(undo_steps(&board), steps + 2);
    for id in [r1, r2] {
        assert_eq!(rows_of(&board, &[id], "strokes"), [solid("#000000", 1.0)]);
    }

    board.execute("edit.undo", &json!({})).unwrap();
    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(undo_steps(&board), steps);
    for id in [r1, r2] {
        assert_eq!(rows_of(&board, &[id], "fills"), [solid("#D9D9D9", 1.0)]);
        assert_eq!(rows_of(&board, &[id], "strokes"), []);
    }
}

#[test]
fn replace_all_makes_one_new_paint_the_only_paint_of_every_layer() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    node_set(&mut board, &[r2], json!({"fills": ["#FF0000", "#0000FF"]}));
    let sel = read(&board, &[r1, r2]);
    assert_eq!(paints(&sel, "fills"), None);
    let steps = undo_steps(&board);

    let edit = add_paint(&sel, "fills");
    assert_eq!(edit.params["ids"], json!([r1, r2]));
    apply(&mut board, &edit);
    assert_eq!(undo_steps(&board), steps + 1);
    assert_eq!(rows_of(&board, &[r1, r2], "fills"), [solid("#D9D9D9", 1.0)]);

    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(rows_of(&board, &[r1], "fills"), [solid("#D9D9D9", 1.0)]);
    assert_eq!(
        rows_of(&board, &[r2], "fills"),
        [solid("#FF0000", 1.0), solid("#0000FF", 1.0)]
    );
}

/// With no list to change a row of, or no such row, an edit says so by being refused.
#[test]
fn edits_that_cannot_apply_are_refused_and_change_nothing() {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 300.0);
    node_set(&mut board, &[r2], json!({"fills": ["#FF0000"]}));
    let one = read(&board, &[r1]);
    let differing = read(&board, &[r1, r2]);

    let edits = [
        remove_paint(&one, "fills", 1),
        remove_paint(&one, "fills", usize::MAX),
        replace_with_solid(&one, "fills", 1),
        set_visible(&one, "fills", 1, false),
        remove_paint(&one, "strokes", 0),
        remove_paint(&one, "effects", 0),
        remove_paint(&differing, "fills", 0),
        replace_with_solid(&differing, "fills", 0),
        set_visible(&differing, "fills", 0, false),
    ];
    let snapshot = read(&board, &[r1, r2]);
    let steps = undo_steps(&board);
    for edit in edits {
        let revision = board.revision();
        let result = board.execute(edit.command, &edit.params);
        assert!(
            matches!(
                result,
                Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
            ),
            "{edit:?}"
        );
        assert_eq!(board.revision(), revision, "{edit:?}");
    }
    assert_eq!(read(&board, &[r1, r2]), snapshot);
    assert_eq!(undo_steps(&board), steps);
}

#[test]
fn set_visible_shows_and_hides_one_row_and_changes_nothing_else() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    node_set(
        &mut board,
        &[r],
        json!({"fills": ["#FF0000", {"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}]}),
    );
    let before = held(&board, r, "fills");

    let edit = set_visible(&read(&board, &[r]), "fills", 1, false);
    apply(&mut board, &edit);
    let rows = rows_of(&board, &[r], "fills");
    assert!(rows[0].visible);
    assert_eq!(rows[1].kind, gradient("Linear").kind);
    assert!(!rows[1].visible);
    let after = held(&board, r, "fills");
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1].kind, before[1].kind);
    assert!(!after[1].visible);

    let edit = set_visible(&read(&board, &[r]), "fills", 1, true);
    apply(&mut board, &edit);
    assert_eq!(held(&board, r, "fills"), before);
}
