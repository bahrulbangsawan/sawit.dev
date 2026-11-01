//! The context menu: a right-click selects what it is on and opens there, Paste here pastes where
//! it opened, ⇧F10 opens it under the selection, and it shows while the UI is hidden.

use gpui_kit::{TestAppContext, VisualTestContext, point, px};
use sawit_board::menu::context_menu;
use serde_json::json;

use super::{
    CLOSED, bounds_of, canvas_has_keyboard, command, middle_of, on_screen, open_menus, right_click,
    run_menu, same_place, selection, sticky_at, toast,
};
use crate::board::tests::{
    Scratch, chrome_canvas, click, clipboard_turn, focus_canvas, layers, run_op,
};

#[gpui_kit::test]
fn right_click_selects_target(cx: &mut TestAppContext) {
    let tmp = Scratch::new("right-click");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
    run_op(&view, command("select.none"), &mut cx);
    assert!(selection(&view, &cx).is_empty());

    // On a sticky that isn't selected: it is, and the menu is the selection's.
    let on_sticky = middle_of(&view, sticky, &cx);
    right_click(&mut cx, on_sticky);
    assert_eq!(selection(&view, &cx), [sticky]);
    assert!(open_menus(&view, &cx).2.is_some());
    assert!(cx.debug_bounds("board-menu-edit.delete").is_some());

    // On a layer that is selected: the whole selection stays.
    let other = sticky_at(&view, (200.0, 0.0), &mut cx);
    let both =
        json!({"op": "command", "command": "select.set", "params": {"ids": [sticky, other]}});
    run_op(&view, both, &mut cx);
    right_click(&mut cx, on_sticky);
    assert_eq!(selection(&view, &cx), [sticky, other]);

    // On empty canvas, with stickies selected: nothing stays selected, and the menu is the
    // empty canvas's four items, where the pointer is (ruling P6-C10).
    let empty = on_screen(&view, (100.0, -150.0), &cx);
    right_click(&mut cx, empty);
    assert!(selection(&view, &cx).is_empty());
    let at = open_menus(&view, &cx).2.expect("a context menu");
    assert!(same_place(at.position, empty), "{at:?}");
    let items = view.read_with(&cx, |view, _| {
        context_menu(&view.board().unwrap().session).len()
    });
    assert_eq!(items, 4);
    for selector in [
        "board-menu-edit.paste",
        "board-menu-select.all",
        "board-menu-view.zoomFit",
        "board-menu-view.toggleUi",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "{selector} isn't drawn"
        );
    }
    assert!(cx.debug_bounds("board-menu-edit.delete").is_none());
}

/// Ruling P6-C4: the context menu's Paste here pastes the copy where the menu opened, through
/// the keyboard's copy and paste.
#[gpui_kit::test]
fn paste_here_pastes_where_the_menu_opened(cx: &mut TestAppContext) {
    let _turn = clipboard_turn();
    let tmp = Scratch::new("paste-here");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    sticky_at(&view, (-100.0, 0.0), &mut cx);
    run_menu(&view, "edit.copy", &mut cx);
    // Right of the sticky, on empty canvas.
    let here = on_screen(&view, (150.0, 80.0), &cx);
    right_click(&mut cx, here);
    click(&mut cx, "board-menu-edit.paste");
    let [_, pasted] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    let bounds = bounds_of(&view, pasted, &cx);
    assert!(
        (bounds.x0 - 150.0).abs() < 1e-3 && (bounds.y0 - 80.0).abs() < 1e-3,
        "{bounds:?}"
    );
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(canvas_has_keyboard(&view, &mut cx));
    assert_eq!(toast(&view, &cx), None);
}

/// Ruling P6-C6: while the UI is hidden, the context menu and the palette still show, and the
/// empty canvas's menu brings the UI back.
#[gpui_kit::test]
fn the_menus_show_while_the_ui_is_hidden(cx: &mut TestAppContext) {
    let tmp = Scratch::new("menus-ui-hidden");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let hidden = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.board().unwrap().session.ds().unwrap().view.ui_hidden
        })
    };
    view.update(&mut cx, |view, cx| {
        view.choose("view.toggleUi", json!({}), cx)
    });
    cx.run_until_parked();
    assert!(hidden(&cx));
    assert!(cx.debug_bounds("board-top-bar").is_none());

    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-k");
    assert!(cx.debug_bounds("board-palette").is_some());
    cx.simulate_keystrokes("escape");

    let empty = on_screen(&view, (100.0, -150.0), &cx);
    right_click(&mut cx, empty);
    click(&mut cx, "board-menu-view.toggleUi");
    cx.run_until_parked();
    assert!(!hidden(&cx));
    assert!(cx.debug_bounds("board-top-bar").is_some());
}

/// Ruling P6-C17: ⇧F10 opens the context menu under the selection, or in the middle of the
/// canvas with nothing selected.
#[gpui_kit::test]
fn the_context_menu_key_opens_it_under_the_selection(cx: &mut TestAppContext) {
    let tmp = Scratch::new("menu-key");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("shift-f10");
    let at = open_menus(&view, &cx).2.expect("a context menu");
    let bounds = bounds_of(&view, sticky, &cx);
    let corner = on_screen(&view, (bounds.x0, bounds.y1), &cx);
    assert!(
        same_place(at.position, corner + point(px(0.), px(8.))),
        "{at:?}"
    );
    assert!(cx.debug_bounds("board-menu-edit.delete").is_some());
    // The menu has the keyboard: Escape closes it and gives the keyboard back to the canvas.
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(canvas_has_keyboard(&view, &mut cx));

    run_op(&view, command("select.none"), &mut cx);
    cx.simulate_keystrokes("shift-f10");
    let at = open_menus(&view, &cx).2.expect("a context menu");
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    assert!(
        same_place(at.position, canvas.center()),
        "{at:?} in {canvas:?}"
    );
    assert!(cx.debug_bounds("board-menu-view.toggleUi").is_some());
}
