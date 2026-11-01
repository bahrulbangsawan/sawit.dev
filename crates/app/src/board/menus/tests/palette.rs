//! The Actions palette: ⌘K and ⌘/ open it, what is typed is its query, and Enter runs its best
//! match.

use gpui_kit::{Modifiers, TestAppContext, point, px};

use super::super::palette::menu_part;
use super::{
    CLOSED, bounds_of, canvas_has_keyboard, command, open_menus, palette_highlight, palette_query,
    sticky_at, toast, tool,
};
use crate::board::tests::{Scratch, chrome_canvas, focus_canvas, layers, run_op};

#[gpui_kit::test]
fn cmd_k_opens_palette(cx: &mut TestAppContext) {
    let tmp = Scratch::new("cmd-k");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-k");
    assert_eq!(open_menus(&view, &cx), (true, false, None));
    // It shows, and its search field has the keyboard (ruling P6-C3).
    assert!(cx.debug_bounds("board-palette").is_some());
    assert!(!canvas_has_keyboard(&view, &mut cx));
    assert_eq!(palette_query(&view, &cx).as_deref(), Some(""));
    // Escape closes it, and the canvas has the keyboard back.
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(cx.debug_bounds("board-palette").is_none());
    assert!(canvas_has_keyboard(&view, &mut cx));
    // ⌘/ opens it too: 480 px wide, centred in the tab, under its top bar.
    cx.simulate_keystrokes("cmd-/");
    assert_eq!(open_menus(&view, &cx), (true, false, None));
    let palette = cx.debug_bounds("board-palette").expect("the palette");
    assert_eq!(palette.size.width, px(480.));
    assert_eq!(palette.center().x, px(400.));
    assert_eq!(palette.top(), px(64.));
    // A press outside closes it, and the canvas under it never gets the press.
    run_op(&view, command("tool.sticky"), &mut cx);
    cx.simulate_click(point(px(20.), px(560.)), Modifiers::default());
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(layers(&view, &cx).is_empty());
}

#[gpui_kit::test]
fn palette_enter_runs_first_result(cx: &mut TestAppContext) {
    let tmp = Scratch::new("palette-enter");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let sticky = sticky_at(&view, (-100.0, 0.0), &mut cx);
    let at = bounds_of(&view, sticky, &cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-k d u p");
    assert_eq!(palette_query(&view, &cx).as_deref(), Some("dup"));
    // Duplicate matches best: it is first, and highlighted.
    assert!(cx.debug_bounds("board-palette-edit.duplicate").is_some());
    assert_eq!(palette_highlight(&view, &cx), Some(0));
    // The arrows move the highlight, and stop at the first match; the sticky stays put.
    cx.simulate_keystrokes("down");
    assert_eq!(palette_highlight(&view, &cx), Some(1));
    cx.simulate_keystrokes("up up");
    assert_eq!(palette_highlight(&view, &cx), Some(0));
    assert_eq!(bounds_of(&view, sticky, &cx), at);
    // Enter runs it: the sticky is duplicated, and the palette closes.
    cx.simulate_keystrokes("enter");
    assert_eq!(layers(&view, &cx).len(), 2);
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(cx.debug_bounds("board-palette").is_none());
    assert!(canvas_has_keyboard(&view, &mut cx));
    assert_eq!(toast(&view, &cx), None);
}

/// Ruling P6-C3: what is typed in the palette is its query, not the canvas's keys.
#[gpui_kit::test]
fn typing_in_palette_keeps_tool(cx: &mut TestAppContext) {
    let tmp = Scratch::new("palette-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    let before = tool(&view, &cx);
    cx.simulate_keystrokes("cmd-k u p");
    assert_eq!(tool(&view, &cx), before);
    assert_eq!(palette_query(&view, &cx).as_deref(), Some("up"));
    // Closed, the palette gives the canvas the keyboard back, and its keys are its own again.
    cx.simulate_keystrokes("escape");
    assert!(canvas_has_keyboard(&view, &mut cx));
    cx.simulate_keystrokes("u");
    assert_eq!(tool(&view, &cx), "highlighter");
}

#[test]
fn a_palette_row_names_its_menu_beside_the_label() {
    assert_eq!(menu_part("Object › Group selection"), "Object");
    assert_eq!(menu_part("Edit › Copy as › Copy as CSS"), "Edit › Copy as");
    assert_eq!(menu_part("Keyboard shortcuts"), "");
}
