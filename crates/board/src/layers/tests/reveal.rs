//! The layers opened to show a selection.

use super::*;

#[test]
fn reveal_expands_ancestors() {
    let t = tree();
    let doc = t.doc();
    assert_eq!(reveal(&doc, &[t.c]), set(&[t.f, t.g]));
    // The page is no row, a layer opens its ancestors and not itself, and layers add up.
    assert_eq!(reveal(&doc, &[t.g]), set(&[t.f]));
    assert_eq!(reveal(&doc, &[t.f]), set(&[]));
    assert_eq!(reveal(&doc, &[t.a, t.c]), set(&[t.f, t.g]));
    assert_eq!(reveal(&doc, &[]), set(&[]));
    assert_eq!(reveal(&doc, &[NodeId(9_999), t.page]), set(&[]));
}

#[test]
fn reveal_shows_the_selection_in_the_rows() {
    let t = tree();
    let doc = t.doc();
    let open = reveal(&doc, &[t.c]);
    let rows = rows(&doc, t.page, &open, &[t.c], "");
    assert!(row(&rows, t.c).selected);
}

#[test]
fn reveal_skips_board_items_that_hide_their_insides() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let section = make(
        &mut board,
        "node.create",
        json!({"type": "section", "x": 0, "y": 0, "width": 600, "height": 600}),
    );
    let sticky = make(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "hi", "parent": section}),
    );
    let doc = board.doc().unwrap();
    assert_eq!(doc.node(sticky).unwrap().parent, Some(section));
    let text = doc.node(sticky).unwrap().children[0];
    // The note can't open, so opening it would show nothing; the section it is in opens.
    assert_eq!(reveal(&doc, &[text]), set(&[section]));
    assert_eq!(reveal(&doc, &[sticky]), set(&[section]));
}
