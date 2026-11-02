//! The Fill and Stroke sections' tests, by what they test, and the helpers they share.

mod picker;
mod rows;

use figcraft_doc::NodeId;
use gpui_kit::component::color_picker::ColorPickerState;
use gpui_kit::{Entity, VisualTestContext};
use serde_json::{Value, json};

// Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
use super::super::tests::{Design, click, rectangle};
use super::picker::colour_of;
use crate::board::BoardView;

/// Writes a design file at `path` with a rectangle at each of `xs` whose fills are `fills`
/// (as `node.set` takes them), and returns their ids.
fn with_fills(path: &std::path::Path, xs: &[f64], fills: &[Value]) -> Vec<NodeId> {
    let mut design = Design::new();
    let ids: Vec<NodeId> = xs.iter().map(|x| design.make(rectangle(*x))).collect();
    for (id, fills) in ids.iter().zip(fills) {
        design.set(&[*id], json!({ "fills": fills }));
    }
    design.save(path);
    ids
}

/// Colour picker `id`'s state.
fn picker(view: &Entity<BoardView>, id: &str, cx: &VisualTestContext) -> Entity<ColorPickerState> {
    view.read_with(cx, |view, _| view.design.pickers[id].state.clone())
}

/// Has picker `picker` pick `hex`, as a drag of its sliders does: it stays open.
fn pick(picker: &Entity<ColorPickerState>, hex: &str, cx: &mut VisualTestContext) {
    let colour = colour_of(hex, 1.0).unwrap();
    cx.update(|window, cx| {
        picker.update(cx, |picker, cx| picker.update_color(colour, window, cx));
    });
    cx.run_until_parked();
}

/// Types `text` over what field `selector` shows, and presses Enter.
fn type_in(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
    click(cx, selector);
    // A field that has the keyboard already keeps its caret where the click put it.
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
}

/// Whether picker `picker` is open.
fn is_open(picker: &Entity<ColorPickerState>, cx: &VisualTestContext) -> bool {
    picker.read_with(cx, |picker, _| picker.is_open())
}

/// The tool the canvas has on.
fn tool(view: &Entity<BoardView>, cx: &VisualTestContext) -> &'static str {
    view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
}
