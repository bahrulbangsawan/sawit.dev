//! The pointer: events as the tools take them, the requests they make of the view, triple clicks
//! and the arrows of resize handles.

use figcraft_doc::NodeKind;
use figcraft_engine::{ViewState, textedit};

use super::*;

/// The text layer of board object `id`.
fn text_of(board: &BoardDoc, id: NodeId) -> NodeId {
    figcraft_engine::board::text_of(&board.doc().unwrap(), id).unwrap()
}

/// The request a command makes to start typing in board object `id`.
fn edit_text_request(id: NodeId) -> UiRequest {
    UiRequest {
        kind: "board.editText".into(),
        data: json!({"id": id}),
    }
}

/// The page point in the middle of the line of character `index` of the text being edited,
/// where its caret stands.
fn on_character(board: &BoardDoc, index: usize) -> (f64, f64) {
    let ((x, top), (_, bottom)) = crate::overlay::caret_at(&board.session, None, index).unwrap();
    let view = &board.session.ds().unwrap().view;
    view.screen_to_page(x, (top + bottom) / 2.0)
}

/// The characters selected in the text being edited: from and to.
fn selected(board: &BoardDoc) -> Option<(usize, usize)> {
    textedit::selected_range(&board.session).map(|(_, from, to)| (from, to))
}

#[test]
fn pointer_event_uses_page_coordinates() {
    let view = ViewState {
        cx: 100.0,
        cy: 50.0,
        zoom: 2.0,
        vw: 800.0,
        vh: 600.0,
        ..ViewState::default()
    };
    let event = pointer_event(PointerKind::Down, (400.0, 300.0), &view, SHIFT);
    assert_eq!((event.x, event.y), (100.0, 50.0));
    assert_eq!((event.sx, event.sy), (400.0, 300.0));
    assert_eq!(event.zoom, 2.0);
    assert_eq!(event.kind, PointerKind::Down);
    assert!(event.shift && !event.alt && !event.cmd && !event.ctrl);
}

#[test]
fn rectangle_tool_draws_one_undo_step() {
    let mut board = design();
    board.execute("tool.rectangle", &json!({})).unwrap();
    // As FigCraft's own drags go (ruling P2-C4): the last move sizes the shape, and the
    // release commits it.
    send(
        &mut board,
        &[
            at(PointerKind::Down, 0.0, 0.0),
            at(PointerKind::Move, 50.0, 40.0),
            at(PointerKind::Move, 100.0, 80.0),
            at(PointerKind::Up, 100.0, 80.0),
        ],
    );
    let layers = layers(&board);
    assert_eq!(layers.len(), 1);
    let doc = board.doc().unwrap();
    let drawn = doc.node(layers[0]).unwrap();
    assert!(
        matches!(drawn.kind, NodeKind::Rectangle),
        "{:?}",
        drawn.kind
    );
    assert_eq!((drawn.width, drawn.height), (100.0, 80.0));
    assert_eq!(board.session.undo_depth(), (1, 0));
    board.execute("edit.undo", &json!({})).unwrap();
    assert!(page_is_empty(&board));
}

#[test]
fn drag_released_outside_ends_gesture() {
    let mut board = design();
    board.execute("tool.rectangle", &json!({})).unwrap();
    // The release comes far off the canvas, where the window's listener sees it.
    send(
        &mut board,
        &[
            at(PointerKind::Down, 0.0, 0.0),
            at(PointerKind::Move, 50.0, 40.0),
            at(PointerKind::Move, 100.0, 80.0),
            at(PointerKind::Up, 5000.0, 5000.0),
        ],
    );
    assert_eq!(layers(&board).len(), 1);
    assert!(!board.session.tool.busy());
    assert_eq!(board.session.undo_depth(), (1, 0));
}

#[test]
fn sticky_click_starts_text_editing() {
    let mut board = board();
    board.execute("tool.sticky", &json!({})).unwrap();
    let requests = send(
        &mut board,
        &[
            at(PointerKind::Down, 0.0, 0.0),
            at(PointerKind::Up, 0.0, 0.0),
        ],
    );
    handle_requests(&mut board, &requests);
    let [note] = layers(&board)[..] else {
        panic!("layers {:?}", layers(&board))
    };
    assert!(editing(&board));
    assert_eq!(text_edit(&board).id, text_of(&board, note));
}

#[test]
fn next_sticky_keeps_typing() {
    let mut board = board();
    let first = sticky(&mut board, "");
    assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
    let shift_cmd = Mods { shift: true, ..CMD };
    assert_eq!(
        route_key(&mut board, "Enter", shift_cmd),
        Ok(KeyOutcome::Handled)
    );
    let requests = std::mem::take(&mut board.session.ui_requests);
    let rest = handle_requests(&mut board, &requests);
    assert!(rest.is_empty(), "{rest:?}");
    let [one, next] = layers(&board)[..] else {
        panic!("layers {:?}", layers(&board))
    };
    assert_eq!(one, first);
    // Typing goes on in the new sticky.
    assert_eq!(text_edit(&board).id, text_of(&board, next));
    textedit::insert(&mut board.session, "more").unwrap();
    assert_eq!(sticky_text(&board, next), "more");
}

#[test]
fn requests_for_the_view_are_handed_back_in_order() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    let ask = |kind: &str| UiRequest {
        kind: kind.into(),
        data: json!({}),
    };
    let requests = [
        ask("panel.layers"),
        edit_text_request(note),
        ask("palette.actions"),
    ];
    let rest = handle_requests(&mut board, &requests);
    assert_eq!(rest, [ask("panel.layers"), ask("palette.actions")]);
    assert_eq!(text_edit(&board).id, text_of(&board, note));
    assert!(board.session.message.is_none());
}

#[test]
fn a_text_edit_that_cant_start_leaves_a_message_for_the_view() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    board
        .execute("file.saveVersion", &json!({"name": "First"}))
        .unwrap();
    // An earlier version on show can't be edited.
    board
        .execute("version.preview", &json!({"index": 0}))
        .unwrap();
    let rest = handle_requests(&mut board, &[edit_text_request(note)]);
    assert!(rest.is_empty(), "{rest:?}");
    assert!(!editing(&board));
    let message = board.session.message.clone().unwrap_or_default();
    assert!(message.contains("earlier version"), "{message:?}");
}

#[test]
fn leaving_the_canvas_clears_what_hovering_showed() {
    let mut board = design();
    // Alt over a layer measures to it, outlining the box measured to.
    board.session.tool.overlay.measure_box = Some([0.0, 0.0, 10.0, 10.0]);
    pointer_left(&mut board).unwrap();
    assert_eq!(board.session.tool.overlay.measure_box, None);
}

#[test]
fn resize_dir_buckets_angles() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    for (angle, dir) in [
        (0.0, ResizeDir::Horizontal),
        (FRAC_PI_2, ResizeDir::Vertical),
        (FRAC_PI_4, ResizeDir::DiagDown),
        (3.0 * FRAC_PI_4, ResizeDir::DiagUp),
        (PI, ResizeDir::Horizontal),
        // Either way along the line, and the nearest arrow.
        (-FRAC_PI_4, ResizeDir::DiagUp),
        (-FRAC_PI_2, ResizeDir::Vertical),
        (0.3, ResizeDir::Horizontal),
        (1.0, ResizeDir::DiagDown),
        (f64::NAN, ResizeDir::Horizontal),
    ] {
        assert_eq!(resize_dir(angle), dir, "{angle}");
    }
}

/// As FigCraft selects a line on a triple click in the text being edited (ui-egui
/// shortcuts.rs `triple_click_line`), where the third press alone would leave a caret.
#[test]
fn a_triple_click_in_the_text_being_edited_selects_its_line() {
    // Laid out in a system font, the two lines lie one under the other.
    figcraft_text::scan_system_fonts();
    let mut board = board();
    let note = sticky(&mut board, "one\ntwo");
    // As after a double-click: all of the text selected.
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    assert_eq!(selected(&board), Some((0, 7)));
    let (x, y) = on_character(&board, 5);
    let requests = triple_click(&mut board, &at(PointerKind::Down, x, y)).unwrap();
    assert!(requests.is_empty(), "{requests:?}");
    // The second line, "two".
    assert_eq!(selected(&board), Some((4, 7)));
    // The release ends the press, and the line stays selected.
    send(&mut board, &[at(PointerKind::Up, x, y)]);
    assert_eq!(selected(&board), Some((4, 7)));
    assert_eq!(text_edit(&board).id, text_of(&board, note));
}

#[test]
fn a_triple_click_where_nothing_is_edited_is_a_double_click() {
    let mut board = board();
    let note = sticky(&mut board, "hi");
    board.execute("select.none", &json!({})).unwrap();
    // The first click selects the sticky (its middle: it is 200 × 200 from (0, 0)).
    click(&mut board, 100.0, 100.0);
    assert!(!editing(&board));
    // A third quick press, here with no second before it, is taken as a double-click,
    // which starts typing in the sticky.
    let requests = triple_click(&mut board, &at(PointerKind::Down, 100.0, 100.0)).unwrap();
    handle_requests(&mut board, &requests);
    send(&mut board, &[at(PointerKind::Up, 100.0, 100.0)]);
    assert!(editing(&board));
    assert_eq!(text_edit(&board).id, text_of(&board, note));
}

#[test]
fn a_triple_click_in_vector_edit_mode_leaves_it_only_on_empty_canvas() {
    let mut board = design();
    rectangle(&mut board);
    assert_eq!(
        route_key(&mut board, "Enter", NONE),
        Ok(KeyOutcome::Handled)
    );
    let vector_editing = |board: &BoardDoc| board.session.ds().unwrap().vector_edit.is_some();
    assert!(vector_editing(&board));
    // On the shape (its top edge) it is a press: a triple has its own meaning there.
    triple_click(&mut board, &at(PointerKind::Down, 50.0, 0.0)).unwrap();
    send(&mut board, &[at(PointerKind::Up, 50.0, 0.0)]);
    assert!(vector_editing(&board));
    // On empty canvas it is a double-click, which leaves the mode: the double-click that
    // entered it counts on, so leaving would otherwise take a fourth click.
    triple_click(&mut board, &at(PointerKind::Down, 500.0, 500.0)).unwrap();
    send(&mut board, &[at(PointerKind::Up, 500.0, 500.0)]);
    assert!(!vector_editing(&board));
}
