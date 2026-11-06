//! Comment threads.

use super::*;
use crate::design::lists::testing::{apply, design_file};

#[test]
fn a_thread_is_started_answered_resolved_and_deleted() {
    let mut board = design_file();
    apply(&mut board, &add(100.0, 50.0, "Is this the brand blue?"));
    let doc = board.doc().unwrap();
    let page = board.session.page().unwrap();
    let list = threads(&doc, page);
    assert_eq!(list.len(), 1);
    let id = list[0].id;
    assert_eq!(list[0].at, (100.0, 50.0));
    assert_eq!(list[0].messages[0].text, "Is this the brand blue?");
    assert_eq!(pin_at(&doc, page, 104.0, 52.0, 10.0), Some(id));
    assert_eq!(pin_at(&doc, page, 200.0, 52.0, 10.0), None);

    apply(&mut board, &reply(id, "Yes, #0D99FF"));
    let doc = board.doc().unwrap();
    assert_eq!(threads(&doc, page)[0].messages.len(), 2);
    assert_eq!(pins(&doc, page), [(id, (100.0, 50.0), 1)]);
    apply(&mut board, &resolve(id, true));
    let doc = board.doc().unwrap();
    assert!(threads(&doc, page)[0].resolved);
    assert!(pins(&doc, page).is_empty(), "a resolved thread has no pin");
    apply(&mut board, &delete(id));
    assert!(threads(&board.doc().unwrap(), page).is_empty());
}
