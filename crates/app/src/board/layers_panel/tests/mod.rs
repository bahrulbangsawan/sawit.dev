//! The layers panel's tests, by what they test, and the design file and helpers they share.

mod actions;
mod panel;
mod rename;
mod typing;

use std::path::Path;

use figcraft_doc::{Document, NodeId, NodeKind};
use gpui_kit::{
    AppContext as _, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point, px, size,
};
use sawit_board::BoardDoc;
use serde_json::{Value, json};

use super::super::BoardView;

/// Plan 4's design file: frame F holds the group G, which holds C, then A and B on top of it.
/// F's layers run G (the bottom), A, B (the top), so the panel lists F, then B, A and G under
/// it, and C under G.
struct Tree {
    f: NodeId,
    g: NodeId,
    c: NodeId,
    a: NodeId,
    b: NodeId,
}

/// A 50 × 50 layer of `kind` called `name`, on top of `parent`'s layers.
fn add(doc: &mut Document, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
    doc.create(parent, kind, name, 0.0, 0.0, 50.0, 50.0)
        .unwrap()
}

/// Writes `doc` at `path` as a canvas file.
fn write(path: &Path, doc: Document) {
    let board = BoardDoc {
        session: figcraft_engine::Session::with_document(doc),
    };
    std::fs::write(path, board.to_bytes().unwrap()).unwrap();
}

/// Writes Plan 4's design file at `path`, and returns its layers.
fn design_file(path: &Path) -> Tree {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let f = add(&mut doc, page, NodeKind::Frame, "F");
    let g = add(&mut doc, f, NodeKind::Group, "G");
    let c = add(&mut doc, g, NodeKind::Rectangle, "C");
    let a = add(&mut doc, f, NodeKind::Rectangle, "A");
    let b = add(&mut doc, f, NodeKind::Rectangle, "B");
    write(path, doc);
    Tree { f, g, c, a, b }
}

/// The canvas of the file at `path`, opened as the workspace opens it, with no window.
fn open(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(super::super::init_keys);
    cx.new(|cx| BoardView::open(path.to_path_buf(), cx))
}

/// The canvas of the file at `path` in a 1000 × 700 window of its own, which has the keyboard,
/// as the one the user works in does, drawn: wide enough for a design file's two panels and a
/// canvas between them.
fn open_in_window(cx: &mut TestAppContext, path: &Path) -> (Entity<BoardView>, VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(super::super::init_keys);
    let path = path.to_path_buf();
    let window = cx.open_window(size(px(1000.), px(700.)), |_, cx| BoardView::open(path, cx));
    let view = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

/// Changes `view` with `f`, and draws the window again.
fn change(
    view: &Entity<BoardView>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut BoardView, &mut gpui_kit::Context<BoardView>),
) {
    view.update(cx, |view, cx| {
        f(view, cx);
        cx.notify();
    });
    cx.run_until_parked();
}

/// A debug selector made at run time: tests only, so leaking it is fine.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The debug selector of the row of layer `id`.
fn row(id: NodeId) -> &'static str {
    selector(format!("layers-row-{}", id.0))
}

/// Whether the window drew something with debug selector `selector`.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

/// The middle of what the window drew with debug selector `selector`.
fn middle(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
    let drawn = cx.debug_bounds(selector);
    drawn
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center()
}

/// Presses and releases the left button at `at` twice: a double-click.
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
    cx.run_until_parked();
}

fn selection(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<NodeId> {
    view.read_with(cx, |view, _| view.board().unwrap().session.selection())
}

/// How many undo steps the canvas's file has.
fn undo_steps(view: &Entity<BoardView>, cx: &TestAppContext) -> usize {
    view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
}

fn name_of(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> String {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        doc.node(id).unwrap().name.clone()
    })
}

/// Layer `id`'s eye and padlock: whether it is shown, and whether it is locked.
fn shown_and_locked(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> (bool, bool) {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        let node = doc.node(id).unwrap();
        (node.visible, node.locked)
    })
}

/// The layers of `id`, bottom first.
fn children(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> Vec<NodeId> {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        doc.node(id).unwrap().children.clone()
    })
}

/// The file's pages, and the one open.
fn pages(view: &Entity<BoardView>, cx: &TestAppContext) -> (Vec<NodeId>, NodeId) {
    view.read_with(cx, |view, _| {
        let session = &view.board().unwrap().session;
        (
            session.doc().unwrap().pages.clone(),
            session.page().unwrap(),
        )
    })
}

fn toast(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        view.toast.as_ref().map(|toast| toast.to_string())
    })
}

/// What `ds.hover` holds: the layer the canvas outlines as hovered.
fn hovered(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<NodeId> {
    view.read_with(cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().hover
    })
}

fn tool(view: &Entity<BoardView>, cx: &TestAppContext) -> &'static str {
    view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
}

fn command(command: &str) -> Value {
    json!({"op": "command", "command": command})
}

/// The middle of layer `id` in the window, on the page on show.
fn middle_of(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let board = view.board().unwrap();
        let bounds = board.doc().unwrap().world_bounds(id).unwrap();
        let middle = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
        let (x, y) = (board.session.ds().unwrap().view).page_to_screen(middle.0, middle.1);
        view.bounds.origin + point(px(x as f32), px(y as f32))
    })
}
