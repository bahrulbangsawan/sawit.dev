//! The rows of the page list.

use super::*;

#[test]
fn pages_list_and_dividers() {
    let mut board = file(Document::new("Design"));
    let first = board.session.page().unwrap();
    board.execute("page.add", &json!({"name": "Two"})).unwrap();
    board.execute("page.add", &json!({"name": "-"})).unwrap();
    // Adding a page opens it, a divider doesn't open.
    let current = board.session.page().unwrap();
    let doc = board.doc().unwrap();
    assert_eq!(doc.pages.len(), 3);
    assert_eq!(doc.pages[0], first);
    assert_eq!(current, doc.pages[1]);

    let rows = pages(&doc, current);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), doc.pages);
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["Page 1", "Two", "-"]);
    let current: Vec<bool> = rows.iter().map(|row| row.current).collect();
    assert_eq!(current, [false, true, false]);
    let divider: Vec<bool> = rows.iter().map(|row| row.divider).collect();
    assert_eq!(divider, [false, false, true]);
}

#[test]
fn a_page_with_layers_is_not_a_divider_whatever_its_name() {
    let mut board = file(Document::new("Design"));
    board
        .execute("page.add", &json!({"name": "Notes"}))
        .unwrap();
    board
        .execute(
            "node.create",
            &json!({"type": "rectangle", "width": 10, "height": 10}),
        )
        .unwrap();
    let notes = board.session.page().unwrap();
    board
        .execute("page.rename", &json!({"id": notes, "name": "-notes"}))
        .unwrap();
    let doc = board.doc().unwrap();
    let rows = pages(&doc, notes);
    assert_eq!(rows[1].name, "-notes");
    // The engine opens it, so the panel must offer it.
    assert!(!rows[1].divider);
    board
        .execute("page.select", &json!({"id": doc.pages[0]}))
        .unwrap();
    board.execute("page.select", &json!({"id": notes})).unwrap();
}
