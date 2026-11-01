//! Which items are enabled: the commands that can run, and the ones that apply now.

use super::*;
use crate::policy::is_ui;

#[test]
fn items_are_enabled_when_the_engine_or_sawit_runs_them() {
    assert!(available("edit.duplicate"));
    // Sawit carries these out itself, so the engine has no command for them.
    assert!(available("file.export"));
    assert!(available("edit.copyAsPng"));
    assert!(!available("no.such"));
    assert!(!available(""));
    // Every item the context menus show can run. The main menu's can too, but what doesn't
    // apply now.
    let selected = board_with_sticky();
    for (menu, all) in [
        (main_menu(&selected.session), false),
        (context_menu(&selected.session), true),
        (context_menu(&board().session), true),
    ] {
        for node in lists(&menu).into_iter().flatten() {
            if let MenuNode::Item { id, enabled, .. } = node {
                let applies = applies(&selected.session, id);
                assert_eq!(*enabled, all || applies, "{id}");
                assert!(available(id), "{id}");
            }
        }
    }
}

/// Whether the item for `id` in `nodes` is enabled.
fn enabled(nodes: &[MenuNode], id: &str) -> bool {
    match item(nodes, id) {
        MenuNode::Item { enabled, .. } => *enabled,
        _ => unreachable!(),
    }
}

/// The user's screenshot: Object ▸ Frame selection with nothing selected said "command
/// `object.frameSelection` is not available right now: nothing is selected (pass `ids`)".
/// What has nothing to act on is greyed out instead.
#[test]
fn items_that_do_not_apply_now_are_greyed_out() {
    let mut empty = board();
    let menu = main_menu(&empty.session);
    for id in [
        "object.frameSelection",
        "object.group",
        "edit.duplicate",
        "edit.copyAsText",
        "text.bold",
        "align.left",
        "select.sameFill",
        "vector.deleteHeal",
        "edit.undo",
        "edit.redo",
    ] {
        assert!(!enabled(&menu, id), "{id} is enabled with nothing selected");
    }
    // What needs no selection stays enabled.
    for id in ["select.all", "view.zoomFit", "file.export", "edit.paste"] {
        assert!(enabled(&menu, id), "{id}");
    }
    // With a layer selected they apply, and making it is a step to undo.
    sticky(&mut empty, 0);
    let menu = main_menu(&empty.session);
    for id in [
        "object.frameSelection",
        "object.group",
        "edit.duplicate",
        "edit.copyAsText",
        "text.bold",
        "align.left",
        "select.sameFill",
        "edit.undo",
    ] {
        assert!(
            enabled(&menu, id),
            "{id} is disabled with a sticky selected"
        );
    }
    // Undone, it can be done again.
    empty.execute("edit.undo", &json!({})).unwrap();
    assert!(enabled(&main_menu(&empty.session), "edit.redo"));
    // What needs a dialog the canvas doesn't have, or never applies on a canvas, never is.
    let selected = board_with_sticky();
    let menu = main_menu(&selected.session);
    for id in NEEDS_INPUT.iter().chain(NEVER_ON_THE_CANVAS) {
        assert!(!applies(&selected.session, id), "{id}");
        if ids(&menu).contains(id) {
            assert!(!enabled(&menu, id), "{id}");
        }
    }
}

/// While text is edited, copy and paste and the Text menu's styles act on its characters.
#[test]
fn text_commands_apply_while_text_is_edited() {
    let mut board = board();
    let made = board
        .execute("text.create", &json!({"x": 0, "y": 0, "text": "hi"}))
        .unwrap();
    let id = made["id"].clone();
    board
        .execute("text.edit.begin", &json!({"id": id}))
        .unwrap();
    board.session.set_selection(Vec::new());
    for id in ["edit.copy", "edit.paste", "text.bold", "edit.undo"] {
        assert!(applies(&board.session, id), "{id}");
    }
    assert!(!applies(&board.session, "object.group"));
}

/// [`NEEDS_SELECTION`] is what the engine refuses with nothing selected: on an empty board and
/// on an empty design file, every item of the menus the engine refuses so is on it, and every
/// engine command on it is refused so. Only the Select all with same commands, which then
/// select nothing, and Sawit's own Copy as are on it by Sawit's choice. A FigCraft bump that
/// changes what needs a selection fails here.
#[test]
fn needs_selection_is_what_the_engine_refuses_with_nothing_selected() {
    let design = || BoardDoc {
        session: Session::with_document(figcraft_doc::Document::new("Design")),
    };
    let by_choice = |id: &str| id.starts_with("select.same") || is_ui(id);
    let refused_for_the_selection = |error: &crate::BoardError| match error {
        crate::BoardError::Unavailable { reason, .. } => reason.starts_with("nothing is selected"),
        // "select an instance", "select one layer".
        other => other.for_user().starts_with("Select "),
    };
    for make in [board as fn() -> BoardDoc, design] {
        let menu_ids = ids(&main_menu(&make().session));
        for id in menu_ids {
            if is_ui(id) || crate::policy::is_clipboard(id) || NEEDS_INPUT.contains(&id) {
                continue;
            }
            let mut doc = make();
            let refused = doc
                .execute(id, &json!({}))
                .err()
                .filter(refused_for_the_selection);
            assert_eq!(
                NEEDS_SELECTION.contains(&id),
                refused.is_some() || by_choice(id),
                "{id}: {refused:?}"
            );
        }
    }
    // Every id on the list is one the menus show: a misspelt one would grey out nothing.
    let shown = ids(&main_menu(&board().session));
    for id in NEEDS_SELECTION
        .iter()
        .chain(NEEDS_INPUT)
        .chain(NEVER_ON_THE_CANVAS)
    {
        assert!(shown.contains(id), "{id} is in no menu");
    }
}
