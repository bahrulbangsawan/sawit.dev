//! The sections the panel shows for the layers, and its header.

use super::*;

#[test]
fn sections_by_kind() {
    let mut f = fixture();
    let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
    let others = [
        rectangle(&mut f.board, "A", 300.0),
        rectangle(&mut f.board, "B", 400.0),
    ];
    let group = f
        .board
        .execute("object.group", &json!({"ids": others}))
        .unwrap();
    let group = NodeId(group["id"].as_u64().unwrap());

    let rects = read(&f.board, &[f.r1, f.r2]).sections();
    assert!(!rects.contains(&Section::Typography));
    assert!(!rects.contains(&Section::AutoLayout));

    let text = read(&f.board, &[f.text]).sections();
    assert!(text.contains(&Section::Typography));
    // A text that sits among rectangles still brings its Typography along.
    let both = read(&f.board, &[f.r1, f.text]).sections();
    assert!(both.contains(&Section::Typography));

    assert!(
        read(&f.board, &[frame])
            .sections()
            .contains(&Section::AutoLayout)
    );
    // Auto layout needs every layer to be a frame.
    let mixed = read(&f.board, &[frame, f.r1]).sections();
    assert!(!mixed.contains(&Section::AutoLayout));

    // A group has no fill or stroke of its own, unless something else is selected too.
    let groups = read(&f.board, &[group]).sections();
    assert!(!groups.contains(&Section::Fill) && !groups.contains(&Section::Stroke));
    let groups_and_a_rectangle = read(&f.board, &[group, f.r1]).sections();
    assert!(groups_and_a_rectangle.contains(&Section::Fill));
    assert!(groups_and_a_rectangle.contains(&Section::Stroke));
}

#[test]
fn sections_come_in_the_panels_order() {
    use Section::*;
    let mut f = fixture();
    let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
    assert_eq!(
        read(&f.board, &[f.r1]).sections(),
        [Header, Position, Appearance, Fill, Stroke, Effects, Export]
    );
    assert_eq!(
        read(&f.board, &[frame]).sections(),
        [
            Header, Position, AutoLayout, Appearance, Fill, Stroke, Effects, Export
        ]
    );
    assert_eq!(
        read(&f.board, &[f.text]).sections(),
        [
            Header, Position, Appearance, Fill, Stroke, Effects, Typography, Export
        ]
    );
}

#[test]
fn header_names_kind_or_count() {
    let f = fixture();
    assert_eq!(read(&f.board, &[f.r1]).header(), "Rectangle");
    assert_eq!(read(&f.board, &[f.text]).header(), "Text");
    assert_eq!(read(&f.board, &[f.r1, f.r2]).header(), "2 layers");
    assert_eq!(read(&f.board, &[f.r1, f.r2, f.text]).header(), "3 layers");
}

#[test]
fn nothing_selected_shows_nothing() {
    let f = fixture();
    let sel = read(&f.board, &[]);
    assert!(sel.ids.is_empty());
    assert!(sel.sections().is_empty());
    assert_eq!(sel.header(), "");
    assert_eq!(sel.get("x"), Mixed::Absent);
    assert_eq!(sel.get("visible"), Mixed::Absent);
}

#[test]
fn layers_the_file_lacks_and_repeats_are_left_out() {
    let f = fixture();
    let sel = read(&f.board, &[f.r1, NodeId(9_999), f.r1, f.r2]);
    assert_eq!(sel.ids, [f.r1, f.r2]);
    assert_eq!(sel.header(), "2 layers");
    assert_eq!(
        sel.set("opacity", json!(0.5)).params["ids"],
        json!([f.r1, f.r2])
    );
}
