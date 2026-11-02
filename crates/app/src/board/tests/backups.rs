//! The backups an agent's clear, load or deletion keeps of the board it took layers off
//! (CANVAS-CORE-3).

use super::*;

/// The bytes of the backups beside the canvas file `path` that clears, loads and saves that
/// empty the canvas keep: `<name>.bak` and the older ones.
fn backups(path: &Path) -> Vec<Vec<u8>> {
    kept_as(path, ".bak")
}

/// The bytes of the backups beside the canvas file `path` that an agent's deletions keep:
/// `<name>.del.bak` and the older ones.
fn deletion_backups(path: &Path) -> Vec<Vec<u8>> {
    kept_as(path, ".del.bak")
}

/// The bytes of the files beside `path` named `<name><suffix>`, then `<name><suffix>.<n>`.
fn kept_as(path: &Path, suffix: &str) -> Vec<Vec<u8>> {
    let dir = path.parent().unwrap();
    let name = format!("{}{suffix}", path.file_name().unwrap().to_string_lossy());
    names(dir)
        .into_iter()
        .filter(|n| n.starts_with(&name))
        .map(|n| std::fs::read(dir.join(n)).unwrap())
        .collect()
}

/// The layers of the canvases in `backups`, as `get` lists them: each one's texts.
fn backed_up_texts(backups: &[Vec<u8>]) -> Vec<Vec<String>> {
    backups
        .iter()
        .map(|bytes| {
            let mut board = BoardDoc::from_bytes(bytes).unwrap();
            let reply = sawit_board::ops::run(&mut board, &json!({"op": "get"}));
            let elements = reply["elements"].as_array().unwrap();
            let texts = elements.iter().filter_map(|e| e["text"].as_str());
            texts.map(str::to_string).collect()
        })
        .collect()
}

/// CANVAS-CORE-3: an agent's clear or load in the app is undone by ⌘Z, but only while the tab
/// is open: undo isn't saved. Each keeps the board it took off the canvas as a backup, the
/// ones before it too (a clear, a drawing and a clear left only the drawing; a load kept
/// nothing), and that is the board as the canvas showed it, an edit made a moment before
/// included (the clear's backup was of the file, which the autosave hadn't written yet).
#[gpui_kit::test]
fn a_clear_or_a_load_keeps_the_board_it_took_off(cx: &mut TestAppContext) {
    let tmp = Scratch::new("backups");
    let path = tmp.0.join("canvas.sawit");
    write_board(&path, &board_with(&["the user's"]));
    let view = open(cx, &path);
    let ask = |op: Value, cx: &mut TestAppContext| {
        let reply = run_op(&view, op, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
    };
    let flush = |cx: &mut TestAppContext| view.update(cx, |view, _| view.flush());
    let sticky = |text: &str| json!({"op": "add", "elements": [{"type": "sticky", "x": 400, "y": 0, "text": text}]});
    // An edit, then at once a clear.
    ask(sticky("added"), cx);
    ask(json!({"op": "clear"}), cx);
    flush(cx);
    let kept = backed_up_texts(&backups(&path));
    let with_the_edit = vec!["the user's".to_string(), "added".to_string()];
    assert!(kept.contains(&with_the_edit), "{kept:?}");
    // A drawing, then a clear again: the first board is still kept.
    ask(sticky("drawn"), cx);
    flush(cx);
    ask(json!({"op": "clear"}), cx);
    flush(cx);
    let kept = backed_up_texts(&backups(&path));
    assert!(kept.contains(&with_the_edit), "{kept:?}");
    assert!(kept.contains(&vec!["drawn".to_string()]), "{kept:?}");
    // A load in its place keeps what it replaced.
    ask(sticky("before the load"), cx);
    flush(cx);
    let scene = json!({"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}]});
    ask(json!({"op": "replace", "scene": scene}), cx);
    flush(cx);
    let kept = backed_up_texts(&backups(&path));
    assert!(
        kept.contains(&vec!["before the load".to_string()]),
        "{kept:?}"
    );
}

/// An agent's op that deletes layers keeps the board it took them off as a backup, as a clear
/// does: its `delete`, an engine command such as `edit.delete`, and a clear of a board with a
/// second page, which leaves the canvas with layers (only a save that emptied the canvas kept
/// one). Undo isn't saved, so after the tab closes the backup is all that is left of them.
/// Deletions keep theirs apart (`.del.bak`), so that they never push out the board a clear
/// kept. The user's own deletions keep none: they have undo, and a backup at each ⌫ would soon
/// push the agents' out of the five kept.
#[gpui_kit::test]
fn an_agents_deletion_keeps_the_board_it_took_layers_off(cx: &mut TestAppContext) {
    let tmp = Scratch::new("deletion-backups");
    let path = tmp.0.join("canvas.sawit");
    write_board(&path, &board_with(&["first", "second", "third", "fourth"]));
    let view = open(cx, &path);
    let ask = |op: Value, cx: &mut TestAppContext| {
        let reply = run_op(&view, op, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
    };
    let flush = |cx: &mut TestAppContext| view.update(cx, |view, _| view.flush());
    let node_of = |text: &str, cx: &mut TestAppContext| -> u64 {
        let got = run_op(&view, json!({"op": "get"}), cx);
        let elements = got["elements"].as_array().unwrap();
        let found = elements.iter().find(|e| e["text"] == json!(text)).unwrap();
        found["id"].as_str().unwrap().parse().unwrap()
    };
    let texts = |texts: &[&str]| texts.iter().map(|t| t.to_string()).collect::<Vec<_>>();

    // The user deletes a layer by hand: the save keeps no backup.
    let fourth = node_of("fourth", cx);
    view.update(cx, |view, _| {
        let board = view.board_mut().unwrap();
        board
            .execute("edit.delete", &json!({"ids": [fourth]}))
            .unwrap();
    });
    flush(cx);
    assert!(backups(&path).is_empty() && deletion_backups(&path).is_empty());

    // An agent's delete, then its edit.delete: each keeps the board it deleted from, as a
    // deletion's backup.
    let first = node_of("first", cx);
    ask(json!({"op": "delete", "ids": [first.to_string()]}), cx);
    flush(cx);
    let kept = backed_up_texts(&deletion_backups(&path));
    assert_eq!(kept, [texts(&["first", "second", "third"])]);
    let second = node_of("second", cx);
    let params = json!({"ids": [second]});
    ask(
        json!({"op": "command", "command": "edit.delete", "params": params}),
        cx,
    );
    flush(cx);
    let kept = backed_up_texts(&deletion_backups(&path));
    assert!(kept.contains(&texts(&["second", "third"])), "{kept:?}");
    assert_eq!(kept.len(), 2);
    assert!(
        backups(&path).is_empty(),
        "a deletion kept a clear's backup"
    );

    // A clear of the first page while a second page keeps its sticky.
    let page = |command: &str, params: Value| json!({"op": "command", "command": command, "params": params});
    ask(page("page.add", json!({"name": "Two"})), cx);
    ask(sticky_op(), cx);
    ask(page("page.select", json!({"index": 0})), cx);
    ask(json!({"op": "clear"}), cx);
    flush(cx);
    assert_eq!(saved_layers(&path), 0, "the first page is cleared");
    let kept = backups(&path);
    assert_eq!(kept.len(), 1);
    assert_eq!(deletion_backups(&path).len(), 2);
    // The newest, whichever page it opens on.
    let newest = BoardDoc::from_bytes(&kept[0]).unwrap().doc().unwrap();
    let held =
        |text: &str| (newest.nodes()).any(|n| n.kind.text().is_some_and(|t| t.characters == text));
    assert!(
        held("third"),
        "the cleared sticky isn't in the newest backup"
    );
}

/// An agent's load of the backup a clear just kept (`canvas_load` of `canvas.sawit.bak`, at
/// once after its `canvas_clear`) loads the board the clear took off: the load waits for the
/// saves queued before it, and the backup one keeps, to be on disk before it reads the file, as
/// with the app closed. It read the backup from before, or none, while they were still queued.
#[gpui_kit::test]
fn a_load_of_the_backup_a_clear_just_kept_gets_that_board(cx: &mut TestAppContext) {
    let tmp = Scratch::new("load-backup");
    let path = tmp.0.join("canvas.sawit");
    write_board(&path, &board_with(&["the user's"]));
    let view = open(cx, &path);
    let ask = |op: Value, cx: &mut TestAppContext| {
        let reply = run_op(&view, op, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
    };
    let sticky =
        json!({"op": "add", "elements": [{"type": "sticky", "x": 400, "y": 0, "text": "added"}]});
    ask(sticky, cx);
    ask(json!({"op": "clear"}), cx);
    let backup = tmp.0.join("canvas.sawit.bak");
    ask(json!({"op": "replace", "path": backup}), cx);
    let got = run_op(&view, json!({"op": "get"}), cx);
    let texts: Vec<&str> = got["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["text"].as_str())
        .collect();
    assert!(
        texts.contains(&"the user's") && texts.contains(&"added"),
        "the load got {texts:?}"
    );
}
