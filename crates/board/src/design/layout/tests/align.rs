//! The alignment grid: what a cell sets for each kind of frame, and the auto gaps it keeps.

use super::*;

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
    assert!(
        matches!(
            refused,
            Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
        ),
        "{refused:?}"
    );
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
    assert!(
        matches!(
            refused,
            Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(board.revision(), revision);
}
