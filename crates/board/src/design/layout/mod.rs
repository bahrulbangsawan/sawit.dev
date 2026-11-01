//! The Auto layout section's edits: adding auto layout, removing it, and the 3×3 alignment grid.
//!
//! The section's fields read and write through [`DesignSelection::get`] and
//! [`DesignSelection::set`], under the keys `node.set` takes:
//!
//! - `layout` is `"horizontal"`, `"vertical"` or `"grid"`, and `wrap` lets a horizontal or vertical
//!   layout run its children onto new lines.
//! - `gap` is the space between the children along the layout, and `crossGap` the space between
//!   wrapped lines.
//! - `padding` is one number while the four sides agree and the list `[top, right, bottom, left]`
//!   when they don't; `paddingTop`, `paddingRight`, `paddingBottom` and `paddingLeft` are the sides
//!   one by one.
//! - `mainAlign` is how the children sit along the layout: `"min"`, `"center"` or `"max"`, or an
//!   auto gap, `"spaceBetween"`, `"spaceAround"` or `"spaceEvenly"`, which spreads them over the
//!   free space instead of using `gap`. `crossAlign` is how they sit across it: `"min"`,
//!   `"center"`, `"max"`, or `"baseline"` in a horizontal layout. [`align_cell`] sets them from a
//!   cell of the grid, and leaves an auto gap as it is.
//! - `sizingH` and `sizingV` are `"fixed"`, `"hug"` or `"fill"`: how a frame, or a layer inside an
//!   auto layout, sizes itself.
//!
//! For `layout`, `gap`, `crossGap`, `padding` and its sides, `mainAlign`, `crossAlign` and `wrap`,
//! `get` counts every selected frame and `set` writes to every selected layer, so a frame without
//! auto layout is a value of its own ([`DesignSelection::get`] says why).

use figcraft_doc::NodeId;
use serde_json::{Map, Value, json};

use super::{DesignSelection, Edit, Layer};

/// What the rows and the columns of the alignment grid mean along their axis: the first, the
/// middle and the last.
const CELLS: [&str; 3] = ["min", "center", "max"];

/// The edit of the button that adds auto layout: `layout.add` on the selected frames that have
/// none.
///
/// The engine's command wraps what it is given in a new frame, as Figma's Shift+A does, unless
/// every layer is a frame without auto layout. The button means "give these frames an auto
/// layout", so the edit names only those frames, and never wraps one that has a layout already or
/// a layer that is not a frame. When the selection holds no such frame the edit names no layer, and
/// the engine refuses it.
pub fn add_layout(sel: &DesignSelection) -> Edit {
    let ids = ids_where(sel, |layer| layer.frame_like && !has_layout(layer));
    Edit {
        command: "layout.add",
        params: json!({"ids": ids}),
    }
}

/// The edit of the button that removes auto layout: `layout.remove` on the selected frames that
/// have one. The children stay in their frame. When the selection holds no such frame the edit
/// names no layer, and the engine refuses it.
pub fn remove_layout(sel: &DesignSelection) -> Edit {
    Edit {
        command: "layout.remove",
        params: json!({"ids": ids_where(sel, has_layout)}),
    }
}

/// The edit of a click on a cell of the alignment grid, counting from the top left: `row` 0 is the
/// top and `col` 0 the left. A row or column past the grid is its last.
///
/// The cell is where the children go in the frame, so what it says depends on the way the layout
/// runs. A horizontal layout runs along the columns, so the column is `mainAlign` and the row is
/// `crossAlign`; a vertical one runs down the rows, so the row is `mainAlign` and the column is
/// `crossAlign`. A grid layout aligns its children within their cells as a horizontal one does.
/// A frame without auto layout is aligned as a horizontal one, because that is the layout
/// the engine gives it when `mainAlign` is written to it.
///
/// A frame whose `mainAlign` is an auto gap (`spaceBetween`, `spaceAround` or `spaceEvenly`) keeps
/// it, as FigCraft's own grid does: the grid has no cell for an auto gap and the spec's Auto layout
/// section has no other control that brings one back, so the cell sets only the `crossAlign` of
/// such a frame. A grid layout has no auto gap to keep (the engine lays an auto gap out as `min`
/// there), so the cell always sets its `mainAlign`.
///
/// The edit names frames and no other layer: `node.set` skips a layer that is not a frame beside
/// one, but refuses an edit that has no frame in it, and one refused step undoes a whole batch.
/// Frames that need different edits get one `node.set` each, told apart by the way they run and by
/// whether they keep their `mainAlign`: one step for a selection of one kind, and for a mixed
/// selection one `command.batch` of at most four, which is still one undo step. With no frame
/// among the layers the edit names the selection as it is, and the engine refuses it and says why.
pub fn align_cell(sel: &DesignSelection, row: u8, col: u8) -> Edit {
    let (row, col) = (cell(row), cell(col));
    let mut steps = Vec::new();
    for vertical in [false, true] {
        // The cell's row and column, as along the layout (main) and across it (cross).
        let (main, cross) = if vertical { (row, col) } else { (col, row) };
        for keeps_main in [false, true] {
            let ids = ids_where(sel, |layer| {
                layer.frame_like
                    && is_vertical(layer) == vertical
                    && keeps_auto_gap(layer) == keeps_main
            });
            if !ids.is_empty() {
                steps.push(set_alignment(&ids, (!keeps_main).then_some(main), cross));
            }
        }
    }
    match steps.as_slice() {
        [] => Edit {
            command: "node.set",
            params: set_alignment(&sel.ids, Some(col), row),
        },
        [only] => Edit {
            command: "node.set",
            params: only.clone(),
        },
        _ => {
            let commands: Vec<Value> = steps
                .iter()
                .map(|params| json!({"command": "node.set", "params": params}))
                .collect();
            Edit {
                command: "command.batch",
                params: json!({"label": "Set properties", "commands": commands}),
            }
        }
    }
}

/// The alignment a row or a column of the grid stands for.
fn cell(index: u8) -> &'static str {
    CELLS[usize::from(index.min(2))]
}

/// The params of a `node.set` that aligns `ids` across their layout to `cross`, and along it to
/// `main` unless that is `None`.
fn set_alignment(ids: &[NodeId], main: Option<&str>, cross: &str) -> Value {
    let mut props = Map::new();
    if let Some(main) = main {
        props.insert("mainAlign".into(), json!(main));
    }
    props.insert("crossAlign".into(), json!(cross));
    json!({"ids": ids, "props": props})
}

/// The mode of the layer's auto layout, `"horizontal"`, `"vertical"` or `"grid"`. A layer without
/// auto layout has none.
fn mode(layer: &Layer) -> Option<&str> {
    layer.props.get("layout").and_then(Value::as_str)
}

/// Whether the layer's auto layout runs down the rows.
fn is_vertical(layer: &Layer) -> bool {
    mode(layer) == Some("vertical")
}

/// Whether the layer's `mainAlign` is an auto gap that a click on the grid keeps. A grid layout has
/// none to keep: the engine reads an auto gap there as `min`.
fn keeps_auto_gap(layer: &Layer) -> bool {
    let main = layer.props.get("mainAlign").and_then(Value::as_str);
    mode(layer) != Some("grid")
        && matches!(main, Some("spaceBetween" | "spaceAround" | "spaceEvenly"))
}

/// Whether the layer has an auto layout: it holds the `layout` key, which a layer without one lacks.
fn has_layout(layer: &Layer) -> bool {
    layer.props.contains_key("layout")
}

/// The ids of the selected layers that `keep` says yes to, in the order of the selection.
fn ids_where(sel: &DesignSelection, keep: impl Fn(&Layer) -> bool) -> Vec<NodeId> {
    let layers = sel.ids.iter().zip(&sel.layers);
    layers
        .filter(|(_, layer)| keep(layer))
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests;
