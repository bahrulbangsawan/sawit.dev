//! The `mermaid` op: a Mermaid diagram drawn as board shapes, and made ready off the board's
//! thread.

use sawit_core::mermaid::{Palette, render};
use serde_json::{Value, json};

use super::elements::needs_a_board;
use super::gesture::before_op;
use super::{failure, run_guarded};
use crate::doc::{BoardDoc, BoardError};
use crate::skeleton::{self, Skeleton};

/// How far right of what the page holds the `mermaid` op puts a diagram that names no place.
const MERMAID_GAP: f64 = 100.0;

/// What `mermaid` answers when the diagram's shapes or connectors can't be drawn because the canvas
/// is a design file, and what to do instead (SMOKE-3). A chart, which comes in as one picture, can.
const MERMAID_ON_A_DESIGN_FILE: &str = "mermaid: this canvas is a design file, and a diagram's \
     shapes and connectors are for boards: bring the diagram in as an SVG file with the command \
     op's file.import instead";

/// A `mermaid` op whose diagram is rendered and made into skeletons ([`prepare_diagram`]), for
/// [`run_prepared`] to draw.
pub struct PreparedDiagram {
    op: Value,
    items: Result<Vec<Skeleton>, String>,
}

/// For a `mermaid` op, the part of it that takes time and needs no board: its diagram rendered by
/// Pele and made into skeletons. A caller that holds the board on a thread that mustn't wait (the
/// app's UI thread) runs this elsewhere and hands what it made to [`run_prepared`]: rendering
/// takes well under a millisecond for a typical diagram, about 60 ms the first time, while Pele
/// loads, but up to 5 s for a big one, when `sawit_core::mermaid` stops it. `None` for any other
/// op.
pub fn prepare_diagram(op: &Value) -> Option<PreparedDiagram> {
    (op.get("op").and_then(Value::as_str) == Some("mermaid")).then(|| PreparedDiagram {
        op: op.clone(),
        items: diagram_skeletons(op),
    })
}

/// Draws a `mermaid` op's diagram that [`prepare_diagram`] made ready, and answers the op as
/// [`run`] does. It is the part of the op that needs the board, run as `run` runs every op.
pub fn run_prepared(board: &mut BoardDoc, prepared: PreparedDiagram) -> Value {
    let PreparedDiagram { op, items } = prepared;
    run_guarded(board, &op, |board, op| {
        before_op(board, "mermaid", op).unwrap_or_else(|| {
            super::work::in_work(board, op, |board, op| draw_diagram(board, op, items))
        })
    })
}

pub(super) fn mermaid(board: &mut BoardDoc, op: &Value) -> Value {
    let items = diagram_skeletons(op);
    draw_diagram(board, op, items)
}

/// Draws `items`, the skeletons of the diagram of `op` (or why there are none), where `op` says,
/// and answers the op.
/// Drawn into a work's frame ([`super::work::INTO_FRAME`]), its layers go into the frame, those a
/// frame can hold.
fn draw_diagram(board: &mut BoardDoc, op: &Value, items: Result<Vec<Skeleton>, String>) -> Value {
    let into = op.get(super::work::INTO_FRAME).and_then(Value::as_str);
    let items = items.map(|mut items| {
        for item in items.iter_mut().filter(|item| item.frame_id.is_none()) {
            item.frame_id = into.map(str::to_string);
        }
        items
    });
    let drawn = items.and_then(|items| {
        let (left, top, _, _) = skeleton::bounds(&items).ok_or("the diagram is empty")?;
        let (x, y) = diagram_corner(board, op).map_err(|error| error.to_string())?;
        skeleton::apply(board, &items, (x - left, y - top)).map_err(|error| error.to_string())
    });
    match drawn {
        Ok(applied) => {
            let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
            if !applied.notes.is_empty() {
                reply["notes"] = json!(applied.notes);
            }
            reply
        }
        Err(error) if needs_a_board(board, &error) => failure(MERMAID_ON_A_DESIGN_FILE),
        Err(error) => failure(format!("mermaid: {error}")),
    }
}

/// The skeletons of the diagram in `op`'s `source`: rendered by Pele and made into board shapes
/// (`crate::mermaid::to_skeletons`). A diagram stopped for taking too long isn't tried a second
/// time ([`second_try`]).
fn diagram_skeletons(op: &Value) -> Result<Vec<Skeleton>, String> {
    let source = op.get("source").and_then(Value::as_str);
    let Some(source) = source.filter(|source| !source.trim().is_empty()) else {
        return Err("`source` must be a non-empty string".into());
    };
    let palette = Palette::default();
    let diagram = match render(source, &palette) {
        Ok(diagram) => diagram,
        // A diagram that fails both tries is reported in the words of the first.
        Err(first) => match second_try(source, &first) {
            Some(again) => render(&again, &palette).map_err(|_| first)?,
            None => return Err(first),
        },
    };
    crate::mermaid::to_skeletons(&diagram)
}

/// The Mermaid `source` to try again, after its first render failed with `error`: double quotes
/// trip up some diagram types, so a source that has them is tried once more with single ones, as
/// Excalidraw's own Mermaid import does. Not a diagram stopped for taking too long, which the
/// second try took as long again: the window froze for twice the render's time limit, 10 s
/// (CANVAS-CORE-10).
pub(super) fn second_try(source: &str, error: &str) -> Option<String> {
    let again = source.contains('"') && !sawit_core::mermaid::took_too_long(error);
    again.then(|| source.replace('"', "'"))
}

/// Where the `mermaid` op puts a diagram's top left corner: at the op's `x` and `y`. What it leaves
/// out is [`MERMAID_GAP`] right of what the page on show holds and level with its top, or 0 on an
/// empty page.
fn diagram_corner(board: &BoardDoc, op: &Value) -> Result<(f64, f64), BoardError> {
    let page = board.session.page()?;
    let held = board.doc()?.page_content_bounds(page);
    let (x, y) = held.map_or((0.0, 0.0), |held| (held.x1 + MERMAID_GAP, held.y0));
    let given = |key| op.get(key).and_then(Value::as_f64);
    Ok((given("x").unwrap_or(x), given("y").unwrap_or(y)))
}
