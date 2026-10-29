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
mod tests {
    use figcraft_doc::NodeId;
    use serde_json::json;

    use super::*;
    use crate::design::Mixed;
    use crate::design::testing::{
        apply, design_file, inspect, make, node_set, read, run, same, undo_steps,
    };
    use crate::doc::{BoardDoc, BoardError};

    /// A 200 × 100 frame called `name` at (`x`, 0), with two 30 × 30 rectangles in it and no auto
    /// layout.
    fn frame_with_two_children(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
        let frame = make(
            board,
            json!({"type": "frame", "name": name, "x": x, "y": 0, "width": 200, "height": 100}),
        );
        for child_x in [10, 60] {
            make(
                board,
                json!({"type": "rectangle", "parent": frame, "x": child_x, "y": 10,
                       "width": 30, "height": 30}),
            );
        }
        frame
    }

    /// A 200 × 100 frame at (`x`, 0) with auto layout in `direction`, no padding and no gap, that
    /// keeps its size. It holds one 20 × 20 rectangle, so where that rectangle ends up says where
    /// the layout puts its content. Returns the frame and the rectangle.
    fn frame_with_one_child(board: &mut BoardDoc, direction: &str, x: f64) -> (NodeId, NodeId) {
        let frame = make(
            board,
            json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100}),
        );
        let child = make(
            board,
            json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20}),
        );
        node_set(board, &[frame], json!({"layout": direction}));
        (frame, child)
    }

    /// The auto gaps a frame's `mainAlign` can be, as `node.set` spells them.
    const AUTO_GAPS: [&str; 3] = ["spaceBetween", "spaceAround", "spaceEvenly"];

    /// A 200 × 100 frame at (`x`, 0) with auto layout in `direction` and the main alignment `main`,
    /// no padding and no gap, that keeps its size. It holds two 20 × 20 rectangles, so where they
    /// end up says how the layout spreads them: an auto gap puts the free space between them.
    /// Returns the frame and the rectangles.
    fn spread_frame(
        board: &mut BoardDoc,
        direction: &str,
        main: &str,
        x: f64,
    ) -> (NodeId, [NodeId; 2]) {
        let frame = make(
            board,
            json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100}),
        );
        let children = [0, 1].map(|_| {
            make(
                board,
                json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20}),
            )
        });
        node_set(
            board,
            &[frame],
            json!({"layout": direction, "mainAlign": main}),
        );
        (frame, children)
    }

    /// Where layer `id` sits in its frame.
    fn place(board: &BoardDoc, id: NodeId) -> (f64, f64) {
        let layer = inspect(board, id);
        (layer["x"].as_f64().unwrap(), layer["y"].as_f64().unwrap())
    }

    /// The layers on the page, to tell whether a command made one.
    fn layer_count(board: &BoardDoc) -> usize {
        board.doc().unwrap().nodes().count()
    }

    #[test]
    fn add_layout_then_gap() {
        let mut board = design_file();
        let frame = frame_with_two_children(&mut board, "Card", 0.0);

        let edit = add_layout(&read(&board, &[frame]));
        assert_eq!(edit.command, "layout.add");
        apply(&mut board, &edit);
        assert!(matches!(
            read(&board, &[frame]).get("layout"),
            Mixed::Same(_)
        ));

        let edit = read(&board, &[frame]).set("gap", json!(12.0));
        apply(&mut board, &edit);
        assert_eq!(read(&board, &[frame]).get("gap"), same(json!(12.0)));
    }

    #[test]
    fn add_layout_keeps_the_children_in_the_frame() {
        let mut board = design_file();
        let frame = frame_with_two_children(&mut board, "Card", 0.0);
        let layers = layer_count(&board);
        let children = board.doc().unwrap().node(frame).unwrap().children.clone();

        run(&mut board, &[frame], add_layout);

        assert_eq!(layer_count(&board), layers, "no frame around the frame");
        let doc = board.doc().unwrap();
        assert_eq!(doc.node(frame).unwrap().children, children);
        assert!(
            children
                .iter()
                .all(|c| doc.node(*c).unwrap().parent == Some(frame))
        );
    }

    /// `layout.add` wraps frames in a new frame as soon as one of them has an auto layout, so the
    /// edit names only the frames that have none.
    #[test]
    fn add_layout_adds_to_the_frames_that_have_none_and_wraps_nothing() {
        let mut board = design_file();
        let (with, _) = frame_with_one_child(&mut board, "vertical", 0.0);
        let without = frame_with_two_children(&mut board, "Card", 400.0);
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 800, "width": 10, "height": 10}),
        );
        let layers = layer_count(&board);
        let had = read(&board, &[with]);

        let edit = add_layout(&read(&board, &[with, without, rectangle]));
        assert_eq!(edit.params["ids"], json!([without]));
        apply(&mut board, &edit);

        assert_eq!(layer_count(&board), layers, "nothing was wrapped");
        assert!(matches!(
            read(&board, &[without]).get("layout"),
            Mixed::Same(_)
        ));
        assert_eq!(
            read(&board, &[with]),
            had,
            "the frame that had one is as it was"
        );
    }

    #[test]
    fn add_layout_without_a_frame_to_add_to_is_refused_and_changes_nothing() {
        let mut board = design_file();
        let (with, _) = frame_with_one_child(&mut board, "vertical", 0.0);
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 800, "width": 10, "height": 10}),
        );
        for ids in [vec![with], vec![rectangle], vec![with, rectangle], vec![]] {
            let edit = add_layout(&read(&board, &ids));
            assert_eq!(edit.params["ids"], json!([]), "{ids:?}");
            let revision = board.revision();
            let refused = board.execute(edit.command, &edit.params);
            assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
            assert_eq!(board.revision(), revision, "{ids:?}");
        }
    }

    #[test]
    fn remove_layout_takes_auto_layout_off_and_keeps_the_children() {
        let mut board = design_file();
        let (frame, child) = frame_with_one_child(&mut board, "horizontal", 0.0);
        assert!(matches!(
            read(&board, &[frame]).get("layout"),
            Mixed::Same(_)
        ));

        let edit = remove_layout(&read(&board, &[frame]));
        assert_eq!(edit.command, "layout.remove");
        apply(&mut board, &edit);

        assert_eq!(read(&board, &[frame]).get("layout"), Mixed::Absent);
        let doc = board.doc().unwrap();
        assert_eq!(doc.node(child).unwrap().parent, Some(frame));
    }

    #[test]
    fn remove_layout_names_only_the_frames_that_have_one() {
        let mut board = design_file();
        let (with, _) = frame_with_one_child(&mut board, "vertical", 0.0);
        let without = frame_with_two_children(&mut board, "Card", 400.0);
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 800, "width": 10, "height": 10}),
        );

        let edit = remove_layout(&read(&board, &[with, without, rectangle]));
        assert_eq!(edit.params["ids"], json!([with]));
        apply(&mut board, &edit);
        assert_eq!(read(&board, &[with]).get("layout"), Mixed::Absent);
    }

    #[test]
    fn remove_layout_without_a_layout_to_remove_is_refused_and_changes_nothing() {
        let mut board = design_file();
        let without = frame_with_two_children(&mut board, "Card", 0.0);
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 800, "width": 10, "height": 10}),
        );
        for ids in [
            vec![without],
            vec![rectangle],
            vec![without, rectangle],
            vec![],
        ] {
            let edit = remove_layout(&read(&board, &ids));
            assert_eq!(edit.params["ids"], json!([]), "{ids:?}");
            let revision = board.revision();
            let refused = board.execute(edit.command, &edit.params);
            assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
            assert_eq!(board.revision(), revision, "{ids:?}");
        }
    }

    #[test]
    fn adding_and_removing_a_layout_are_one_undo_step_each() {
        let mut board = design_file();
        let frame = frame_with_two_children(&mut board, "Card", 0.0);
        let steps = undo_steps(&board);

        run(&mut board, &[frame], add_layout);
        assert_eq!(undo_steps(&board), steps + 1);
        run(&mut board, &[frame], remove_layout);
        assert_eq!(undo_steps(&board), steps + 2);

        board.execute("edit.undo", &json!({})).unwrap();
        assert!(matches!(
            read(&board, &[frame]).get("layout"),
            Mixed::Same(_)
        ));
        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(read(&board, &[frame]).get("layout"), Mixed::Absent);
    }

    #[test]
    fn align_cell_maps_to_main_and_cross() {
        let mut board = design_file();
        let (frame, _) = frame_with_one_child(&mut board, "horizontal", 0.0);

        let edit = align_cell(&read(&board, &[frame]), 1, 2);
        apply(&mut board, &edit);

        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("max")));
        assert_eq!(sel.get("crossAlign"), same(json!("center")));
    }

    #[test]
    fn a_vertical_layout_runs_its_main_axis_along_the_rows() {
        let mut board = design_file();
        let (frame, _) = frame_with_one_child(&mut board, "vertical", 0.0);

        run(&mut board, &[frame], |sel| align_cell(sel, 1, 2));
        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("center")));
        assert_eq!(sel.get("crossAlign"), same(json!("max")));

        run(&mut board, &[frame], |sel| align_cell(sel, 0, 0));
        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("min")));
        assert_eq!(sel.get("crossAlign"), same(json!("min")));
    }

    /// A grid places its children in cells, and the frame's alignment moves them within a cell:
    /// the main alignment across the columns, the cross alignment down the rows, as in a
    /// horizontal layout.
    #[test]
    fn a_grid_aligns_as_a_horizontal_layout_does() {
        let mut board = design_file();
        let (frame, _) = frame_with_one_child(&mut board, "grid", 0.0);
        assert_eq!(read(&board, &[frame]).get("layout"), same(json!("grid")));

        run(&mut board, &[frame], |sel| align_cell(sel, 2, 1));
        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("center")));
        assert_eq!(sel.get("crossAlign"), same(json!("max")));
    }

    /// The cell is where the content goes in the frame, whichever way the layout runs: the child
    /// lands in that part of a frame that keeps its size.
    #[test]
    fn a_cell_puts_the_content_in_that_part_of_the_frame_whatever_the_direction() {
        let columns = [0.0, 90.0, 180.0];
        let rows = [0.0, 40.0, 80.0];
        for direction in ["horizontal", "vertical"] {
            for row in 0..3u8 {
                for col in 0..3u8 {
                    let mut board = design_file();
                    let (frame, child) = frame_with_one_child(&mut board, direction, 0.0);
                    run(&mut board, &[frame], |sel| align_cell(sel, row, col));
                    assert_eq!(
                        place(&board, child),
                        (columns[usize::from(col)], rows[usize::from(row)]),
                        "{direction}, row {row}, column {col}"
                    );
                }
            }
        }
    }

    #[test]
    fn frames_that_run_both_ways_each_go_to_the_clicked_cell_in_one_undo_step() {
        let mut board = design_file();
        let (across, across_child) = frame_with_one_child(&mut board, "horizontal", 0.0);
        let (down, down_child) = frame_with_one_child(&mut board, "vertical", 400.0);
        let sel = read(&board, &[across, down]);
        let steps = undo_steps(&board);

        let edit = align_cell(&sel, 0, 2);
        assert_eq!(edit.command, "command.batch");
        apply(&mut board, &edit);

        assert_eq!(undo_steps(&board), steps + 1);
        assert_eq!(place(&board, across_child), (180.0, 0.0));
        assert_eq!(place(&board, down_child), (180.0, 0.0));
        // Each keeps its own direction, and aligns along it.
        let across = read(&board, &[across]);
        assert_eq!(across.get("layout"), same(json!("horizontal")));
        assert_eq!(across.get("mainAlign"), same(json!("max")));
        assert_eq!(across.get("crossAlign"), same(json!("min")));
        let down = read(&board, &[down]);
        assert_eq!(down.get("layout"), same(json!("vertical")));
        assert_eq!(down.get("mainAlign"), same(json!("min")));
        assert_eq!(down.get("crossAlign"), same(json!("max")));

        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(place(&board, across_child), (0.0, 0.0));
        assert_eq!(place(&board, down_child), (0.0, 0.0));
    }

    #[test]
    fn frames_that_run_the_same_way_are_one_node_set() {
        let mut board = design_file();
        let (a, _) = frame_with_one_child(&mut board, "vertical", 0.0);
        let (b, _) = frame_with_one_child(&mut board, "vertical", 400.0);
        let edit = align_cell(&read(&board, &[a, b]), 2, 1);
        assert_eq!(edit.command, "node.set");
        assert_eq!(
            edit.params,
            json!({"ids": [a, b], "props": {"mainAlign": "max", "crossAlign": "center"}})
        );
    }

    /// A click on the grid leaves an auto gap as it is. The grid has no cell for one and nothing
    /// else in the panel brings one back, so writing `mainAlign` from the cell would end the
    /// distribution for good: the children stay where the mode puts them along the layout, and only
    /// the alignment across it follows the cell.
    #[test]
    fn an_auto_gap_stays_through_every_cell() {
        let words = ["min", "center", "max"];
        let columns = [0.0, 90.0, 180.0];
        let rows = [0.0, 40.0, 80.0];
        for direction in ["horizontal", "vertical"] {
            let horizontal = direction == "horizontal";
            for main in AUTO_GAPS {
                for row in 0..3u8 {
                    for col in 0..3u8 {
                        let at = format!("{direction}, {main}, row {row}, column {col}");
                        let mut board = design_file();
                        let (frame, children) = spread_frame(&mut board, direction, main, 0.0);
                        let before = children.map(|child| place(&board, child));
                        let apart = if horizontal {
                            before[1].0 - before[0].0
                        } else {
                            before[1].1 - before[0].1
                        };
                        assert!(apart > 20.0, "{at}: the auto gap spreads the children");

                        run(&mut board, &[frame], |sel| align_cell(sel, row, col));

                        let sel = read(&board, &[frame]);
                        assert_eq!(sel.get("mainAlign"), same(json!(main)), "{at}");
                        let across = if horizontal { row } else { col };
                        let cross = json!(words[usize::from(across)]);
                        assert_eq!(sel.get("crossAlign"), same(cross), "{at}");
                        for (child, was) in children.into_iter().zip(before) {
                            let expected = if horizontal {
                                (was.0, rows[usize::from(row)])
                            } else {
                                (columns[usize::from(col)], was.1)
                            };
                            assert_eq!(place(&board, child), expected, "{at}");
                        }
                    }
                }
            }
        }
    }

    /// The cell sends the cross alignment alone to a frame that keeps its auto gap: the row for a
    /// horizontal layout, the column for a vertical one.
    #[test]
    fn a_frame_that_keeps_its_auto_gap_is_sent_the_cross_alignment_only() {
        let mut board = design_file();
        let (across, _) = spread_frame(&mut board, "horizontal", "spaceBetween", 0.0);
        let (down, _) = spread_frame(&mut board, "vertical", "spaceAround", 400.0);

        let edit = align_cell(&read(&board, &[across]), 0, 2);
        assert_eq!(edit.command, "node.set");
        assert_eq!(
            edit.params,
            json!({"ids": [across], "props": {"crossAlign": "min"}})
        );
        let edit = align_cell(&read(&board, &[down]), 0, 2);
        assert_eq!(edit.command, "node.set");
        assert_eq!(
            edit.params,
            json!({"ids": [down], "props": {"crossAlign": "max"}})
        );
    }

    /// Whichever auto gap a frame has, it keeps it, so frames of one direction with different
    /// modes are still one `node.set`.
    #[test]
    fn frames_that_keep_an_auto_gap_are_one_node_set_whichever_the_mode() {
        let mut board = design_file();
        let (a, _) = spread_frame(&mut board, "vertical", "spaceBetween", 0.0);
        let (b, _) = spread_frame(&mut board, "vertical", "spaceEvenly", 400.0);

        let edit = align_cell(&read(&board, &[a, b]), 2, 1);
        assert_eq!(edit.command, "node.set");
        assert_eq!(
            edit.params,
            json!({"ids": [a, b], "props": {"crossAlign": "center"}})
        );
    }

    /// One frame of each kind a click can meet: the layout runs along the columns or down the rows,
    /// and its main alignment is a position or an auto gap. Each kind gets its own `node.set`, and
    /// the batch of them is one undo step.
    #[test]
    fn frames_of_every_kind_are_one_batch_and_one_undo_step() {
        let mut board = design_file();
        let (across_plain, _) = frame_with_one_child(&mut board, "horizontal", 0.0);
        let (across_gap, _) = spread_frame(&mut board, "horizontal", "spaceBetween", 400.0);
        let (down_plain, _) = frame_with_one_child(&mut board, "vertical", 800.0);
        let (down_gap, _) = spread_frame(&mut board, "vertical", "spaceEvenly", 1200.0);
        let all = [across_plain, across_gap, down_plain, down_gap];
        let before = all.map(|frame| read(&board, &[frame]));
        let steps = undo_steps(&board);

        let edit = align_cell(&read(&board, &all), 1, 2);
        assert_eq!(edit.command, "command.batch");
        let step = |frame: NodeId, props: Value| json!({"command": "node.set", "params": {"ids": [frame], "props": props}});
        let expected = [
            step(
                across_plain,
                json!({"mainAlign": "max", "crossAlign": "center"}),
            ),
            step(across_gap, json!({"crossAlign": "center"})),
            step(
                down_plain,
                json!({"mainAlign": "center", "crossAlign": "max"}),
            ),
            step(down_gap, json!({"crossAlign": "max"})),
        ];
        let commands = edit.params["commands"].as_array().unwrap();
        assert_eq!(commands.len(), expected.len(), "{commands:?}");
        for wanted in &expected {
            assert!(commands.contains(wanted), "{wanted} is not in {commands:?}");
        }

        apply(&mut board, &edit);

        assert_eq!(undo_steps(&board), steps + 1);
        let alignment = |frame: NodeId| {
            let sel = read(&board, &[frame]);
            (sel.get("mainAlign"), sel.get("crossAlign"))
        };
        assert_eq!(
            alignment(across_plain),
            (same(json!("max")), same(json!("center")))
        );
        assert_eq!(
            alignment(across_gap),
            (same(json!("spaceBetween")), same(json!("center")))
        );
        assert_eq!(
            alignment(down_plain),
            (same(json!("center")), same(json!("max")))
        );
        assert_eq!(
            alignment(down_gap),
            (same(json!("spaceEvenly")), same(json!("max")))
        );

        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(all.map(|frame| read(&board, &[frame])), before);
    }

    /// A grid has no auto gap: the engine lays a grid out as if an auto gap were `min`, and
    /// FigCraft's own grid sets a grid's `mainAlign` from the cell. So the cell replaces it.
    #[test]
    fn a_grid_has_no_auto_gap_to_keep() {
        let mut board = design_file();
        let (frame, _) = frame_with_one_child(&mut board, "grid", 0.0);
        node_set(&mut board, &[frame], json!({"mainAlign": "spaceBetween"}));
        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("spaceBetween")));

        let edit = align_cell(&sel, 2, 1);
        assert_eq!(
            edit.params["props"],
            json!({"mainAlign": "center", "crossAlign": "max"})
        );
        apply(&mut board, &edit);

        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("mainAlign"), same(json!("center")));
        assert_eq!(sel.get("crossAlign"), same(json!("max")));
    }

    /// `node.set` skips a layer that is not a frame beside a frame, but refuses an edit whose layers
    /// are none of them frames, and a step of a batch that is refused undoes the whole batch. So the
    /// edit names no layer that is not a frame, whatever kind of frame it is beside.
    #[test]
    fn a_layer_that_is_not_a_frame_stays_out_of_the_edit_beside_every_kind_of_frame() {
        let mut board = design_file();
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 4000, "width": 10, "height": 10}),
        );
        let frames = [
            frame_with_two_children(&mut board, "Card", 0.0),
            frame_with_one_child(&mut board, "horizontal", 400.0).0,
            frame_with_one_child(&mut board, "vertical", 800.0).0,
            frame_with_one_child(&mut board, "grid", 1200.0).0,
            spread_frame(&mut board, "horizontal", "spaceBetween", 1600.0).0,
            spread_frame(&mut board, "vertical", "spaceAround", 2000.0).0,
        ];
        let before = read(&board, &[rectangle]);

        for frame in frames {
            let edit = align_cell(&read(&board, &[frame, rectangle]), 2, 0);
            assert_eq!(edit.params["ids"], json!([frame]), "{frame:?}");
            apply(&mut board, &edit);
        }
        assert_eq!(read(&board, &[rectangle]), before);
    }

    #[test]
    fn align_cell_over_layers_that_are_not_frames_is_refused_and_changes_nothing() {
        let mut board = design_file();
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "width": 10, "height": 10}),
        );
        let edit = align_cell(&read(&board, &[rectangle]), 1, 1);
        let revision = board.revision();
        let refused = board.execute(edit.command, &edit.params);
        assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
        assert_eq!(board.revision(), revision);
    }

    /// `node.set` of an alignment gives a frame without auto layout a horizontal one, so the cell
    /// of such a frame means what it does in a horizontal layout.
    #[test]
    fn a_frame_without_auto_layout_gets_a_horizontal_one() {
        let mut board = design_file();
        let frame = frame_with_two_children(&mut board, "Card", 0.0);
        assert_eq!(read(&board, &[frame]).get("layout"), Mixed::Absent);

        run(&mut board, &[frame], |sel| align_cell(sel, 0, 2));
        let sel = read(&board, &[frame]);
        assert_eq!(sel.get("layout"), same(json!("horizontal")));
        assert_eq!(sel.get("mainAlign"), same(json!("max")));
        assert_eq!(sel.get("crossAlign"), same(json!("min")));
    }

    #[test]
    fn a_layer_that_is_not_a_frame_is_skipped_not_refused() {
        let mut board = design_file();
        let (frame, child) = frame_with_one_child(&mut board, "horizontal", 0.0);
        let rectangle = make(
            &mut board,
            json!({"type": "rectangle", "x": 800, "width": 10, "height": 10}),
        );
        let before = read(&board, &[rectangle]);

        run(&mut board, &[frame, rectangle], |sel| align_cell(sel, 2, 2));

        assert_eq!(place(&board, child), (180.0, 80.0));
        assert_eq!(read(&board, &[rectangle]), before);
    }

    #[test]
    fn a_cell_past_the_grid_is_its_last_row_and_column() {
        let mut board = design_file();
        let (frame, _) = frame_with_one_child(&mut board, "horizontal", 0.0);
        let sel = read(&board, &[frame]);
        assert_eq!(align_cell(&sel, 3, 9), align_cell(&sel, 2, 2));
        assert_eq!(align_cell(&sel, 1, 200), align_cell(&sel, 1, 2));
    }

    #[test]
    fn align_cell_with_nothing_selected_is_refused_and_changes_nothing() {
        let mut board = design_file();
        frame_with_one_child(&mut board, "horizontal", 0.0);
        let edit = align_cell(&read(&board, &[]), 1, 1);
        assert_eq!(edit.params["ids"], json!([]));
        let revision = board.revision();
        let refused = board.execute(edit.command, &edit.params);
        assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
        assert_eq!(board.revision(), revision);
    }
}
