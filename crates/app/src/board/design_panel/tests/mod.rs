//! The Design panel's tests, by what they test, and the helpers they share with the tests of its
//! sections.

mod fields;
mod scrub;
mod sections;

use std::path::{Path, PathBuf};

use figcraft_doc::NodeId;
use gpui_kit::{
    AppContext as _, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px, size,
};
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::super::BoardView;

/// A folder of the test's own, removed once the test has ended, after its views have closed
/// ([`crate::testing::scratch_dir`]): a closing canvas saves into it.
pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        Self(crate::testing::scratch_dir(&format!("design-panel-{test}")))
    }
}

/// A design file being made, before it is written.
pub(super) struct Design(figcraft_engine::Session);

impl Design {
    pub(super) fn new() -> Self {
        Self(figcraft_engine::Session::new())
    }

    /// Makes a layer with `node.create`'s params `layer`, and returns its id.
    pub(super) fn make(&mut self, layer: Value) -> NodeId {
        let made = self.0.execute("node.create", &layer).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// Sets `props` on layers `ids`.
    pub(super) fn set(&mut self, ids: &[NodeId], props: Value) {
        let params = json!({"ids": ids, "props": props});
        self.0.execute("node.set", &params).unwrap();
    }

    /// Writes the file at `path`.
    pub(super) fn save(mut self, path: &Path) {
        let params = json!({"path": path.to_str().unwrap()});
        self.0.execute("file.saveAs", &params).unwrap();
    }
}

/// Writes a design file at `path` with a layer made from each of `layers` (`node.create`'s
/// params) on its page, and returns their ids.
pub(super) fn design_file(path: &Path, layers: &[Value]) -> Vec<NodeId> {
    let mut design = Design::new();
    let ids = layers.iter().map(|layer| design.make(layer.clone()));
    let ids = ids.collect();
    design.save(path);
    ids
}

/// A 100 × 100 rectangle at (`x`, 0).
pub(super) fn rectangle(x: f64) -> Value {
    json!({"type": "rectangle", "x": x, "y": 0, "width": 100, "height": 100})
}

/// A 200 × 100 frame at (`x`, 0) holding a 20 × 20 rectangle, and that rectangle.
fn frame_with_a_child(design: &mut Design, x: f64) -> (NodeId, NodeId) {
    let frame = json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100});
    let frame = design.make(frame);
    let child = json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20});
    (frame, design.make(child))
}

/// The canvas of the file at `path` in a window of its own, as the workspace shows it, with
/// `ids` selected and drawn. The window is tall enough for every section to show at once, and
/// has the keyboard, as the one the user types in does: its fields hear when they take the
/// keyboard and lose it.
pub(super) fn open_in_window(
    cx: &mut TestAppContext,
    path: &Path,
    ids: &[NodeId],
) -> (Entity<BoardView>, VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(crate::board::init_keys);
    let path = path.to_path_buf();
    let window = cx.open_window(size(px(900.), px(1000.)), |_, cx| BoardView::open(path, cx));
    let view = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, _| window.activate_window());
    select(&view, ids, &mut cx);
    (view, cx)
}

/// Selects `ids`, as a click or an agent does, and draws the window again.
pub(super) fn select(view: &Entity<BoardView>, ids: &[NodeId], cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let board = view.board_mut().unwrap();
        board.session.set_selection(ids.to_vec());
        cx.notify();
    });
    cx.run_until_parked();
}

/// Clicks the middle of what the window drew with debug selector `selector`.
pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let drawn = cx.debug_bounds(selector);
    let bounds = drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

/// Whether the window drew something with debug selector `selector`.
pub(super) fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

/// What field `key` reads.
pub(super) fn field_text(view: &Entity<BoardView>, key: &str, cx: &TestAppContext) -> String {
    view.read_with(cx, |view, cx| {
        view.design.fields[key].input.read(cx).value().to_string()
    })
}

/// Gives the canvas the keyboard, as a press on it does.
pub(super) fn focus_canvas(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
}

/// The selected layers' `key`, as the Design panel reads it.
pub(super) fn panel_value(
    view: &Entity<BoardView>,
    ids: &[NodeId],
    key: &str,
    cx: &TestAppContext,
) -> Mixed {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        DesignSelection::read(&doc, ids).get(key)
    })
}

/// The canvas of the design file at `path`, opened as the workspace opens it, with `ids`
/// selected.
pub(super) fn open(cx: &mut TestAppContext, path: &Path, ids: &[NodeId]) -> Entity<BoardView> {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(crate::board::init_keys);
    let view = cx.new(|cx| BoardView::open(path.to_path_buf(), cx));
    view.update(cx, |view, _| {
        let board = view.board_mut().unwrap();
        board.session.set_selection(ids.to_vec());
    });
    view
}

/// Layer `id`'s `key`, as the engine's inspector gives it.
pub(super) fn prop(view: &Entity<BoardView>, id: NodeId, key: &str, cx: &TestAppContext) -> Value {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        figcraft_engine::inspect::node_json(&doc, id, 0)[key].clone()
    })
}

/// How many undo steps the canvas's file has.
pub(super) fn undo_steps(view: &Entity<BoardView>, cx: &TestAppContext) -> usize {
    view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
}

/// A text reading `characters` at (0, 0), with the engine's default fill.
pub(super) fn text_layer(characters: &str) -> Value {
    json!({"type": "text", "text": characters, "x": 0, "y": 0})
}

/// Starts editing text `id` on the canvas, the caret after its last character, as a
/// double-click on it does, and gives the canvas the keyboard: what is typed then goes into
/// the text.
pub(super) fn edit_text(view: &Entity<BoardView>, id: NodeId, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let session = &mut view.board_mut().unwrap().session;
        figcraft_engine::textedit::begin(session, id, Some(usize::MAX), false).unwrap();
        cx.notify();
    });
    focus_canvas(view, cx);
}

/// Whether a text is being edited on the canvas.
pub(super) fn editing(view: &Entity<BoardView>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.editing_text())
}

/// The label of the engine's gesture that is open, if one is.
pub(super) fn gesture(view: &Entity<BoardView>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.open_gesture())
}

/// Where page point `page` is in the window, as the canvas shows it now.
pub(super) fn on_canvas(
    view: &Entity<BoardView>,
    page: (f64, f64),
    cx: &VisualTestContext,
) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let (x, y) = ds.view.page_to_screen(page.0, page.1);
        view.bounds.origin + point(px(x as f32), px(y as f32))
    })
}

/// The canvas's zoom.
pub(super) fn zoom(view: &Entity<BoardView>, cx: &VisualTestContext) -> f64 {
    view.read_with(cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().view.zoom
    })
}

/// Runs the engine's undo, as ⌘Z on the canvas does.
pub(super) fn undo(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    cx.run_until_parked();
}

/// Draws two points of a path with the pen, which leaves the path open, and returns it: the
/// engine holds the pen's gesture open until the path ends.
pub(super) fn draw_pen_path(view: &Entity<BoardView>, cx: &mut VisualTestContext) -> NodeId {
    view.update(cx, |view, cx| view.choose("tool.pen", json!({}), cx));
    cx.run_until_parked();
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let origin = view.read_with(cx, |view, _| view.bounds.origin);
    for at in [point(px(40.), px(300.)), point(px(160.), px(340.))] {
        let at = origin + at;
        cx.simulate_mouse_down(at, left, none);
        cx.simulate_mouse_up(at, left, none);
    }
    assert_eq!(gesture(view, cx).as_deref(), Some("Pen"));
    let selected = view.read_with(cx, |view, _| view.board().unwrap().session.selection());
    let [path] = selected[..] else {
        panic!("the pen selects its path: {selected:?}")
    };
    path
}
