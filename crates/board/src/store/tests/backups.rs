//! The backups a save keeps: when it keeps one, of which kind, and how they move along.

use std::path::Path;
use std::sync::Arc;

use figcraft_doc::{Document, NodeKind};
use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::doc::file_with_a_damaged_image;
use crate::store::backup::{lossy_backup, sibling};
use crate::store::writer::{Batch, Msg, Save};

#[test]
fn is_empty_means_no_layers_on_any_page() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    assert!(is_empty(&board.doc().unwrap()));
    let sticky = board
        .execute("board.sticky", &json!({"x": 0, "y": 0}))
        .unwrap();
    assert!(!is_empty(&board.doc().unwrap()));
    board
        .execute("edit.delete", &json!({"ids": [sticky["id"]]}))
        .unwrap();
    assert!(is_empty(&board.doc().unwrap()));

    // A layer on a page other than the first counts too; no pages at all is empty.
    let mut doc = Document::new("Two pages");
    let second = doc.add_page("Page 2");
    assert!(is_empty(&doc));
    doc.create(second, NodeKind::Rectangle, "R", 0.0, 0.0, 10.0, 10.0)
        .unwrap();
    assert!(!is_empty(&doc));
    assert!(is_empty(&Document::without_pages("None")));
}

/// A save takes layers off the canvas when a layer the board had, on any page and at any
/// depth, is gone from what it saves: then the file's board is kept as a backup first. An
/// edit that adds, moves or renames layers takes nothing off, nor does deleting an empty page.
#[test]
fn a_layer_gone_from_any_page_is_a_save_that_takes_layers_off() {
    let mut board = board_with(&["one", "two"]);
    let page = board.doc().unwrap().pages[0];
    let first = board.doc().unwrap().node(page).unwrap().children[0];
    for (command, params, takes_off) in [
        ("board.sticky", json!({"x": 900, "y": 0}), false),
        ("node.set", json!({"id": first, "x": 40, "y": 40}), false),
        ("node.set", json!({"id": first, "name": "First"}), false),
        ("edit.delete", json!({"ids": [first]}), true),
    ] {
        let before = board.doc().unwrap();
        board.execute(command, &params).unwrap();
        let after = board.doc().unwrap();
        assert_eq!(
            removes_layers(&before, &after),
            takes_off,
            "{command} {params}"
        );
    }

    // On another page: a layer in a frame there, the page with it, or an empty page.
    let mut doc = Document::new("Two pages");
    let second = doc.add_page("Page 2");
    let empty = doc.add_page("Page 3");
    let frame = (doc.create(second, NodeKind::Frame, "F", 0.0, 0.0, 400.0, 400.0)).unwrap();
    let nested = (doc.create(frame, NodeKind::Rectangle, "R", 10.0, 10.0, 50.0, 50.0)).unwrap();
    let without = |id| {
        let mut after = doc.clone();
        after.remove(id).unwrap();
        removes_layers(&doc, &after)
    };
    assert!(without(nested), "a nested layer");
    assert!(without(second), "a page with layers on it");
    assert!(!without(empty), "a page with nothing on it");
    assert!(!removes_layers(&doc, &doc.clone()));
}

#[test]
fn emptying_keeps_backup() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["keep me"]), false);
    writer.flush();
    writer.save(path.clone(), blank(), true);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    let backup = open(&sibling(&path, ".bak")).unwrap();
    assert!(!is_empty(&backup.doc().unwrap()));
    assert!(has_text(&backup, "keep me"));
    assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
}

#[test]
fn backups_are_made_only_when_layers_are_emptied() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let bak = sibling(&path, ".bak");
    let writer = Writer::spawn();
    let save = |bytes: Vec<u8>, empty: bool| {
        writer.save(path.clone(), bytes, empty);
        writer.flush();
    };
    // Nothing on disk yet: nothing to keep.
    save(blank(), true);
    assert!(!bak.exists());
    // A save that keeps layers keeps no backup, even over a file with layers.
    save(bytes_with(&["one"]), false);
    let two = bytes_with(&["two"]);
    save(two.clone(), false);
    assert!(!bak.exists());
    // Emptying keeps the file as it was...
    save(blank(), true);
    assert_eq!(std::fs::read(&bak).unwrap(), two);
    // ...and emptying an empty canvas again leaves that backup alone.
    save(blank(), true);
    assert_eq!(std::fs::read(&bak).unwrap(), two);
    assert_eq!(writer.last_error(), None);
}

#[test]
fn clearing_after_a_lossy_open_keeps_the_original() {
    // Opened with notes, edited and saved (the save drops the damaged image), then cleared,
    // as an agent's add then clear does: the emptying save keeps the edited canvas, and the
    // original stays where the open put it.
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let original = file_with_a_damaged_image();
    std::fs::write(&path, &original).unwrap();
    let (_, issues) = open_with_issues(&path).unwrap();
    assert!(!issues.is_empty());

    let writer = Writer::spawn();
    let edited = bytes_with(&["kept", "added"]);
    writer.save(path.clone(), edited.clone(), false);
    writer.flush();
    writer.save(path.clone(), blank(), true);
    writer.flush();
    assert_eq!(writer.last_error(), None);
    assert_eq!(std::fs::read(sibling(&path, ".bak")).unwrap(), edited);
    assert_eq!(std::fs::read(lossy_backup(&path)).unwrap(), original);
}

/// What the backups beside the canvas file `path` hold: `<name>.bak`, then any older one, in
/// the order of their names.
fn backups(path: &Path) -> Vec<Vec<u8>> {
    let dir = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    names(dir)
        .into_iter()
        .filter(|n| n.starts_with(&format!("{name}.bak")))
        .map(|n| std::fs::read(dir.join(n)).unwrap())
        .collect()
}

/// CANVAS-CORE-3: an agent that starts over twice, a clear, a drawing and a clear again, left
/// one `.bak`, of its own drawing: the board that was there first, which the first clear kept,
/// was in no file any more. Each backup now moves the ones before it along, so it is still
/// there.
#[test]
fn clearing_twice_keeps_the_board_cleared_first() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let first = bytes_with(&["the user's board"]);
    std::fs::write(&path, &first).unwrap();
    let writer = Writer::spawn();
    let save = |bytes: Vec<u8>, empty: bool| {
        writer.save(path.clone(), bytes, empty);
        writer.flush();
        assert_eq!(writer.last_error(), None);
    };
    save(blank(), true);
    let drawn = bytes_with(&["the agent's drawing"]);
    save(drawn.clone(), false);
    save(blank(), true);
    let kept = backups(&path);
    assert!(kept.contains(&first), "the first board is in no backup");
    assert!(kept.contains(&drawn), "the drawing is in no backup");
}

/// CANVAS-CORE-3: a save that keeps a backup, of either kind, is never folded into another
/// save of its file, nor the one before it into it: that one is what the backup is of, the
/// board as the canvas last handed it over. Saves between two such saves still fold, the
/// newest written.
#[test]
fn a_batch_writes_a_save_that_keeps_a_backup_and_the_one_before_it() {
    let save = |byte: u8, backup: Backup| {
        Msg::Save(Save {
            path: "a".into(),
            bytes: vec![byte],
            backup,
        })
    };
    let mut batch = Batch::default();
    for msg in [
        save(1, Backup::None),
        save(2, Backup::None),
        save(3, Backup::Board),
        save(4, Backup::None),
        save(5, Backup::None),
        save(6, Backup::Board),
        save(7, Backup::Board),
        save(8, Backup::None),
        save(9, Backup::Deletion),
    ] {
        batch.add(msg);
    }
    let written: Vec<u8> = batch.saves.iter().map(|s| s.bytes[0]).collect();
    assert_eq!(written, [2, 3, 5, 6, 7, 8, 9]);
}

/// CANVAS-CORE-3: the newest backup is `.bak`; each one before it moves one name along, and
/// the oldest of [`BACKUPS`] goes. A board the newest backup holds already moves none along.
#[test]
fn backups_move_along_and_the_oldest_goes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let writer = Writer::spawn();
    let boards: Vec<Vec<u8>> = (0..7)
        .map(|n| bytes_with(&[&format!("board {n}")]))
        .collect();
    for board in &boards {
        writer.save(path.clone(), board.clone(), false);
        writer.save(path.clone(), blank(), true);
        writer.flush();
        assert_eq!(writer.last_error(), None);
    }
    let newest_first: Vec<Vec<u8>> = boards.iter().rev().take(BACKUPS).cloned().collect();
    let kept: Vec<Vec<u8>> = (0..BACKUPS)
        .map(|n| std::fs::read(backup_file(&path, n)).unwrap())
        .collect();
    assert!(
        kept == newest_first,
        "the backups aren't the newest, in order"
    );
    assert!(!backup_file(&path, BACKUPS).exists());
    assert_eq!(
        backup_file(&path, 0).file_name().unwrap(),
        "canvas.sawit.bak"
    );
    assert_eq!(
        backup_file(&path, 4).file_name().unwrap(),
        "canvas.sawit.bak.4"
    );

    // The same board cleared again, saved by another app's undo, say.
    std::fs::write(&path, &boards[6]).unwrap();
    writer.save(path.clone(), blank(), true);
    writer.flush();
    let again: Vec<Vec<u8>> = (0..BACKUPS)
        .map(|n| std::fs::read(backup_file(&path, n)).unwrap())
        .collect();
    assert!(
        again == kept,
        "the backups moved along for a board kept already"
    );
}

/// An agent's deletions keep their backups apart from those of clears and loads:
/// `<name>.del.bak`, then `<name>.del.bak.1` to `.del.bak.4`. Sharing the five `.bak` files,
/// five small deletions after a clear took the board it cleared out of all of them; now only a
/// clear, a load or a save that empties the canvas moves the `.bak` files along.
#[test]
fn deletions_keep_backups_of_their_own_apart_from_a_clears() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let users = bytes_with(&["the user's board"]);
    std::fs::write(&path, &users).unwrap();
    let writer = Writer::spawn();
    let save = |bytes: Vec<u8>, backup: Backup| {
        writer.save_with(path.clone(), bytes, backup);
        writer.flush();
        assert_eq!(writer.last_error(), None);
    };
    save(blank(), Backup::Board);
    // A drawing of eight stickies, then seven deletions of one sticky each: `drawn[n]` is the
    // board with the first `n`.
    let texts: Vec<String> = (0..8).map(|n| format!("sticky {n}")).collect();
    let drawn: Vec<Vec<u8>> = (0..=8)
        .map(|n| bytes_with(&texts[..n].iter().map(String::as_str).collect::<Vec<_>>()))
        .collect();
    save(drawn[8].clone(), Backup::None);
    for n in (1..8).rev() {
        save(drawn[n].clone(), Backup::Deletion);
    }

    assert_eq!(std::fs::read(backup_file(&path, 0)).unwrap(), users);
    assert!(!backup_file(&path, 1).exists());
    // The boards before the five newest deletions, newest first.
    let kept: Vec<Vec<u8>> = (0..BACKUPS)
        .map(|n| std::fs::read(deletion_backup_file(&path, n)).unwrap())
        .collect();
    assert!(
        kept == drawn[2..7],
        "the deletions' backups are out of order"
    );
    assert!(!deletion_backup_file(&path, BACKUPS).exists());
    assert_eq!(
        deletion_backup_file(&path, 0).file_name().unwrap(),
        "canvas.sawit.del.bak"
    );
    assert_eq!(
        deletion_backup_file(&path, 4).file_name().unwrap(),
        "canvas.sawit.del.bak.4"
    );
}

/// The backup the save after an agent's op keeps: the board's for a load, a clear and an op
/// that empties the canvas; a deletion's for an op that takes some layers off and leaves
/// others, on the page on show or another; none for an op that only adds or changes layers.
#[test]
fn the_backup_after_an_agents_op_is_the_boards_or_a_deletions() {
    /// The board before and after `board` runs `command` with `params`.
    fn run(board: &mut BoardDoc, command: &str, params: serde_json::Value) -> [Arc<Document>; 2] {
        let before = board.doc().unwrap();
        board.execute(command, &params).unwrap();
        [before, board.doc().unwrap()]
    }
    let mut board = board_with(&["one", "two"]);
    let page = board.doc().unwrap().pages[0];
    let first = board.doc().unwrap().node(page).unwrap().children[0];
    let [before, added] = run(&mut board, "board.sticky", json!({"x": 900, "y": 0}));
    assert_eq!(backup_after("add", &before, &added), Backup::None);
    assert_eq!(backup_after("replace", &before, &added), Backup::Board);
    let [before, deleted] = run(&mut board, "edit.delete", json!({"ids": [first]}));
    assert_eq!(backup_after("delete", &before, &deleted), Backup::Deletion);
    assert_eq!(backup_after("command", &before, &deleted), Backup::Deletion);
    assert_eq!(backup_after("clear", &before, &deleted), Backup::Board);
    let rest = board.doc().unwrap().node(page).unwrap().children.clone();
    let [before, emptied] = run(&mut board, "edit.delete", json!({ "ids": rest }));
    assert!(is_empty(&emptied));
    assert_eq!(backup_after("delete", &before, &emptied), Backup::Board);
    // The stronger of two backups asked for between two saves is the one kept.
    assert_eq!(Backup::Deletion.max(Backup::Board), Backup::Board);
    assert_eq!(Backup::None.max(Backup::Deletion), Backup::Deletion);
}

/// CANVAS-CORE-3: a save the changed-on-disk guard refuses changes nothing: not the file, and
/// not the backups either. The backup used to be made before the check, and replaced the one
/// there with the other app's file.
#[test]
fn a_refused_save_leaves_the_backups_as_they_were() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("design.sawit");
    let writer = Writer::spawn();
    writer.save(path.clone(), bytes_with(&["kept"]), false);
    writer.flush();
    writer.save(path.clone(), blank(), true);
    writer.flush();
    let before = backups(&path);
    assert_eq!(before.len(), 1);
    std::fs::write(&path, bytes_with(&["read"])).unwrap();
    writer.guard(path.clone(), Stamp::of(&path));
    let theirs = bytes_with(&["theirs", "and more"]);
    std::fs::write(&path, &theirs).unwrap();

    writer.save(path.clone(), blank(), true);
    writer.flush();
    let error = writer.last_error().unwrap();
    assert!(error.contains("changed on disk"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert!(backups(&path) == before, "the backup was replaced");
}

#[test]
fn emptying_keeps_a_file_that_does_not_load() {
    // A canvas file another app left unreadable (cut short, or from a newer Sawit): an
    // emptying save can't tell that it had no layers, so it keeps it, as the old canvas
    // kept a scene it couldn't parse. An empty file has nothing to keep.
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let bak = sibling(&path, ".bak");
    let writer = Writer::spawn();
    let save_blank = || {
        writer.save(path.clone(), blank(), true);
        writer.flush();
    };
    std::fs::write(&path, b"").unwrap();
    save_blank();
    assert!(!bak.exists());

    let unreadable = bytes_with(&["cut short"])[..100].to_vec();
    assert!(figcraft_format::load_bytes(&unreadable).is_err());
    std::fs::write(&path, &unreadable).unwrap();
    save_blank();
    assert_eq!(writer.last_error(), None);
    assert_eq!(std::fs::read(&bak).unwrap(), unreadable);
    assert!(is_empty(&open(&path).unwrap().doc().unwrap()));
}

#[test]
fn a_failed_backup_stops_the_emptying_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("canvas.sawit");
    let full = bytes_with(&["keep me"]);
    let writer = Writer::spawn();
    writer.save(path.clone(), full.clone(), false);
    writer.flush();
    // A folder where the backup should go.
    std::fs::create_dir(sibling(&path, ".bak")).unwrap();
    writer.save(path.clone(), blank(), true);
    writer.flush();
    let error = writer.last_error().unwrap();
    assert!(error.contains("canvas.sawit.bak"), "{error}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        full,
        "the canvas is as it was"
    );
}
