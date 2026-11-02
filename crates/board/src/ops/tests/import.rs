//! File ▸ Import from file… and Place image.

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::design::export::{ExportRow, export_bytes};
use crate::doc::BoardError;
use crate::ops::elements::page_layers;

/// One red rectangle, 100 × 80, as an SVG file holds it.
const RECT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect width="100" height="80" fill="#f00"/></svg>"##;

/// A board whose view shows page point (300.5, 200) in its middle.
fn board_seen_about_its_point() -> BoardDoc {
    let mut board = board();
    let view = json!({"zoom": 1, "vw": 1000, "vh": 800, "cx": 300.5, "cy": 200});
    board.execute("view.set", &view).unwrap();
    board
}

/// Layer `id`'s top left corner and size on the page.
fn placed(board: &BoardDoc, id: NodeId) -> (f64, f64, f64, f64) {
    let bounds = board.doc().unwrap().world_bounds(id).unwrap();
    (bounds.x0, bounds.y0, bounds.width(), bounds.height())
}

/// File ▸ Import and Place image: what they make is centred in the view, as a paste is, and
/// selected, one undo step each.
#[test]
fn an_imported_file_is_centred_in_the_view() {
    let dir = tempfile::tempdir().unwrap();
    // A 160 × 90 PNG, as Export makes one.
    let mut design = design();
    let rectangle = json!({"type": "rectangle", "width": 160, "height": 90});
    let rectangle = made(&mut design, "node.create", rectangle);
    let png = export_bytes(&mut design, &[rectangle], &ExportRow::default()).unwrap();
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, png).unwrap();
    // An SVG file, whatever the case of its extension.
    let logo = dir.path().join("Logo.SVG");
    std::fs::write(&logo, RECT_SVG).unwrap();

    let mut board = board_seen_about_its_point();
    let [image] = import_file(&mut board, &photo, false).unwrap()[..] else {
        panic!("no image")
    };
    // Its middle on the view's, its corner on whole units.
    assert_eq!(placed(&board, image), (221.0, 155.0, 160.0, 90.0));
    assert_eq!(board.session.selection(), [image]);
    let name = |board: &BoardDoc, id| board.doc().unwrap().node(id).unwrap().name.clone();
    assert_eq!(name(&board, image), "photo");
    let [frame] = import_file(&mut board, &logo, false).unwrap()[..] else {
        panic!("no frame")
    };
    assert_eq!(placed(&board, frame), (251.0, 160.0, 100.0, 80.0));
    let [again] = import_file(&mut board, &photo, true).unwrap()[..] else {
        panic!("no placed image")
    };
    assert_eq!(placed(&board, again), (221.0, 155.0, 160.0, 90.0));
    assert_eq!(name(&board, again), "photo");
    assert_eq!(board.session.selection(), [again]);
    assert_eq!(board.session.undo_depth().0, 3);
}

#[test]
fn a_file_that_cannot_be_imported_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut board = board();
    let missing = dir.path().join("missing.png");
    let error = import_file(&mut board, &missing, false).unwrap_err();
    assert!(matches!(error, BoardError::Io(_)), "{error:?}");
    // Place image takes images only.
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();
    assert!(import_file(&mut board, &notes, true).is_err());
    assert!(page_layers(&board).unwrap().is_empty());
    assert_eq!(board.session.undo_depth(), (0, 0));
}
