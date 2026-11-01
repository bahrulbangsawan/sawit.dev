//! Keys while text is edited, and ending the edit before a command.

use figcraft_engine::textedit;

use super::*;

/// A new sticky reading `text`, 200 × 200 with its top left corner at page point (`x`, 0).
fn sticky_at(board: &mut BoardDoc, x: f64, text: &str) -> NodeId {
    let made = board
        .execute("board.sticky", &json!({"x": x, "y": 0, "text": text}))
        .unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

/// Starts typing in sticky `note` as a double-click does, all of its text selected, and types
/// `typed` in place of it.
fn type_into(board: &mut BoardDoc, note: NodeId, typed: &str) {
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    textedit::insert(&mut board.session, typed).unwrap();
    assert_eq!(sticky_text(board, note), typed);
}

/// The engine's gesture that is open, if one is: typing holds one, "Edit text".
fn gesture(board: &BoardDoc) -> Option<String> {
    let ds = board.session.ds().unwrap();
    ds.interaction.as_ref().map(|gesture| gesture.label.clone())
}

#[test]
fn clipboard_keys_copy_and_paste_the_text_being_edited() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    let shift_cmd = Mods { shift: true, ..CMD };
    // ⌘C, ⌘X and ⌘V copy, cut and paste the characters, as in any text field, and so does
    // ⇧⌘V, as in FigCraft's text editor.
    for (key, mods, id) in [
        ("c", CMD, "edit.copy"),
        ("x", CMD, "edit.cut"),
        ("v", CMD, "edit.paste"),
        ("v", shift_cmd, "edit.pasteOver"),
    ] {
        assert_eq!(
            route_key(&mut board, key, mods),
            Ok(KeyOutcome::Ui(id)),
            "{id}"
        );
    }
    // Pasting to replace the selected layers waits, like the other canvas shortcuts.
    assert_eq!(
        route_key(&mut board, "r", shift_cmd),
        Ok(KeyOutcome::Unhandled)
    );
    assert_eq!(sticky_text(&board, note), "hi");
    assert!(editing(&board));
}

#[test]
fn typing_is_left_to_the_ime_while_editing() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    // Editing starts with all of "hi" selected.
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    assert_eq!(route_key(&mut board, "a", NONE), Ok(KeyOutcome::Unhandled));
    assert_eq!(
        route_key(&mut board, "Space", NONE),
        Ok(KeyOutcome::Unhandled)
    );
    assert_eq!(sticky_text(&board, note), "hi");
    // Backspace is the editor's: it deletes the selection, and the edit goes on.
    assert_eq!(
        route_key(&mut board, "Backspace", NONE),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(sticky_text(&board, note), "");
    assert!(editing(&board));
}

#[test]
fn next_sticky_asks_to_edit_the_new_sticky() {
    let mut board = board();
    let first = sticky(&mut board, "");
    assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
    let shift_cmd = Mods { shift: true, ..CMD };
    assert_eq!(
        route_key(&mut board, "Enter", shift_cmd),
        Ok(KeyOutcome::Handled)
    );
    let selection = board.session.selection();
    let [next] = selection.as_slice() else {
        panic!("selection {selection:?}")
    };
    assert_ne!(*next, first);
    let edits: Vec<Option<NodeId>> = board
        .session
        .ui_requests
        .iter()
        .filter(|r| r.kind == "board.editText")
        .map(|r| figcraft_engine::params::id_of(&r.data["id"]))
        .collect();
    assert_eq!(edits, [Some(*next)]);
}

#[test]
fn cmd_a_selects_the_text_being_edited() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    // Right collapses the selection to a caret at the end.
    assert_eq!(
        route_key(&mut board, "ArrowRight", NONE),
        Ok(KeyOutcome::Handled)
    );
    let edit = text_edit(&board);
    assert_eq!((edit.anchor, edit.caret), (2, 2));
    let layers = board.session.selection();
    assert_eq!(route_key(&mut board, "a", CMD), Ok(KeyOutcome::Handled));
    let edit = text_edit(&board);
    assert_eq!((edit.anchor, edit.caret), (0, 2));
    assert_eq!(board.session.selection(), layers);
}

#[test]
fn cmd_enter_ends_the_edit() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    assert_eq!(route_key(&mut board, "Enter", CMD), Ok(KeyOutcome::Handled));
    assert!(!editing(&board));
    // No newline replaced the selected text.
    assert_eq!(sticky_text(&board, note), "hi");
}

#[test]
fn canvas_shortcuts_wait_while_editing() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    let doc = board.doc().unwrap();
    // ⌘D duplicates and ⌘G groups layers on the canvas.
    assert_eq!(route_key(&mut board, "d", CMD), Ok(KeyOutcome::Unhandled));
    assert_eq!(route_key(&mut board, "g", CMD), Ok(KeyOutcome::Unhandled));
    assert_eq!(board.doc().unwrap(), doc);
    assert!(editing(&board));
}

#[test]
fn undo_while_editing_takes_back_the_typing() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    textedit::insert(&mut board.session, "x").unwrap();
    assert_eq!(sticky_text(&board, note), "x");
    assert_eq!(route_key(&mut board, "z", CMD), Ok(KeyOutcome::Handled));
    assert!(!editing(&board));
    // The edit was its own undo step: the sticky stays, as it read before.
    assert!(board.doc().unwrap().node(note).is_some());
    assert_eq!(sticky_text(&board, note), "hi");
    // ⇧⌘Z in a new edit ends that edit, then redoes the typing.
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    let shift_cmd = Mods { shift: true, ..CMD };
    assert_eq!(
        route_key(&mut board, "z", shift_cmd),
        Ok(KeyOutcome::Handled)
    );
    assert!(!editing(&board));
    assert_eq!(sticky_text(&board, note), "x");
}

/// Plan 6 final review C1, what the canvas does on a right-click on empty canvas while a
/// sticky is typed in: it ends the edit, then clears the selection (ruling P6-C10). No gesture
/// is left open, so a click on another sticky keeps what was typed, and later commands are
/// undo steps of their own: one undo takes back the last sticky only.
#[test]
fn ending_the_edit_before_select_none_keeps_what_was_typed() {
    let mut board = board();
    let note = sticky_at(&mut board, 0.0, "old");
    let other = sticky_at(&mut board, 600.0, "B");
    board.execute("select.none", &json!({})).unwrap();
    type_into(&mut board, note, "new");
    end_text_edit(&mut board).unwrap();
    assert!(!editing(&board));
    assert_eq!(gesture(&board), None);
    // The sticky's text gives the selection back to the sticky.
    assert_eq!(board.session.selection(), [note]);
    board.execute("select.none", &json!({})).unwrap();
    click(&mut board, 700.0, 100.0);
    assert_eq!(board.session.selection(), [other]);
    assert_eq!(sticky_text(&board, note), "new");
    // The two stickies, and the typing.
    assert_eq!(board.session.undo_depth(), (3, 0));
    let last = sticky_at(&mut board, 1200.0, "C");
    assert_eq!(board.session.undo_depth(), (4, 0));
    board.execute("edit.undo", &json!({})).unwrap();
    assert!(board.doc().unwrap().node(last).is_none());
    assert_eq!(layers(&board), [note, other]);
    assert_eq!(sticky_text(&board, note), "new");
}

/// Ruling R-TEXTEDIT: the Text menu's commands, and copy and paste, run inside the edit, which
/// goes on; any other command ends it first, as one undo step.
#[test]
fn only_text_commands_and_copy_and_paste_run_inside_the_edit() {
    let mut board = board();
    let note = sticky_at(&mut board, 0.0, "old");
    type_into(&mut board, note, "new");
    let steps = board.session.undo_depth();
    for id in ["text.bold", "text.alignCenter", "edit.copy", "edit.paste"] {
        end_text_edit_before(&mut board, id).unwrap();
        assert!(editing(&board), "{id}");
        assert_eq!(gesture(&board).as_deref(), Some("Edit text"), "{id}");
    }
    assert_eq!(board.session.undo_depth(), steps);
    end_text_edit_before(&mut board, "view.zoomFit").unwrap();
    assert!(!editing(&board));
    assert_eq!(gesture(&board), None);
    assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
    assert_eq!(sticky_text(&board, note), "new");
    // With nothing typed in, there is nothing to end.
    end_text_edit_before(&mut board, "edit.delete").unwrap();
    assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
    assert_eq!(board.session.selection(), [note]);
}

/// Plan 6 final review C1: Delete on the sticky being typed in, from its context menu or the
/// Edit menu. The edit ends first, so the whole sticky goes, as an undo step of its own, and
/// it stays gone after a click on another sticky; undone, it comes back as it was typed in.
#[test]
fn delete_after_the_edit_ends_keeps_the_sticky_typed_in_deleted() {
    let mut board = board();
    let note = sticky_at(&mut board, 0.0, "old");
    let other = sticky_at(&mut board, 600.0, "B");
    type_into(&mut board, note, "new");
    end_text_edit_before(&mut board, "edit.delete").unwrap();
    board.execute("edit.delete", &json!({})).unwrap();
    assert_eq!(layers(&board), [other]);
    click(&mut board, 700.0, 100.0);
    assert_eq!(layers(&board), [other]);
    assert_eq!(gesture(&board), None);
    assert_eq!(board.session.undo_depth(), (4, 0));
    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(layers(&board), [note, other]);
    assert_eq!(sticky_text(&board, note), "new");
}

/// Ruling R-TEXTEDIT for the keymap: a binding the user made can run any command while a
/// sticky is typed in. One FigCraft has live while editing runs inside the edit, which goes on;
/// copy acts on the text; any other ends the edit first, so a delete takes the whole sticky as
/// an undo step of its own, and it stays gone after a click on another sticky.
#[test]
fn a_bound_command_ends_the_text_edit_unless_it_acts_on_it() {
    let mut board = board();
    let note = sticky_at(&mut board, 0.0, "old");
    let other = sticky_at(&mut board, 600.0, "B");
    type_into(&mut board, note, "new");
    let steps = board.session.undo_depth();
    assert_eq!(
        route_command(&mut board, "text.alignCenter", None),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(
        route_command(&mut board, "edit.copy", None),
        Ok(KeyOutcome::Ui("edit.copy"))
    );
    assert!(editing(&board));
    assert_eq!(gesture(&board).as_deref(), Some("Edit text"));
    assert_eq!(board.session.undo_depth(), steps);
    assert_eq!(
        route_command(&mut board, "edit.delete", None),
        Ok(KeyOutcome::Handled)
    );
    assert!(!editing(&board));
    assert_eq!(layers(&board), [other]);
    click(&mut board, 700.0, 100.0);
    assert_eq!(layers(&board), [other]);
    assert_eq!(gesture(&board), None);
    assert_eq!(board.session.undo_depth(), (steps.0 + 2, 0));
    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(layers(&board), [note, other]);
    assert_eq!(sticky_text(&board, note), "new");
}
