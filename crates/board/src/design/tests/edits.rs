//! The edits the panel makes: `node.set` on the selected layers, the align and distribute
//! buttons, and the text keys, which go to the text layers only.

use super::*;
use crate::doc::BoardError;

#[test]
fn editing_mixed_x_moves_all_in_one_step() {
    let mut f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    let steps = f.board.session.undo_depth().0;

    let edit = sel.set("x", json!(10.0));
    apply(&mut f.board, &edit);
    assert_eq!(f.board.session.undo_depth().0, steps + 1);
    assert_eq!(
        read(&f.board, &[f.r1, f.r2]).get("x"),
        same(json!(10.0)),
        "both layers moved"
    );

    f.board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(f.board.session.undo_depth().0, steps);
    assert_eq!(read(&f.board, &[f.r1]).get("x"), same(json!(0.0)));
    assert_eq!(read(&f.board, &[f.r2]).get("x"), same(json!(50.0)));
}

#[test]
fn an_edit_is_node_set_on_the_selected_layers() {
    let f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(
        sel.set("opacity", json!(0.5)),
        Edit {
            command: "node.set",
            params: json!({"ids": [f.r1, f.r2], "props": {"opacity": 0.5}}),
        }
    );
}

#[test]
fn align_left_runs() {
    let mut f = fixture();
    apply(
        &mut f.board,
        &Edit {
            command: "select.set",
            params: json!({"ids": [f.r1, f.r2]}),
        },
    );
    let edit = align("align.left");
    assert_eq!(edit.command, "align.left");
    apply(&mut f.board, &edit);
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(sel.get("x"), same(json!(0.0)), "both left edges are one");
}

#[test]
fn align_and_distribute_carry_no_ids() {
    for command in [
        "align.left",
        "align.hcenter",
        "align.right",
        "align.top",
        "align.vcenter",
        "align.bottom",
        "distribute.horizontal",
        "distribute.vertical",
    ] {
        assert_eq!(
            align(command),
            Edit {
                command,
                params: json!({}),
            }
        );
    }
}

#[test]
fn a_text_key_goes_to_the_text_layers_only() {
    let mut f = fixture();
    let sel = read(&f.board, &[f.r1, f.text]);
    let keys = [
        "characters",
        "fontFamily",
        "fontStyle",
        "fontSize",
        "lineHeight",
        "letterSpacing",
        "textAlign",
        "textAlignVertical",
        "textAutoResize",
    ];
    for key in keys {
        assert_eq!(
            sel.set(key, json!("x")).params["ids"],
            json!([f.text]),
            "{key}"
        );
    }
    // Every other key still goes to every layer.
    for key in [
        "x",
        "opacity",
        "fills",
        "width",
        "constrainProportions",
        "gap",
    ] {
        assert_eq!(
            sel.set(key, json!(1.0)).params["ids"],
            json!([f.r1, f.text]),
            "{key}"
        );
    }

    // So the engine, which refuses a text key on a rectangle, takes the edit, and the
    // rectangle stays as it was.
    let before = read(&f.board, &[f.r1]);
    let edit = sel.set("fontSize", json!(18.0));
    apply(&mut f.board, &edit);
    assert_eq!(read(&f.board, &[f.text]).get("fontSize"), same(json!(18.0)));
    assert_eq!(read(&f.board, &[f.r1]), before);
}

#[test]
fn a_text_key_without_a_text_layer_is_refused_and_changes_nothing() {
    let mut f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    let edit = sel.set("fontSize", json!(18.0));
    assert_eq!(edit.params["ids"], json!([]));

    let revision = f.board.revision();
    let refused = f.board.execute(edit.command, &edit.params);
    assert!(
        matches!(
            refused,
            Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(f.board.revision(), revision);
    assert_eq!(read(&f.board, &[f.r1, f.r2]), sel);
}
