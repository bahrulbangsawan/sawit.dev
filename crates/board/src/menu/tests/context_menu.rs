//! The context menu, over a selection and over the empty canvas.

use super::*;

#[test]
fn context_menu_on_selection() {
    let board = board_with_sticky();
    let menu = context_menu(&board.session);
    let shown = ids(&menu);
    for id in ["edit.delete", "arrange.front", "object.toggleVisible"] {
        assert!(shown.contains(&id), "{id} is not in {shown:?}");
    }
    assert_eq!(label_of(&menu, "object.toggleVisible"), "Hide");
    assert_eq!(label_of(&menu, "object.toggleLock"), "Lock");
    assert_eq!(label_of(&menu, "edit.paste"), "Paste here");
    // The spec's list, in its order.
    assert_eq!(
        shown,
        [
            "edit.cut",
            "edit.copy",
            "edit.paste",
            "edit.duplicate",
            "edit.delete",
            "arrange.front",
            "arrange.forward",
            "arrange.backward",
            "arrange.back",
            "object.group",
            "object.ungroup",
            "object.frameSelection",
            "layout.add",
            "object.flipH",
            "object.flipV",
            "object.toggleVisible",
            "object.toggleLock",
            "edit.copyAsText",
            "edit.copyAsCss",
            "edit.copyAsSvg",
            "edit.copyAsPng",
            "select.all",
        ]
    );
    // Copy as is a submenu of Text, CSS, SVG and PNG.
    assert_eq!(
        items(children_of(&menu, "Copy as")),
        [
            ("edit.copyAsText", "Text"),
            ("edit.copyAsCss", "CSS"),
            ("edit.copyAsSvg", "SVG"),
            ("edit.copyAsPng", "PNG"),
        ]
    );
    // The spec's seven groups, with a line between each.
    let lines = outline(&menu).iter().filter(|label| *label == "-").count();
    assert_eq!(lines, 6);
}

#[test]
fn context_menu_on_empty_canvas() {
    let menu = context_menu(&board().session);
    assert_eq!(menu.len(), 4, "{menu:?}");
    assert_eq!(
        items(&menu),
        [
            ("edit.paste", "Paste here"),
            ("select.all", "Select all"),
            ("view.zoomFit", "Zoom to fit"),
            ("view.toggleUi", "Show/Hide UI"),
        ]
    );
}

#[test]
fn context_menu_items_show_their_shortcuts() {
    let selected = board_with_sticky();
    let menu = context_menu(&selected.session);
    assert_eq!(shortcut_of(&menu, "edit.duplicate"), Some("⌘D"));
    assert_eq!(shortcut_of(&menu, "edit.delete"), Some("⌫"));
    assert_eq!(shortcut_of(&menu, "object.toggleVisible"), Some("⇧⌘H"));
    let empty = context_menu(&board().session);
    assert_eq!(shortcut_of(&empty, "view.toggleUi"), Some("⌘\\"));
    assert_eq!(shortcut_of(&empty, "view.zoomFit"), Some("⇧1"));
}

#[test]
fn context_menu_labels_follow_the_selection() {
    let mut board = board();
    let a = sticky(&mut board, 0);
    let b = sticky(&mut board, 300);
    let labels = |board: &BoardDoc| {
        let menu = context_menu(&board.session);
        (
            label_of(&menu, "object.toggleVisible").to_string(),
            label_of(&menu, "object.toggleLock").to_string(),
        )
    };
    let select = |board: &mut BoardDoc, ids: &[NodeId]| {
        board.execute("select.set", &json!({"ids": ids})).unwrap();
    };

    // Both shown and unlocked: the menu hides and locks.
    select(&mut board, &[a, b]);
    assert_eq!(labels(&board), ("Hide".into(), "Lock".into()));

    // One hidden: the engine's toggle shows every layer then, so the menu says Show.
    let params = json!({"ids": [a], "visible": false});
    board.execute("object.toggleVisible", &params).unwrap();
    select(&mut board, &[a, b]);
    assert_eq!(labels(&board).0, "Show");
    select(&mut board, &[a]);
    assert_eq!(labels(&board).0, "Show");
    select(&mut board, &[b]);
    assert_eq!(labels(&board).0, "Hide");

    // One locked: the engine's toggle locks every layer then, so the menu says Lock. Locking
    // deselects, and the layers panel can still select a locked layer.
    let params = json!({"ids": [a], "locked": true});
    board.execute("object.toggleLock", &params).unwrap();
    select(&mut board, &[a, b]);
    assert_eq!(labels(&board).1, "Lock");
    select(&mut board, &[a]);
    assert_eq!(labels(&board).1, "Unlock");
    let params = json!({"ids": [b], "locked": true});
    board.execute("object.toggleLock", &params).unwrap();
    select(&mut board, &[a, b]);
    assert_eq!(labels(&board).1, "Unlock");
}
