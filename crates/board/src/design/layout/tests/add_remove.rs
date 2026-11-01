//! Adding auto layout to the selected frames and removing it.

use super::*;

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
        assert!(
            matches!(
                refused,
                Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
            ),
            "{refused:?}"
        );
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
        assert!(
            matches!(
                refused,
                Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
            ),
            "{refused:?}"
        );
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
