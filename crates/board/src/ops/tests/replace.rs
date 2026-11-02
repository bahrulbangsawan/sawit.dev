//! The `replace` op with a canvas file, and `file.import` of one.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::doc::DocKind;

/// A board file of two pages, a sticky on each.
fn two_page_file(dir: &std::path::Path) -> String {
    let mut other = BoardDoc::new_board("Two pages").unwrap();
    other
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "first"}))
        .unwrap();
    other
        .execute("page.add", &json!({"name": "Second"}))
        .unwrap();
    other
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "second"}))
        .unwrap();
    let path = dir.join("two.sawit");
    std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}

/// A `.sawit` file is handed over by its bytes, named as the engine knows a canvas file, which
/// it would otherwise place as an image.
#[test]
fn replace_loads_a_sawit_file_by_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = other_board_file(dir.path());
    let mut board = board();
    let reply = ask(&mut board, json!({"op": "replace", "path": path}));
    let now = elements(&mut board);
    assert_eq!(now.len(), 2, "{path}: {now:?}");
    assert!(now.iter().all(|e| e["text"] == json!("other")), "{now:?}");
    assert_eq!(reply["elements"], json!(now));
    // The engine-name shim: the engine reads a canvas file only by its own extension.
    assert_eq!(engine_name("other.sawit"), "other.figcraft");
    assert_eq!(engine_name("Other.SAWIT"), "Other.figcraft");
    assert_eq!(
        engine_name("canvas.sawit.bak.2"),
        "canvas.sawit.bak.2.figcraft"
    );
    for name in ["page.png", "logo.svg", "notes.sawitx"] {
        assert_eq!(engine_name(name), name);
    }
}

/// Regression: a file named with the engine's own extension is no canvas file Sawit reads.
/// `replace`, `file.import` run as a command (in a batch too) and Import from file… refuse
/// it, unread, and the canvas is as it was.
#[test]
fn a_file_by_the_engines_own_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.figcraft");
    std::fs::copy(other_board_file(dir.path()), &path).unwrap();
    let mut board = board();

    let error = refusal(&mut board, json!({"op": "replace", "path": path}));
    assert!(
        error.contains("is not a .sawit or .excalidraw file"),
        "{error}"
    );
    let reply = send(&mut board, "file.import", json!({"path": path}));
    let error = reply["error"].as_str().unwrap_or_default();
    assert!(error.contains("name a .sawit file"), "{reply}");
    let batch = json!({"commands": [{"command": "file.import", "params": {"path": path}}]});
    let reply = send(&mut board, "command.batch", batch);
    let error = reply["error"].as_str().unwrap_or_default();
    assert!(error.contains("name a .sawit file"), "{reply}");
    let error = import_file(&mut board, &path, false)
        .unwrap_err()
        .for_user();
    assert!(error.contains("choose a .sawit file"), "{error}");

    assert_eq!(board.doc().unwrap().pages.len(), 1);
    assert!(elements(&mut board).is_empty());
}

/// `file.import` run as a command reads a `.sawit` file it names as a canvas file, as the
/// steps of a batch do: its pages come in after the canvas's own, where the engine alone would
/// have placed the file as an image. A `.sawit` file that isn't there is the error.
#[test]
fn file_import_of_a_sawit_file_brings_its_pages_in() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    let mut board = board();
    let reply = send(&mut board, "file.import", json!({"path": other}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(board.doc().unwrap().pages.len(), 2, "{reply}");

    let batch = json!({"commands": [{"command": "file.import", "params": {"path": other}}]});
    let reply = send(&mut board, "command.batch", batch);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(board.doc().unwrap().pages.len(), 3, "{reply}");

    let missing = dir.path().join("missing.sawit");
    let reply = send(&mut board, "file.import", json!({"path": missing}));
    let error = reply["error"].as_str().unwrap_or_default();
    assert!(error.starts_with("file.import: couldn't read"), "{reply}");
    assert_eq!(board.doc().unwrap().pages.len(), 3);
}

#[test]
fn replace_loads_a_sawit_file_as_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    // Two pages of its own: both are replaced.
    board
        .execute("page.add", &json!({"name": "Notes"}))
        .unwrap();
    board.execute("page.select", &json!({"index": 0})).unwrap();
    let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

    let reply = ask(&mut board, json!({"op": "replace", "path": other}));

    // The canvas shows the file's page, and says what is on it as `get` does.
    let now = elements(&mut board);
    assert_eq!(now.len(), 2, "{now:?}");
    assert!(now.iter().all(|e| e["text"] == json!("other")), "{now:?}");
    assert_eq!(reply["elements"], json!(now));
    assert_eq!(board.doc().unwrap().pages.len(), 1);
    assert!(board.session.selection().is_empty());
    // In the canvas's own session: one document, the one it had, and still a board.
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(board.session.ds().unwrap().uid, uid);
    assert_eq!(board.kind(), DocKind::Board);
    // One step: undo brings the old board back, pages and all, and redo the file's.
    assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
    assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!(board.doc().unwrap().pages.len(), 2);
    let back = elements(&mut board);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0]["text"], json!("mine"));
    ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
    assert_eq!(elements(&mut board).len(), 2);
}

#[test]
fn replace_takes_every_page_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = two_page_file(dir.path());
    let mut board = board();
    add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));

    ask(&mut board, json!({"op": "replace", "path": path}));

    // The first page is on show, and the old page is gone.
    let doc = board.doc().unwrap();
    let names: Vec<&str> = doc
        .pages
        .iter()
        .map(|page| doc.node(*page).unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["Page 1", "Second"]);
    let now = elements(&mut board);
    assert_eq!(now.len(), 1);
    assert_eq!(now[0]["text"], json!("first"));
}

#[test]
fn replace_keeps_a_design_file_a_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = design();
    file.execute(
        "node.create",
        &json!({"type": "frame", "x": 0, "y": 0, "name": "Screen"}),
    )
    .unwrap();
    let path = dir.path().join("design.sawit");
    std::fs::write(&path, file.to_bytes().unwrap()).unwrap();
    let mut board = board();

    let reply = ask(&mut board, json!({"op": "replace", "path": path}));

    assert_eq!(board.kind(), DocKind::Board);
    assert_eq!(reply["elements"][0]["name"], json!("Screen"));
    assert_eq!(reply["elements"][0]["type"], json!("frame"));
}

#[test]
fn replace_of_a_missing_or_bad_file_is_an_error_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.sawit");
    std::fs::write(&bad, b"not a canvas file").unwrap();
    let missing = dir.path().join("missing.sawit");
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

    let error = refusal(&mut board, json!({"op": "replace", "path": missing}));
    assert!(
        error.starts_with("replace:") && error.contains("missing.sawit"),
        "{error}"
    );
    let error = refusal(&mut board, json!({"op": "replace", "path": bad}));
    assert!(error.starts_with("replace:"), "{error}");

    // Not a new, empty board over the canvas: it is as it was.
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), steps);
    assert_eq!(elements(&mut board).len(), 1);
}

/// CANVAS-CORE-6, for an agent's load (`canvas_load`): a `replace` of a canvas file has
/// the engine read the file `path` names, whole and as it finds it, on the canvas's UI thread
/// (with the app closed, in sawit-mcp, holding every other canvas call back). A named pipe
/// blocked it for ever, and a link a cloned repository holds to `/dev/zero` read until the Mac
/// ran out of memory. Both are refused at once, as when a canvas opens such a file, and the
/// canvas is as it was; an `.excalidraw` file is, as before.
#[test]
fn replace_of_a_pipe_or_a_device_is_refused_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let pipe = dir.path().join("board.sawit");
    crate::store::pipe_at(&pipe);
    let scene = dir.path().join("scene.excalidraw");
    crate::store::pipe_at(&scene);
    let refused_at_once = |paths: &[&std::path::Path]| {
        let failed: Vec<String> = (paths.iter())
            .filter_map(|path| {
                let shown = path.display().to_string();
                let replaced = on_a_board_of_its_own(json!({"op": "replace", "path": path}));
                let Some((reply, unchanged)) = replaced else {
                    return Some(format!("{shown}: no answer within 5 s"));
                };
                let error = reply["error"].as_str().unwrap_or_default();
                let refused = error.starts_with(&format!("replace: couldn't read {shown}"))
                    && error.contains("not a regular file");
                (!refused || !unchanged).then(|| format!("{shown}: {reply}"))
            })
            .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    };
    // The pipes first: a device that never ends is read only once they are refused.
    refused_at_once(&[&pipe, &scene]);
    let zero = dir.path().join("zero.sawit");
    std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
    refused_at_once(&[&zero]);
}

#[test]
fn replace_needs_a_scene_or_a_file_of_a_kind_it_knows() {
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    let before = board.doc().unwrap();
    // No scene and no path, or a path that isn't text, runs nothing.
    for op in [
        json!({"op": "replace"}),
        json!({"op": "replace", "path": 5}),
        json!({"op": "replace", "scene": null}),
    ] {
        let error = refusal(&mut board, op.clone());
        assert!(error.starts_with("replace needs"), "{op}: {error}");
    }
    for path in ["/tmp/board.png", "/tmp/board.bak", "/tmp/board"] {
        let error = refusal(&mut board, json!({"op": "replace", "path": path}));
        let expected = format!("replace: {path} is not a .sawit or .excalidraw file");
        assert_eq!(error, expected);
    }
    // A copy kept of a canvas file that isn't there says so.
    let missing = "/nowhere/canvas.sawit.bak";
    let error = refusal(&mut board, json!({"op": "replace", "path": missing}));
    assert!(
        error.starts_with(&format!("replace: couldn't read {missing}: ")),
        "{error}"
    );
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (1, 0));
}

/// MCP-6: the copies Sawit keeps of a canvas file (`canvas.sawit.bak` and the older
/// `.bak.<n>`, which a clear or a load keeps of the board it takes off, `.del.bak` and the
/// older `.del.bak.<n>`, which an agent's deletion keeps, and the `.lossy.bak` of a file
/// opened with notes) are canvas files under another name, and load as the file they were:
/// that is how an agent brings back a board it cleared. Refused by its name, a backup could
/// only be loaded once someone copied it to a `.sawit` name; and passed on by its path,
/// the engine, which tells a file's kind by its name, would place it as an image.
#[test]
fn replace_loads_a_copy_kept_of_a_canvas_file() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    for name in [
        "canvas.sawit.bak",
        "canvas.sawit.bak.2",
        "canvas.sawit.del.bak",
        "canvas.sawit.del.bak.3",
        "canvas.sawit.lossy.bak",
        "Board.SAWIT.BAK",
    ] {
        let kept = dir.path().join(name);
        std::fs::copy(&other, &kept).unwrap();
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let reply = ask(&mut board, json!({"op": "replace", "path": kept}));
        let now = elements(&mut board);
        assert_eq!(now.len(), 2, "{name}: {now:?}");
        assert!(
            now.iter().all(|e| e["text"] == json!("other")),
            "{name}: {now:?}"
        );
        assert_eq!(reply["elements"], json!(now), "{name}");
        assert_eq!(board.kind(), DocKind::Board);
    }
    // Read as a canvas file is checked: a named pipe at such a name is refused, unread.
    let pipe = dir.path().join("pipe.sawit.bak");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.unwrap().success(), "mkfifo {}", pipe.display());
    let error = refusal(&mut board(), json!({"op": "replace", "path": pipe}));
    assert!(error.contains("not a regular file"), "{error}");
}
