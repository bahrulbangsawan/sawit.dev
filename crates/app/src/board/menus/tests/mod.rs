//! The menus' tests, by what they test, and the helpers they share.

mod context_menu;
mod main_menu;
mod palette;
mod typing;

use figcraft_doc::NodeId;
use figcraft_doc::geom::Rect;
use gpui_kit::{
    Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point, px,
};
use serde_json::{Value, json};

use super::super::BoardView;
use super::super::tests::{run_op, view_state};
use super::MenuAt;

/// Puts a sticky note with its top left corner at page point `at`, selected, as an agent's
/// command does, and returns it.
fn sticky_at(view: &Entity<BoardView>, at: (f64, f64), cx: &mut TestAppContext) -> NodeId {
    let op = json!({
        "op": "command", "command": "board.sticky", "params": {"x": at.0, "y": at.1},
    });
    let made = run_op(view, op, cx);
    figcraft_engine::params::id_of(&made["result"]["id"]).unwrap()
}

/// The op that runs engine command `command` with no params.
fn command(command: &str) -> Value {
    json!({"op": "command", "command": command})
}

fn selection(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<NodeId> {
    view.read_with(cx, |view, _| view.board().unwrap().session.selection())
}

/// The box of `layer` on the page.
fn bounds_of(view: &Entity<BoardView>, layer: NodeId, cx: &TestAppContext) -> Rect {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        doc.world_bounds(layer).unwrap()
    })
}

/// Where page point `page` is in the window.
fn on_screen(view: &Entity<BoardView>, page: (f64, f64), cx: &TestAppContext) -> Point<Pixels> {
    let canvas = view.read_with(cx, |view, _| view.bounds);
    let (x, y) = view_state(view, cx).page_to_screen(page.0, page.1);
    canvas.origin + point(px(x as f32), px(y as f32))
}

/// The middle of `layer` in the window.
fn middle_of(view: &Entity<BoardView>, layer: NodeId, cx: &TestAppContext) -> Point<Pixels> {
    let bounds = bounds_of(view, layer, cx);
    let middle = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
    on_screen(view, middle, cx)
}

/// Whether `a` and `b` are the same place in the window, but for rounding.
fn same_place(a: Point<Pixels>, b: Point<Pixels>) -> bool {
    (f32::from(a.x) - f32::from(b.x)).abs() < 0.01 && (f32::from(a.y) - f32::from(b.y)).abs() < 0.01
}

/// A press and release of the right button at `at` in the window.
fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
}

/// Runs menu item `id` as a click on it does.
fn run_menu(view: &Entity<BoardView>, id: &'static str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.run_menu(id, window, cx)));
}

/// Which of the palette, the sheet and the context menu are open.
fn open_menus(view: &Entity<BoardView>, cx: &TestAppContext) -> (bool, bool, Option<MenuAt>) {
    view.read_with(cx, |view, _| {
        let menus = &view.menus;
        (menus.palette_open, menus.sheet_open, menus.context_menu_at)
    })
}

/// Nothing open: the palette, the sheet and the context menu are closed.
const CLOSED: (bool, bool, Option<MenuAt>) = (false, false, None);

fn canvas_has_keyboard(view: &Entity<BoardView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.read(cx).focus.is_focused(window))
}

/// What is typed in the palette's search field, while it shows.
fn palette_query(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, cx| {
        let palette = view.menus.palette.as_ref()?;
        Some(palette.query.read(cx).value().to_string())
    })
}

/// Which of the palette's matches Enter runs.
fn palette_highlight(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<usize> {
    view.read_with(cx, |view, _| {
        view.menus.palette.as_ref().map(|palette| palette.highlight)
    })
}

/// The tool the canvas's pointer events go to.
fn tool(view: &Entity<BoardView>, cx: &TestAppContext) -> &'static str {
    view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
}

fn toast(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| view.toast.as_ref().map(ToString::to_string))
}
