//! Key names, and what a key runs on the canvas: shortcuts, Sawit's keys, the arrows and the
//! number keys.

use figcraft_doc::ListStyle;
use figcraft_engine::keymap::{self, KEYS, Platform};

use super::*;

/// The list style of sticky `id`'s first paragraph.
fn list_style(board: &BoardDoc, id: NodeId) -> ListStyle {
    let doc = board.doc().unwrap();
    let text = figcraft_engine::board::text_of(&doc, id).unwrap();
    let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
    text.para_list(0).list
}

/// A keystroke as `gpui_keystroke` writes it, split into its modifiers and its key. The
/// modifiers must come in the order ctrl, alt, shift, cmd, or they end up in the key.
fn split_keystroke(keystroke: &str) -> (Mods, &str) {
    let mut rest = keystroke;
    let mut take = |prefix: &str| match rest.strip_prefix(prefix) {
        Some(after) => {
            rest = after;
            true
        }
        None => false,
    };
    let (ctrl, alt, shift, cmd) = (take("ctrl-"), take("alt-"), take("shift-"), take("cmd-"));
    (
        Mods {
            shift,
            alt,
            cmd,
            ctrl,
        },
        rest,
    )
}

#[test]
fn key_names_map_gpui_to_figcraft() {
    assert_eq!(key_name("backspace").as_deref(), Some("Backspace"));
    assert_eq!(key_name("left").as_deref(), Some("ArrowLeft"));
    assert_eq!(key_name("k").as_deref(), Some("k"));
    assert_eq!(key_name("f5").as_deref(), Some("F5"));
    assert_eq!(key_name("capslock"), None);
}

#[test]
fn gpui_keystrokes_from_specs() {
    assert_eq!(
        gpui_keystroke("Shift+Cmd+K").as_deref(),
        Some("shift-cmd-k")
    );
    assert_eq!(gpui_keystroke("Cmd+\\").as_deref(), Some("cmd-\\"));
    assert_eq!(gpui_keystroke("Delete").as_deref(), Some("backspace"));
    assert_eq!(gpui_keystroke("Plus").as_deref(), Some("shift-="));
}

#[test]
fn every_mac_shortcut_round_trips_through_gpui() {
    // A binding's GPUI keystroke, read back through `key_name`, is that binding's key again.
    for spec in KEYS.iter().flat_map(|b| b.mac) {
        let keystroke = gpui_keystroke(spec).unwrap_or_else(|| panic!("{spec}: no keystroke"));
        let (mods, key) = split_keystroke(&keystroke);
        let name = key_name(key).unwrap_or_else(|| panic!("{spec} -> {keystroke}: no name"));
        assert!(
            keymap::matches(spec, Platform::Mac, &name, mods),
            "{spec} -> {keystroke} -> {name} {mods:?}"
        );
    }
}

#[test]
fn v_selects_move_tool() {
    let mut board = board();
    board.execute("tool.sticky", &json!({})).unwrap();
    assert_eq!(tool(&board), "sticky");
    assert_eq!(route_key(&mut board, "v", NONE), Ok(KeyOutcome::Handled));
    assert_eq!(tool(&board), "move");
}

#[test]
fn undo_runs_through_the_keymap() {
    let mut board = board();
    sticky(&mut board, "");
    assert!(!page_is_empty(&board));
    assert_eq!(route_key(&mut board, "z", CMD), Ok(KeyOutcome::Handled));
    assert!(page_is_empty(&board));
}

#[test]
fn hidden_commands_are_blocked() {
    let mut board = board();
    let before = tool(&board);
    // ⌘N is file.new, I is edit.pickColor (the eyedropper).
    assert_eq!(route_key(&mut board, "n", CMD), Ok(KeyOutcome::Unhandled));
    assert_eq!(route_key(&mut board, "i", NONE), Ok(KeyOutcome::Unhandled));
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(tool(&board), before);
}

#[test]
fn ui_commands_are_returned() {
    let mut board = board();
    assert_eq!(
        route_key(&mut board, "s", CMD),
        Ok(KeyOutcome::Ui("file.save"))
    );
}

#[test]
fn clipboard_keys_are_left_to_the_view() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    select(&mut board, note);
    let (revision, doc) = (board.revision(), board.doc().unwrap());
    let shift_cmd = Mods { shift: true, ..CMD };
    for (key, mods, id) in [
        ("c", CMD, "edit.copy"),
        ("x", CMD, "edit.cut"),
        ("v", CMD, "edit.paste"),
        ("v", shift_cmd, "edit.pasteOver"),
        ("r", shift_cmd, "edit.pasteReplace"),
    ] {
        assert_eq!(
            route_key(&mut board, key, mods),
            Ok(KeyOutcome::Ui(id)),
            "{id}"
        );
    }
    // Nothing ran: the view copies and pastes, with the system clipboard and the other tabs.
    assert!(board.session.clipboard.nodes.is_empty());
    assert_eq!(board.revision(), revision);
    assert_eq!(board.doc().unwrap(), doc);
}

#[test]
fn ui_requests_are_left_for_the_view() {
    let mut board = board();
    assert_eq!(route_key(&mut board, "k", CMD), Ok(KeyOutcome::Handled));
    let kinds: Vec<&str> = board
        .session
        .ui_requests
        .iter()
        .map(|r| r.kind.as_str())
        .collect();
    assert_eq!(kinds, ["palette.actions"]);
    assert_eq!(route_key(&mut board, "\\", CMD), Ok(KeyOutcome::Handled));
    assert!(board.session.ds().unwrap().view.ui_hidden);
}

#[test]
fn keys_without_a_shortcut_are_unhandled() {
    let mut board = board();
    assert_eq!(route_key(&mut board, "g", NONE), Ok(KeyOutcome::Unhandled));
    assert_eq!(
        route_key(&mut board, "NoSuchKey", NONE),
        Ok(KeyOutcome::Unhandled)
    );
}

#[test]
fn number_keys_need_a_selection() {
    let mut board = board();
    let (revision, doc) = (board.revision(), board.doc().unwrap());
    // With nothing selected, object.opacityKeys would refuse.
    assert_eq!(route_key(&mut board, "5", NONE), Ok(KeyOutcome::Unhandled));
    assert_eq!(board.revision(), revision);
    assert_eq!(board.doc().unwrap(), doc);
    let note = sticky(&mut board, "");
    select(&mut board, note);
    assert_eq!(route_key(&mut board, "5", NONE), Ok(KeyOutcome::Handled));
    assert_eq!(board.doc().unwrap().node(note).unwrap().opacity, 0.5);
}

#[test]
fn shifted_digits_from_a_us_keyboard() {
    let mut board = board();
    // GPUI sends ⇧0 as `)` with Shift off: view.zoom100.
    board.execute("view.set", &json!({"zoom": 2})).unwrap();
    assert_eq!(route_key(&mut board, ")", NONE), Ok(KeyOutcome::Handled));
    assert_eq!(board.session.ds().unwrap().view.zoom, 1.0);
    // ⇧2 zooms to the selection.
    let note = sticky(&mut board, "hi");
    select(&mut board, note);
    let view = board.session.ds().unwrap().view.clone();
    assert_eq!(route_key(&mut board, "@", NONE), Ok(KeyOutcome::Handled));
    assert_ne!(board.session.ds().unwrap().view, view);
    // While editing, ⇧⌘8 and ⇧⌘7 make lists, and ⇧0 is left to the input method to type.
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    assert_eq!(route_key(&mut board, "*", CMD), Ok(KeyOutcome::Handled));
    assert_eq!(list_style(&board, note), ListStyle::Bulleted);
    assert_eq!(route_key(&mut board, "&", CMD), Ok(KeyOutcome::Handled));
    assert_eq!(list_style(&board, note), ListStyle::Numbered);
    assert_eq!(route_key(&mut board, ")", NONE), Ok(KeyOutcome::Unhandled));
    assert_eq!(sticky_text(&board, note), "hi");
}

/// Sawit's keys ([`crate::shortcuts::SAWIT_KEYS`]) run their commands as FigCraft's run theirs:
/// ⇧6 and ⇧8 (which GPUI reports as `^` and `*`) choose the polygon and the star, and ⌃J
/// joins. With nothing to join, ⌃J is refused, and its toast says nothing (the key is quiet).
#[test]
fn sawits_keys_run_their_commands() {
    let mut design = design();
    let outcome = route_default_key(&mut design, "^", NONE).unwrap();
    assert_eq!(outcome, KeyOutcome::Handled);
    assert_eq!(design.session.tool.tool, figcraft_engine::ToolId::Polygon);
    route_default_key(&mut design, "8", SHIFT).unwrap();
    assert_eq!(design.session.tool.tool, figcraft_engine::ToolId::Star);
    let ctrl = Mods { ctrl: true, ..NONE };
    let refused = route_default_key(&mut design, "j", ctrl).unwrap_err();
    assert!(refused.is_unavailable(), "{refused:?}");
    assert_eq!(refused.for_key(), None);
    // ⌘J is Sawit's (the terminal), so it is no key of the canvas's.
    assert_eq!(
        route_default_key(&mut design, "j", CMD).unwrap(),
        KeyOutcome::Unhandled
    );
}

/// SMOKE-3: a board tool's key on a design file (Q for a sticky, B for a shape...) says in its
/// toast why it does nothing, in Sawit's words: FigCraft's send the user to File ▸ New board,
/// which the canvas hides.
#[test]
fn a_board_tools_key_on_a_design_file_says_what_the_canvas_is() {
    for (id, key) in [
        ("tool.sticky", "q"),
        ("tool.shape", "b"),
        ("tool.marker", "m"),
    ] {
        let mut design = design();
        let error = route_command(&mut design, id, Some((key, Mods::default())));
        assert_eq!(
            error.map_err(|e| e.to_string()),
            Err(format!(
                "command `{id}` is not available right now: this is for boards, and this \
                     canvas is a design file"
            )),
        );
    }
}

/// As FigCraft's own keyboard finishes the pen's path on Delete (ui-egui shortcuts.rs
/// `run_binding`), where the edit.delete the key is bound to would take the path being drawn,
/// which is selected, off the canvas mid-way.
#[test]
fn delete_while_the_pen_draws_finishes_the_path() {
    for key in ["Backspace", "Delete"] {
        let mut board = design();
        board.execute("tool.pen", &json!({})).unwrap();
        click(&mut board, 0.0, 0.0);
        click(&mut board, 100.0, 0.0);
        // The path being drawn is a layer already, and selected.
        let [path] = layers(&board)[..] else {
            panic!("layers {:?}", layers(&board))
        };
        assert_eq!(board.session.selection(), [path], "{key}");
        assert!(board.session.tool.pen.is_some());
        assert_eq!(route_key(&mut board, key, NONE), Ok(KeyOutcome::Handled));
        // Finished, not deleted: the path stays with its 2 points, the Pen stays chosen,
        // and its next click starts another path.
        assert_eq!(layers(&board), [path], "{key}");
        assert_eq!(network(&board, path).vertices.len(), 2, "{key}");
        assert!(board.session.tool.pen.is_none(), "{key}");
        assert_eq!(tool(&board), "pen");
        // The path is one undo step.
        assert_eq!(board.session.undo_depth(), (1, 0), "{key}");
    }
}

#[test]
fn delete_after_the_pens_first_point_deletes_nothing() {
    let mut board = design();
    let rect = rectangle(&mut board);
    board.execute("tool.pen", &json!({})).unwrap();
    // One point: nothing drawn yet, and the pen is under way.
    click(&mut board, 300.0, 300.0);
    assert!(board.session.tool.pen.is_some());
    assert_eq!(
        route_key(&mut board, "Backspace", NONE),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(layers(&board), [rect]);
    assert!(board.session.tool.pen.is_none());
    assert_eq!(tool(&board), "pen");
}

/// As FigCraft's own keyboard resizes with ⌘ and an arrow (ui-egui shortcuts.rs `key_event`;
/// keymap.rs GESTURES "⌘ arrows"): right and down grow, left and up shrink, by the nudge, its
/// top-left kept.
#[test]
fn cmd_and_an_arrow_resize_the_selection() {
    let mut board = design();
    let rect = rectangle(&mut board);
    let (small, big) = (
        board.session.prefs.nudge_small,
        board.session.prefs.nudge_big,
    );
    let size = |board: &BoardDoc| {
        let doc = board.doc().unwrap();
        let n = doc.node(rect).unwrap();
        (n.width, n.height)
    };
    let top_left = |board: &BoardDoc| {
        let bounds = board.doc().unwrap().world_bounds(rect).unwrap();
        (bounds.x0, bounds.y0)
    };
    assert_eq!(
        route_key(&mut board, "ArrowRight", CMD),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(size(&board), (100.0 + small, 80.0));
    assert_eq!(
        route_key(&mut board, "ArrowUp", CMD),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(size(&board), (100.0 + small, 80.0 - small));
    // With Shift, by the big nudge.
    let shift_cmd = Mods { shift: true, ..CMD };
    assert_eq!(
        route_key(&mut board, "ArrowDown", shift_cmd),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(size(&board), (100.0 + small, 80.0 - small + big));
    assert_eq!(top_left(&board), (0.0, 0.0));
    // Without ⌘ an arrow moves it, as before.
    assert_eq!(
        route_key(&mut board, "ArrowRight", NONE),
        Ok(KeyOutcome::Handled)
    );
    assert_eq!(top_left(&board), (small, 0.0));
    assert_eq!(size(&board), (100.0 + small, 80.0 - small + big));
    // With nothing selected, ⌘ and an arrow pan the view, as an arrow does.
    board.execute("select.none", &json!({})).unwrap();
    let (view, doc) = (
        board.session.ds().unwrap().view.clone(),
        board.doc().unwrap(),
    );
    assert_eq!(
        route_key(&mut board, "ArrowLeft", CMD),
        Ok(KeyOutcome::Handled)
    );
    assert_ne!(board.session.ds().unwrap().view, view);
    assert_eq!(board.doc().unwrap(), doc);
}
