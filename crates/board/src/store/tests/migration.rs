//! `.excalidraw` files and the old canvas: imported, bound to the `.sawit` file beside them.

use std::path::{Path, PathBuf};

use sawit_core::nyawit;
use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::doc::{BoardError, DocKind};

/// The old canvas as Excalidraw saved it: nine layers on the page, a frame's child among the
/// ones inside, an embed that can't be imported, and a deleted rectangle.
const FIXTURE: &str = include_str!("../../../tests/fixtures/excalidraw/basic.excalidraw");

#[test]
fn an_excalidraw_file_is_bound_to_the_sawit_file_beside_it() {
    let bound = |path: &str| bound_file(Path::new(path));
    assert_eq!(bound("/p/sketch.excalidraw"), Path::new("/p/sketch.sawit"));
    assert_eq!(bound("/p/v1.2.Excalidraw"), Path::new("/p/v1.2.sawit"));
    // The old project canvas is bound to the project canvas.
    assert_eq!(
        bound("/p/.nyawit/canvas.excalidraw"),
        nyawit::board_file(Path::new("/p"))
    );
    // A canvas file is bound to itself.
    assert_eq!(bound("/p/board.sawit"), Path::new("/p/board.sawit"));
}

#[test]
fn importing_a_file_draws_it_on_a_board_named_after_it_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sketch.excalidraw");
    std::fs::write(&path, FIXTURE).unwrap();

    let (board, issues) = import_file(&path).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    assert_eq!(top_level(&board), 9);
    assert_eq!(board.doc().unwrap().name, "sketch");
    assert_eq!(issues, ["skipped 1 embed"]);
    // It opens as a file does: nothing to undo.
    assert_eq!(board.session.undo_depth(), (0, 0));
    // The file is as it was, and nothing is made beside it.
    assert_eq!(std::fs::read(&path).unwrap(), FIXTURE.as_bytes());
    assert_eq!(names(dir.path()), ["sketch.excalidraw"]);
}

#[test]
fn a_file_with_nothing_in_it_imports_blank_and_any_other_failure_names_it() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sketch.excalidraw");
    for blank in [
        &b""[..],
        b" \n",
        br#"{"type": "excalidraw", "elements": []}"#,
    ] {
        std::fs::write(&path, blank).unwrap();
        let (board, issues) = import_file(&path).unwrap();
        assert!(is_empty(&board.doc().unwrap()), "{blank:?}");
        assert!(issues.is_empty(), "{blank:?}: {issues:?}");
    }
    for (scene, says) in [
        (&b"{ not json"[..], "isn't valid JSON"),
        (br#"{"type": "excalidraw"}"#, "`elements`"),
    ] {
        std::fs::write(&path, scene).unwrap();
        let error = import_file(&path).err().unwrap();
        assert!(matches!(error, BoardError::Format(_)), "{error:?}");
        let message = error.to_string();
        assert!(
            message.contains("sketch.excalidraw") && message.contains(says),
            "{message}"
        );
    }
    assert_eq!(names(dir.path()), ["sketch.excalidraw"]);
}

#[test]
fn migration_imports_once_and_keeps_original() {
    let dir = tempdir().unwrap();
    let nyawit = dir.path().join(".nyawit");
    std::fs::create_dir(&nyawit).unwrap();
    let old = nyawit.join("canvas.excalidraw");
    let path = nyawit.join("canvas.sawit");
    std::fs::write(&old, FIXTURE).unwrap();

    // The first open imports the old canvas. Nothing is written: not the new file, and the
    // old one is as it was.
    let mut board = open(&path).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    assert_eq!(top_level(&board), 9);
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
    assert!(!path.exists());
    assert_eq!(names(&nyawit), ["canvas.excalidraw"]);

    // The first save writes the native canvas, and from then on that is what opens.
    board
        .execute("board.sticky", &json!({"x": 2000, "y": 0, "text": "after"}))
        .unwrap();
    let writer = Writer::spawn();
    writer.save(path.clone(), board.to_bytes().unwrap(), false);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());

    // Not imported again: the nine layers and the sticky, not the old canvas a second time.
    let again = open(&path).unwrap();
    assert!(has_text(&again, "after"));
    assert_eq!(top_level(&again), 10);
    assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.sawit"]);
}

/// A project folder whose `.nyawit/` holds the old canvas `scene`: the folder, the old canvas
/// and the canvas file that isn't there.
fn project_with_old_canvas(scene: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempdir().unwrap();
    let nyawit = dir.path().join(".nyawit");
    std::fs::create_dir(&nyawit).unwrap();
    let old = nyawit.join("canvas.excalidraw");
    std::fs::write(&old, scene).unwrap();
    (dir, old, nyawit.join("canvas.sawit"))
}

#[test]
fn migration_says_what_it_left_out_and_makes_no_backup() {
    let (dir, old, path) = project_with_old_canvas(FIXTURE.as_bytes());
    let nyawit = dir.path().join(".nyawit");

    let (board, issues) = open_with_issues(&path).unwrap();
    assert_eq!(top_level(&board), 9);
    // The notes of the import are what is shown, as for any file that opens with something said.
    assert_eq!(issues, ["skipped 1 embed"]);
    // Neither file is written, and nothing is kept beside them: there is no `.bak`, because
    // the old canvas isn't touched and no canvas file was there to save over.
    assert_eq!(names(&nyawit), ["canvas.excalidraw"]);
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());

    // A canvas the import left nothing out of opens without a word.
    let clean = br#"{"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}]}"#;
    let (dir, _, path) = project_with_old_canvas(clean);
    let (board, issues) = open_with_issues(&path).unwrap();
    assert_eq!(top_level(&board), 1);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
}

#[test]
fn a_migrated_canvas_opens_as_a_file_does_with_no_undo_step() {
    let (_dir, _, path) = project_with_old_canvas(FIXTURE.as_bytes());
    let board = open(&path).unwrap();
    assert_eq!(board.kind(), DocKind::Board);
    // Undoing is for what the user did after it opened: it can't take the canvas away.
    assert_eq!(board.session.undo_depth(), (0, 0));
    assert!(board.session.selection().is_empty());
    // The document is a board's, and what a save writes of it opens as the same canvas.
    let doc = board.doc().unwrap();
    assert_eq!(doc.pages.len(), 1);
    let again = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
    assert_eq!(again.kind(), DocKind::Board);
    assert_eq!(top_level(&again), 9);
    assert_eq!(again.doc().unwrap().nodes().count(), doc.nodes().count());
}

#[test]
fn only_the_canvas_file_takes_over_from_the_old_canvas() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("canvas.excalidraw"), FIXTURE).unwrap();

    // Any other file that is missing is a new board, even beside a `canvas.excalidraw`.
    let other = open(&dir.path().join("other.sawit")).unwrap();
    assert_eq!(top_level(&other), 0);
    let named_alike = open(&dir.path().join("my-canvas.sawit")).unwrap();
    assert_eq!(top_level(&named_alike), 0);
    // The canvas of a window with no project is `canvas.sawit` in Sawit's own folder, with
    // the old one beside it.
    let app = open(&board_file(None, dir.path())).unwrap();
    assert_eq!(top_level(&app), 9);
    assert_eq!(names(dir.path()), ["canvas.excalidraw"]);

    // No old canvas, or one that is a folder, is nothing to import.
    let empty = tempdir().unwrap();
    assert_eq!(
        top_level(&open(&empty.path().join("canvas.sawit")).unwrap()),
        0
    );
    std::fs::create_dir(empty.path().join("canvas.excalidraw")).unwrap();
    let (board, issues) = open_with_issues(&empty.path().join("canvas.sawit")).unwrap();
    assert_eq!(top_level(&board), 0);
    assert!(issues.is_empty());
}

#[test]
fn the_old_canvas_is_never_imported_over_a_canvas_file_that_is_there() {
    let (_dir, old, path) = project_with_old_canvas(FIXTURE.as_bytes());
    std::fs::write(&path, bytes_with(&["native"])).unwrap();
    let board = open(&path).unwrap();
    assert!(has_text(&board, "native"));
    assert_eq!(top_level(&board), 1);

    // Not when the canvas file is corrupt either: it stays as it is, and so does the old one.
    std::fs::write(&path, b"not a canvas file").unwrap();
    assert!(matches!(open(&path), Err(BoardError::Format(_))));
    assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas file");
    assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
}

#[test]
fn an_old_canvas_with_nothing_on_it_opens_blank() {
    let deleted = br#"{"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "isDeleted": true}]}"#;
    // A scene with nothing drawn, and a file with nothing in it at all: empty, only white
    // space, or only a byte order mark. Today's canvas opens each of them blank, and there is
    // nothing in them to protect.
    let lone_bom = "\u{feff}".as_bytes();
    let spaced_bom = "\u{feff} \r\n\t".as_bytes();
    for scene in [
        &deleted[..],
        br#"{"elements": []}"#,
        b"",
        b" \n",
        lone_bom,
        spaced_bom,
    ] {
        let (dir, old, path) = project_with_old_canvas(scene);
        for (board, issues) in [
            open_with_issues(&path).unwrap(),
            (open(&path).unwrap(), Vec::new()),
        ] {
            assert_eq!(board.kind(), DocKind::Board, "{scene:?}");
            assert_eq!(top_level(&board), 0, "{scene:?}");
            assert!(issues.is_empty(), "{scene:?}: {issues:?}");
            assert!(is_empty(&board.doc().unwrap()));
            // Opened as a file is: nothing to undo.
            assert_eq!(board.session.undo_depth(), (0, 0));
        }
        // Neither file is written, and nothing is kept beside them.
        assert!(!path.exists(), "{scene:?}");
        assert_eq!(std::fs::read(&old).unwrap(), scene);
        assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
    }
}

#[test]
fn an_old_canvas_that_cannot_be_imported_is_an_error_and_nothing_is_made() {
    // A new board would take its place at the first save, and hide it.
    let invalid_utf8 = [0xFF, 0xFE, 0x00];
    for (scene, kind, says) in [
        (&b"{ not json"[..], "format", "isn't valid JSON"),
        (&br#"{"type": "excalidraw"}"#[..], "format", "`elements`"),
        (&br#"[1, 2]"#[..], "format", "`elements`"),
        (&invalid_utf8[..], "io", "couldn't read"),
    ] {
        let (dir, old, path) = project_with_old_canvas(scene);
        for error in [
            open(&path).err().unwrap(),
            open_with_issues(&path).err().unwrap(),
        ] {
            let right_kind = match kind {
                "format" => matches!(error, BoardError::Format(_)),
                _ => matches!(error, BoardError::Io(_)),
            };
            assert!(right_kind, "{error:?}");
            let message = error.to_string();
            assert!(
                message.contains("canvas.excalidraw") && message.contains(says),
                "{message}"
            );
        }
        assert!(!path.exists());
        assert_eq!(std::fs::read(&old).unwrap(), scene);
        assert_eq!(names(&dir.path().join(".nyawit")), ["canvas.excalidraw"]);
    }
}

/// A canvas tab opens an `.excalidraw` file imported, bound to the `.sawit` file beside it
/// (spec D14), and so does sawit-mcp with the app closed: the scene while that file isn't
/// there, and that file once a save has made it, so the scene is imported once. Neither file
/// is written by opening.
#[test]
fn an_excalidraw_file_opens_imported_until_its_sawit_file_is_there() {
    let dir = tempdir().unwrap();
    let scene = dir.path().join("sketch.excalidraw");
    std::fs::write(&scene, FIXTURE).unwrap();
    let board = open_bound(&scene).unwrap();
    assert_eq!(top_level(&board), 9);
    assert_eq!(board.doc().unwrap().name, "sketch");
    assert_eq!(names(dir.path()), ["sketch.excalidraw"]);

    // Saved once, edited since: the `.sawit` file is what opens, not the scene again.
    std::fs::write(dir.path().join("sketch.sawit"), bytes_with(&["edited"])).unwrap();
    let board = open_bound(&scene).unwrap();
    assert_eq!(top_level(&board), 1);
    assert!(has_text(&board, "edited"));
    assert_eq!(std::fs::read(&scene).unwrap(), FIXTURE.as_bytes());

    // A `.sawit` file opens itself, as `open` opens it: one that isn't there is a new board.
    let file = dir.path().join("board.sawit");
    assert_eq!(top_level(&open_bound(&file).unwrap()), 0);
    assert!(!file.exists());
    // A scene that can't be imported is the import's error, and nothing is made.
    let broken = dir.path().join("broken.excalidraw");
    std::fs::write(&broken, b"{ not json").unwrap();
    let error = open_bound(&broken).err().unwrap().to_string();
    assert!(error.contains("broken.excalidraw"), "{error}");
    assert!(!dir.path().join("broken.sawit").exists());
}
