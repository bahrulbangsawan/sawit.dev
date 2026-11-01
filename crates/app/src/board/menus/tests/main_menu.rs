//! The main menu and the shortcuts sheet it opens, and an agent's Escape, which closes whichever
//! menu is open.

use gpui_kit::{Modifiers, TestAppContext, point, px};
use serde_json::json;

use super::{
    CLOSED, canvas_has_keyboard, command, open_menus, run_menu, selection, sticky_at, toast,
};
use crate::board::tests::{
    Scratch, chrome_canvas, click, focus_canvas, layers, run_op, saved_layers,
};

#[gpui_kit::test]
fn menu_runs_engine_command(cx: &mut TestAppContext) {
    let tmp = Scratch::new("menu-runs");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
    assert_eq!(selection(&view, &cx), [sticky]);
    run_menu(&view, "edit.duplicate", &mut cx);
    assert_eq!(layers(&view, &cx).len(), 2);
    assert_eq!(toast(&view, &cx), None);
    // A command Sawit carries out itself goes to the canvas, never to the engine (ruling
    // P6-C1): Save writes the canvas's file now, and says so.
    run_menu(&view, "file.save", &mut cx);
    assert_eq!(toast(&view, &cx).as_deref(), Some("Saved"));
    assert_eq!(saved_layers(&tmp.0.join("canvas.sawit")), 2);
}

/// Ruling P6-C7: an agent's Escape closes what is open, as the key does in the window, and
/// the canvas never sees it.
#[gpui_kit::test]
fn a_key_ops_escape_closes_the_open_menu(cx: &mut TestAppContext) {
    let tmp = Scratch::new("escape-op");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
    let escape = json!({"op": "key", "key": "Escape"});
    for (open, shown) in [
        ("app.actions", "board-palette"),
        ("help.shortcuts", "board-sheet"),
        ("view.contextMenu", "board-menu-edit.delete"),
    ] {
        let reply = run_op(&view, command(open), &mut cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.run_until_parked();
        assert!(
            cx.debug_bounds(shown).is_some(),
            "{open}: {shown} isn't drawn"
        );
        let reply = run_op(&view, escape.clone(), &mut cx);
        assert_eq!(reply, json!({"ok": true, "handled": true}), "{open}");
        cx.run_until_parked();
        assert_eq!(open_menus(&view, &cx), CLOSED, "{open}");
        assert!(cx.debug_bounds(shown).is_none(), "{open}: {shown} is drawn");
        assert_eq!(selection(&view, &cx), [sticky], "{open}");
        assert!(canvas_has_keyboard(&view, &mut cx), "{open}");
    }
    // With nothing open, Escape is the canvas's: it deselects.
    let reply = run_op(&view, escape, &mut cx);
    assert_eq!(reply, json!({"ok": true, "handled": true}));
    assert!(selection(&view, &cx).is_empty());
}

#[gpui_kit::test]
fn the_main_menu_comes_first_and_runs_its_items(cx: &mut TestAppContext) {
    let tmp = Scratch::new("main-menu");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let bar = cx.debug_bounds("board-top-bar").expect("the top bar");
    let button = cx
        .debug_bounds("board-main-menu")
        .expect("the main menu's button");
    let zoom = cx.debug_bounds("board-zoom").expect("the zoom menu");
    assert!(
        button.left() - bar.left() < px(16.),
        "{button:?} in {bar:?}"
    );
    assert!(button.right() < zoom.left());
    // The keyboard walks it: Help is its last menu, and Keyboard shortcuts Help's one item.
    click(&mut cx, "board-main-menu");
    cx.run_until_parked();
    cx.simulate_keystrokes("up right enter");
    assert_eq!(open_menus(&view, &cx), (false, true, None));
    assert!(cx.debug_bounds("board-sheet").is_some());
}

#[gpui_kit::test]
fn the_shortcuts_sheet_lists_the_keys_and_closes(cx: &mut TestAppContext) {
    let tmp = Scratch::new("sheet");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    // ⌃⇧/, as the keyboard reports it: the shifted symbol, without Shift.
    cx.simulate_keystrokes("ctrl-?");
    assert_eq!(open_menus(&view, &cx), (false, true, None));
    // In the middle of the tab, 24 px from its edges at most: its rows scroll inside it.
    let sheet = cx.debug_bounds("board-sheet").expect("the sheet");
    assert_eq!(sheet.center(), point(px(400.), px(300.)));
    assert_eq!(sheet.size.width, px(800. - 48.));
    assert!(sheet.size.height <= px(600. - 48.), "{sheet:?}");
    assert!(cx.debug_bounds("board-sheet-Essential").is_some());
    assert!(cx.debug_bounds("board-sheet-app.actions").is_some());
    // Escape closes it, and gives the canvas the keyboard back.
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(cx.debug_bounds("board-sheet").is_none());
    assert!(canvas_has_keyboard(&view, &mut cx));
    // So does a press outside it, which the canvas under it never gets.
    let steps = view.read_with(&cx, |view, _| view.board().unwrap().session.undo_depth());
    run_op(&view, command("tool.sticky"), &mut cx);
    run_op(&view, command("help.shortcuts"), &mut cx);
    cx.run_until_parked();
    let sheet = cx.debug_bounds("board-sheet").expect("the sheet");
    let outside = point(sheet.left() - px(8.), sheet.top() - px(8.));
    cx.simulate_click(outside, Modifiers::default());
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(layers(&view, &cx).is_empty());
    assert_eq!(
        view.read_with(&cx, |view, _| view.board().unwrap().session.undo_depth()),
        steps
    );
}
