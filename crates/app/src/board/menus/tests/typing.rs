//! A menu command, a tool, a template or an agent's op while a sticky is typed in: the typing ends
//! first, as one undo step, unless the command acts on the text (ruling R-TEXTEDIT).

use figcraft_doc::NodeId;
use gpui_kit::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext,
};
use serde_json::json;

use super::{
    CLOSED, canvas_has_keyboard, command, middle_of, on_screen, open_menus, right_click, run_menu,
    selection, toast, tool,
};
use crate::board::BoardView;
use crate::board::tests::{
    Scratch, chrome_canvas, click, clipboard_turn, focus_canvas, layers, run_op,
};

/// Puts a sticky note reading `text` with its top left corner at page point `at`, selected,
/// as an agent's command does, and returns it.
fn sticky_reading(
    view: &Entity<BoardView>,
    at: (f64, f64),
    text: &str,
    cx: &mut TestAppContext,
) -> NodeId {
    let op = json!({
        "op": "command", "command": "board.sticky",
        "params": {"x": at.0, "y": at.1, "text": text},
    });
    let made = run_op(view, op, cx);
    figcraft_engine::params::id_of(&made["result"]["id"]).unwrap()
}

/// Two quick presses of the left button at `at` in the window: a double-click, which on a
/// sticky starts typing in it, all of its text selected.
fn double_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: at,
            button,
            modifiers,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            button,
            modifiers,
            click_count,
        });
    }
}

/// Whether a text is being typed in on the canvas.
fn typing(view: &Entity<BoardView>, cx: &TestAppContext) -> bool {
    view.read_with(cx, |view, _| view.editing_text())
}

/// What sticky `note` reads.
fn reads(view: &Entity<BoardView>, note: NodeId, cx: &TestAppContext) -> String {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, note).unwrap();
        let text = doc.node(text).and_then(|node| node.kind.text()).unwrap();
        text.characters.clone()
    })
}

/// How many steps the canvas's undo takes back.
fn undo_steps(view: &Entity<BoardView>, cx: &TestAppContext) -> usize {
    view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
}

/// The engine's gesture that is open, if one is: typing holds one, "Edit text", until the
/// typing ends.
fn gesture(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        ds.interaction.as_ref().map(|gesture| gesture.label.clone())
    })
}

/// Two stickies side by side with nothing selected, the first reading "old", then typing in
/// the first as a person does: a double-click on it, and "new" typed in place of its text.
/// The two stickies, and how many undo steps there were before the typing.
fn typing_in_the_first_of_two(
    view: &Entity<BoardView>,
    cx: &mut VisualTestContext,
) -> (NodeId, NodeId, usize) {
    let note = sticky_reading(view, (-300.0, -100.0), "old", cx);
    let other = sticky_reading(view, (100.0, -100.0), "B", cx);
    run_op(view, command("select.none"), cx);
    let steps = undo_steps(view, cx);
    let at = middle_of(view, note, cx);
    double_click(cx, at);
    assert!(typing(view, cx), "the double-click started no typing");
    cx.simulate_input("new");
    assert_eq!(reads(view, note, cx), "new");
    (note, other, steps)
}

/// Final review C1 (ruling R-TEXTEDIT): a right-click on empty canvas while a sticky is typed
/// in ends the typing first, as a press outside it does, and then clears the selection
/// (ruling P6-C10). What was typed is an undo step of its own, and it stays after the next
/// click. Clearing the selection alone left the engine's "Edit text" gesture open, and the
/// next click on a layer cancelled it, which took the typing back.
#[gpui_kit::test]
fn a_right_click_on_empty_canvas_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("right-click-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, other, steps) = typing_in_the_first_of_two(&view, &mut cx);

    // Empty canvas, above the gap between the stickies.
    let empty = on_screen(&view, (0.0, -150.0), &cx);
    right_click(&mut cx, empty);
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert!(selection(&view, &cx).is_empty());
    assert!(cx.debug_bounds("board-menu-view.zoomFit").is_some());
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menus(&view, &cx), CLOSED);
    assert!(canvas_has_keyboard(&view, &mut cx));

    // A click on the other sticky selects it, and what was typed stays.
    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(reads(&view, note, &cx), "new");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// Final review C1: Delete in the context menu of the sticky being typed in. The right-click
/// ends the typing as one undo step, so the menu is the sticky's: Delete takes the sticky off
/// the canvas as a step of its own, and it stays off after the next click. Inside the open
/// edit, Delete took the text being typed, and that click brought it back as it was.
#[gpui_kit::test]
fn delete_in_the_menu_of_the_sticky_typed_in_keeps_it_deleted(cx: &mut TestAppContext) {
    let tmp = Scratch::new("delete-typed-in");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, other, steps) = typing_in_the_first_of_two(&view, &mut cx);

    let on_note = middle_of(&view, note, &cx);
    right_click(&mut cx, on_note);
    assert!(!typing(&view, &cx));
    assert_eq!(selection(&view, &cx), [note]);
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    click(&mut cx, "board-menu-edit.delete");
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(undo_steps(&view, &cx), steps + 2);

    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(toast(&view, &cx), None);
    // Undone a step at a time: the sticky comes back as it was typed in, then as it read.
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(reads(&view, note, &cx), "new");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(reads(&view, note, &cx), "old");
}

/// Ruling R-TEXTEDIT from the ≡ menu and the palette ([`BoardView::run_menu`]): a Text menu
/// command, and copy and paste, act on the text being typed, which stays open; any other
/// command ends the typing first, as one undo step. Edit ▸ Delete then takes the sticky off
/// the canvas as a step of its own. Inside the open edit it took the text being typed, and
/// left the engine's "Edit text" gesture open.
#[gpui_kit::test]
fn menu_commands_end_the_typing_unless_they_act_on_it(cx: &mut TestAppContext) {
    let _turn = clipboard_turn();
    let tmp = Scratch::new("menu-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, other, steps) = typing_in_the_first_of_two(&view, &mut cx);

    run_menu(&view, "text.alignCenter", &mut cx);
    run_menu(&view, "edit.copy", &mut cx);
    assert!(typing(&view, &cx));
    assert_eq!(undo_steps(&view, &cx), steps);
    cx.simulate_input("er");
    assert_eq!(reads(&view, note, &cx), "newer");

    run_menu(&view, "edit.delete", &mut cx);
    assert!(!typing(&view, &cx));
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(toast(&view, &cx), None);
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(reads(&view, note, &cx), "newer");
}

/// Ruling R-TEXTEDIT from the chrome ([`BoardView::choose`]): a tool picked in the toolbar
/// while a sticky is typed in ends the typing first, as one undo step. Picking the Hand tool
/// drops the text edit without ending it, which left the engine's "Edit text" gesture open:
/// back on the Move tool, the next click on a layer cancelled it, and the typing was gone.
#[gpui_kit::test]
fn a_tool_picked_while_typing_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("tool-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, other, steps) = typing_in_the_first_of_two(&view, &mut cx);

    click(&mut cx, "tool.hand");
    assert_eq!(tool(&view, &cx), "hand");
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    click(&mut cx, "tool.move");
    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(reads(&view, note, &cx), "new");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// CANVAS-UI-5, ruling R-TEXTEDIT from the toolbar's Templates & table picker: a template or
/// a table placed while a sticky is typed in ends the typing first, as one undo step, and is
/// a step of its own. Placed inside the open edit, it made no undo step, the canvas went on
/// typing in the sticky while the template showed selected, and one ⌘Z took both back.
#[gpui_kit::test]
fn a_template_or_a_table_placed_while_typing_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("picker-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, _, steps) = typing_in_the_first_of_two(&view, &mut cx);
    let template = figcraft_engine::board_templates::TEMPLATES[0].id;
    click(&mut cx, "board-picker");
    cx.run_until_parked();
    view.update(&mut cx, |view, cx| view.place_template(template, cx));
    cx.run_until_parked();
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    assert_eq!(toast(&view, &cx), None);
    // One ⌘Z takes the template back, and what was typed stays.
    let placed = layers(&view, &cx).len();
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(layers(&view, &cx).len(), 2, "{placed} layers before");
    assert_eq!(reads(&view, note, &cx), "new");

    // A table, the same way.
    let steps = undo_steps(&view, &cx);
    view.update(&mut cx, |view, cx| {
        let session = &mut view.board_mut().unwrap().session;
        assert!(figcraft_engine::board::edit_text(session, note).unwrap());
        cx.notify();
    });
    cx.simulate_input("er");
    let typed = reads(&view, note, &cx);
    assert_ne!(typed, "new", "nothing was typed");
    click(&mut cx, "board-picker");
    cx.run_until_parked();
    view.update(&mut cx, |view, cx| view.place_table(2, 3, cx));
    cx.run_until_parked();
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(layers(&view, &cx).len(), 3);
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    assert_eq!(reads(&view, note, &cx), typed);
    assert_eq!(toast(&view, &cx), None);
}

/// The page on show.
fn page(view: &Entity<BoardView>, cx: &TestAppContext) -> NodeId {
    view.read_with(cx, |view, _| view.board().unwrap().session.page().unwrap())
}

/// Plan 7 final review I-2 (ruling R-TEXTEDIT, the command op): an agent's `page.select`
/// while a sticky is typed in ends the typing first, as one undo step. Alone, it dropped the
/// text edit and left the engine's "Edit text" gesture open: the user's next click on a layer
/// of the page it opened cancelled the gesture, and the typing was gone, with no undo step to
/// bring it back.
#[gpui_kit::test]
fn an_agents_page_switch_while_typing_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-page-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    // A sticky reading "old" on the first page, a second page with a sticky on it, then the
    // first page again (which fits its sticky in the view).
    let first = page(&view, &cx);
    let note = sticky_reading(&view, (-100.0, -100.0), "old", &mut cx);
    run_op(&view, command("page.add"), &mut cx);
    let second = page(&view, &cx);
    let there = sticky_reading(&view, (-100.0, -100.0), "there", &mut cx);
    let back = json!({"op": "command", "command": "page.select", "params": {"id": first}});
    run_op(&view, back, &mut cx);
    let steps = undo_steps(&view, &cx);
    // Typing in it, as a person does.
    let at = middle_of(&view, note, &cx);
    double_click(&mut cx, at);
    assert!(typing(&view, &cx), "the double-click started no typing");
    cx.simulate_input("new");
    assert_eq!(reads(&view, note, &cx), "new");

    let switch = json!({"op": "command", "command": "page.select", "params": {"id": second}});
    let reply = run_op(&view, switch, &mut cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(page(&view, &cx), second);
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 1);

    // The user's click on the sticky there, with the Move tool, selects it, and what was typed
    // stays.
    cx.simulate_click(middle_of(&view, there, &cx), Modifiers::default());
    assert_eq!(selection(&view, &cx), [there]);
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(reads(&view, note, &cx), "new");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// Plan 7 final review I-2: an agent's `delete` of the sticky being typed in ends the typing
/// first, as one undo step, so the delete is a step of its own and stays after the next click.
/// Inside the open edit the sticky went, but the engine's "Edit text" gesture stayed open, and
/// the user's next click on a layer cancelled it, which brought the sticky back as it read
/// before the typing.
#[gpui_kit::test]
fn an_agents_delete_of_the_sticky_typed_in_keeps_it_deleted(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-delete-typing");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (note, other, steps) = typing_in_the_first_of_two(&view, &mut cx);

    let reply = run_op(&view, json!({"op": "delete", "ids": [note]}), &mut cx);
    assert_eq!(reply["deleted"], json!(1), "{reply}");
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(undo_steps(&view, &cx), steps + 2);

    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(layers(&view, &cx), [other]);
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(toast(&view, &cx), None);
    // Undone a step at a time: the sticky comes back as it was typed in, then as it read.
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(reads(&view, note, &cx), "new");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(reads(&view, note, &cx), "old");
}
