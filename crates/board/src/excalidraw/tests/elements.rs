//! Each kind of element: labels, texts, lines, frames, images, colours and sizes.

use super::*;

/// The first solid fill and stroke of layer `id`, as `#RRGGBB`.
fn paints(board: &BoardDoc, id: &str) -> (Option<String>, Option<String>) {
    let doc = board.doc().unwrap();
    let node = doc.node(id.parse().unwrap()).unwrap();
    let first = |paints: &[figcraft_color::Paint]| {
        paints
            .iter()
            .find_map(figcraft_color::Paint::solid_color)
            .map(figcraft_color::Color::to_css_hex)
    };
    (first(&node.fills), first(&node.strokes))
}

#[test]
fn deleted_elements_are_skipped_without_a_word() {
    let mut deleted = rectangle("gone", 0.0, 0.0);
    deleted["isDeleted"] = json!(true);
    let mut deleted_text = label("gone-label", "gone", "bye");
    deleted_text["isDeleted"] = json!(true);
    let imported = read(json!([deleted, deleted_text, rectangle("kept", 0.0, 0.0)]));
    assert_eq!(imported.notes, Vec::<String>::new());
    assert_eq!(imported.skeletons.len(), 1);
    assert_eq!(imported.skeletons[0].id.as_deref(), Some("kept"));
}

#[test]
fn a_bound_text_is_its_containers_label() {
    let mut wrapped = label("t1", "box", "wrapped\nby\nexcalidraw");
    // What was typed is `originalText`; `text` has been wrapped to the container's width.
    wrapped["originalText"] = json!("wrapped by excalidraw");
    let imported = read(json!([
        rectangle("box", 0.0, 0.0),
        wrapped,
        {"id": "oval", "type": "ellipse", "x": 200, "y": 0, "width": 80, "height": 80},
        label("t2", "oval", "Oval"),
        {"id": "diamond", "type": "diamond", "x": 400, "y": 0, "width": 80, "height": 80},
        label("t3", "diamond", "Diamond"),
        {"id": "arrow", "type": "arrow", "x": 0, "y": 200, "points": [[0, 0], [100, 0]]},
        label("t4", "arrow", "calls"),
    ]));
    assert_eq!(imported.notes, Vec::<String>::new());
    // The labels are not elements of their own.
    let ids: Vec<&str> = imported
        .skeletons
        .iter()
        .filter_map(|s| s.id.as_deref())
        .collect();
    assert_eq!(ids, ["box", "oval", "diamond", "arrow"]);
    let text = |id: &str| {
        with_id(&imported, id)
            .label
            .as_ref()
            .map(|l| l.text.clone())
    };
    assert_eq!(text("box").as_deref(), Some("wrapped by excalidraw"));
    assert_eq!(text("oval").as_deref(), Some("Oval"));
    assert_eq!(text("diamond").as_deref(), Some("Diamond"));
    assert_eq!(text("arrow").as_deref(), Some("calls"));
}

#[test]
fn a_text_that_cannot_be_a_label_stays_a_text() {
    let line = json!({"id": "line", "type": "line", "x": 0, "y": 0, "points": [[0, 0], [9, 9]]});
    let mut gone = rectangle("gone", 0.0, 0.0);
    gone["isDeleted"] = json!(true);
    let imported = read(json!([
        rectangle("box", 0.0, 0.0),
        label("first", "box", "first"),
        // A second text for the same container stays a text of its own.
        label("second", "box", "second"),
        // A container that isn't in the scene, is deleted, or is a line, can't hold one.
        label("orphan", "nowhere", "orphan"),
        gone,
        label("ghost", "gone", "ghost"),
        line,
        label("on-line", "line", "on a line"),
        // No characters: nothing to draw.
        label("empty", "box", ""),
    ]));
    let texts: Vec<(&str, &str)> = imported
        .skeletons
        .iter()
        .filter(|s| s.kind == "text")
        .map(|s| (s.id.as_deref().unwrap(), s.text.as_deref().unwrap()))
        .collect();
    assert_eq!(
        texts,
        [
            ("second", "second"),
            ("orphan", "orphan"),
            ("ghost", "ghost"),
            ("on-line", "on a line"),
        ]
    );
    let first = with_id(&imported, "box").label.as_ref().unwrap();
    assert_eq!(first.text, "first");
}

#[test]
fn lines_and_freedraws_keep_their_points() {
    let imported = read(json!([
        {"id": "bend", "type": "line", "x": 10, "y": 10, "points": [[0, 0], [30, 5], [60, 0]]},
        {"id": "pen", "type": "freedraw", "x": 100, "y": 100,
         "points": [[0, 0], [1.5, 2.5], [4, 4], "junk", [1], [null, 2]],
         "pressures": [0.5, 0.5, 0.5]},
    ]));
    assert_eq!(
        with_id(&imported, "pen").points,
        Some(vec![[0.0, 0.0], [1.5, 2.5], [4.0, 4.0]]),
        "a point that isn't a pair of numbers is dropped"
    );

    // A line through more than two points is ink, as is a freedraw; neither is a connector.
    let mut board = board();
    import(
        &mut board,
        &scene(json!([
            {"id": "bend", "type": "line", "x": 10, "y": 10, "points": [[0, 0], [30, 5], [60, 0]]},
            {"id": "pen", "type": "freedraw", "x": 100, "y": 100, "points": [[0, 0], [20, 30]]},
        ])),
    )
    .unwrap();
    let elements = elements(&mut board);
    assert_eq!(of_type(&elements, "ink").len(), 2, "{elements:?}");
    assert_eq!(of_type(&elements, "connector").len(), 0);
    let bend = of_type(&elements, "ink")
        .into_iter()
        .find(|e| e["x"] == json!(10.0))
        .unwrap();
    assert_eq!(bend["width"], json!(60.0));
}

#[test]
fn frames_hold_the_elements_that_name_them() {
    let mut gone = json!({"id": "old", "type": "frame", "x": 0, "y": 0, "width": 50, "height": 50});
    gone["isDeleted"] = json!(true);
    let mut inside = rectangle("inside", 30.0, 40.0);
    inside["frameId"] = json!("frame");
    let mut orphan = rectangle("orphan", 0.0, 0.0);
    orphan["frameId"] = json!("old");
    let mut magic = rectangle("magic", 0.0, 0.0);
    magic["frameId"] = json!("ai");
    let imported = read(json!([
        inside,
        orphan,
        magic,
        gone,
        {"id": "ai", "type": "magicframe", "x": 0, "y": 0, "width": 50, "height": 50},
        {"id": "frame", "type": "frame", "x": 10, "y": 20, "width": 300, "height": 200,
         "name": "  Plan  ", "strokeColor": "#bbb", "backgroundColor": "#ff0000",
         "frameId": "frame"},
    ]));
    assert_eq!(imported.notes, ["skipped 1 embed"]);
    assert_eq!(
        with_id(&imported, "inside").frame_id.as_deref(),
        Some("frame")
    );
    // A frame that isn't drawn can't hold anything.
    assert_eq!(with_id(&imported, "orphan").frame_id, None);
    assert_eq!(with_id(&imported, "magic").frame_id, None);
    // Frames draw in a style of their own: the colours saved with one are not its look. They
    // don't nest, either.
    let frame = with_id(&imported, "frame");
    assert_eq!(frame.name.as_deref(), Some("Plan"));
    assert_eq!(
        (
            &frame.stroke_color,
            &frame.background_color,
            &frame.frame_id
        ),
        (&None, &None, &None)
    );

    // On the board the section holds the rectangle, which keeps its place on the page.
    let mut board = board();
    import(
        &mut board,
        &scene(json!([
            {"id": "inside", "type": "rectangle", "x": 30, "y": 40, "width": 100, "height": 50,
             "frameId": "frame"},
            {"id": "frame", "type": "frame", "x": 10, "y": 20, "width": 300, "height": 200},
        ])),
    )
    .unwrap();
    let elements = elements(&mut board);
    assert_eq!(elements.len(), 1, "{elements:?}");
    let doc = board.doc().unwrap();
    let section = doc
        .node(elements[0]["id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    let rectangle = section.children[0];
    let at = doc.world_bounds(rectangle).unwrap();
    assert_eq!((at.x0, at.y0), (30.0, 40.0));
}

#[test]
fn an_image_shows_the_picture_in_the_files_of_the_scene() {
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let image = |id: &str, file: &str| {
        json!({"id": id, "type": "image", "x": 10, "y": 20, "width": 60, "height": 30,
                   "fileId": file, "status": "saved", "scale": [1, 1]})
    };
    let mut scene = scene(json!([
        image("png", "f-png"),
        image("jpg", "f-jpg"),
        image("svg", "f-svg"),
        image("odd", "f-odd"),
        image("lost", "f-nowhere"),
        image("plain", "f-plain"),
        image("blank", "f-blank"),
        {"id": "no-file-id", "type": "image", "x": 0, "y": 0},
        {"id": "arrow", "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [100, 100]],
         "startBinding": {"elementId": "lost", "focus": 0, "gap": 1}},
    ]));
    let url = |media: &str| format!("data:{media};base64,{PNG}");
    scene["files"] = json!({
        "f-png": {"mimeType": "image/png", "id": "f-png", "dataURL": url("image/png")},
        "f-jpg": {"mimeType": "image/jpeg", "dataURL": url("image/jpeg")},
        "f-svg": {"mimeType": "image/svg+xml", "dataURL": url("image/svg+xml")},
        // The type is in the URL only, and unknown: the importer reads the bytes.
        "f-odd": {"dataURL": url("application/octet-stream")},
        // Not base64, or nothing in it.
        "f-plain": {"mimeType": "image/svg+xml", "dataURL": "data:image/svg+xml;utf8,<svg/>"},
        "f-blank": {"mimeType": "image/png", "dataURL": "data:image/png;base64,"},
    });

    let imported = to_skeletons(&scene).unwrap();
    let picture = |id: &str| {
        let image = with_id(&imported, id);
        (image.data.as_deref(), image.name.as_deref())
    };
    assert_eq!(picture("png"), (Some(PNG), Some("Image.png")));
    assert_eq!(picture("jpg"), (Some(PNG), Some("Image.jpg")));
    assert_eq!(picture("svg"), (Some(PNG), Some("Image.svg")));
    assert_eq!(picture("odd"), (Some(PNG), Some("Image.png")));
    assert_eq!(
        (
            with_id(&imported, "png").width,
            with_id(&imported, "png").height
        ),
        (Some(60.0), Some(30.0))
    );
    // The ones with no picture are skipped, one note for them all.
    assert_eq!(
        imported.notes,
        ["skipped 4 images with no picture in the scene"]
    );
    let kept: Vec<&str> = imported
        .skeletons
        .iter()
        .filter_map(|s| s.id.as_deref())
        .collect();
    assert_eq!(kept, ["png", "jpg", "svg", "odd", "arrow"]);
    // An arrow bound to an image that isn't drawn is free at its own start.
    let arrow = with_id(&imported, "arrow");
    assert_eq!(arrow.start.as_ref().unwrap().id, None);

    // A picture comes on the board as an image layer of the size the scene gives it.
    let mut board = board();
    let mut one = scene.clone();
    one["elements"] = json!([image("png", "f-png")]);
    assert_eq!(import(&mut board, &one).unwrap(), Vec::<String>::new());
    let elements = elements(&mut board);
    assert_eq!(of_type(&elements, "image").len(), 1, "{elements:?}");
    assert_eq!(elements[0]["width"], json!(60.0));
    assert_eq!(elements[0]["height"], json!(30.0));

    // With no `files` at all there is no picture either.
    let none = json!({"elements": [image("png", "f-png")]});
    let imported = to_skeletons(&none).unwrap();
    assert!(imported.skeletons.is_empty());
    assert_eq!(
        imported.notes,
        ["skipped 1 image with no picture in the scene"]
    );
}

#[test]
fn colours_come_across_when_the_engine_can_read_them() {
    let imported = read(json!([
        {"id": "ok", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
         "strokeColor": "#1E1E1E", "backgroundColor": "transparent"},
        {"id": "css", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
         "strokeColor": "rgba(255, 0, 0, 0.5)", "backgroundColor": "teal"},
        {"id": "bad", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
         "strokeColor": "currentColor", "backgroundColor": 12},
        {"id": "blank", "type": "ellipse", "x": 0, "y": 0, "width": 10, "height": 10,
         "strokeColor": "", "backgroundColor": null},
    ]));
    let colours = |id: &str| {
        let item = with_id(&imported, id);
        (
            item.stroke_color.as_deref(),
            item.background_color.as_deref(),
        )
    };
    assert_eq!(colours("ok"), (Some("#1E1E1E"), Some("transparent")));
    assert_eq!(colours("css"), (Some("rgba(255, 0, 0, 0.5)"), Some("teal")));
    // One the engine would refuse is left out, so that it can't fail the import.
    assert_eq!(colours("bad"), (None, None));
    assert_eq!(colours("blank"), (None, None));

    let mut board = board();
    let notes = import(
        &mut board,
        &scene(json!([
            {"id": "bad", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
             "strokeColor": "currentColor", "backgroundColor": "nonsense"},
        ])),
    )
    .unwrap();
    assert_eq!(notes, Vec::<String>::new());
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn a_text_is_coloured_by_its_stroke_color_and_has_no_background() {
    let mut text = json!({"id": "t", "type": "text", "x": 5, "y": 6, "text": "Hi",
                              "originalText": "Hi", "strokeColor": "#00FF00",
                              "backgroundColor": "#FF0000"});
    text["containerId"] = json!(null);
    let imported = read(json!([text.clone()]));
    let item = with_id(&imported, "t");
    assert_eq!(item.stroke_color.as_deref(), Some("#00FF00"));
    assert_eq!(item.background_color, None);

    let mut board = board();
    import(&mut board, &scene(json!([text]))).unwrap();
    let elements = elements(&mut board);
    let id = elements[0]["id"].as_str().unwrap();
    // The glyphs are green, and nothing is drawn behind or around them.
    assert_eq!(paints(&board, id), (Some("#00FF00".into()), None));
}

#[test]
fn a_text_wraps_at_its_width_only_when_excalidraw_fixed_it() {
    let text = |id: &str, auto: Value| {
        let mut text = json!({"id": id, "type": "text", "x": 0, "y": 0, "width": 120,
                                  "height": 25, "text": "Hello"});
        if !auto.is_null() {
            text["autoResize"] = auto;
        }
        text
    };
    let imported = read(json!([
        text("grows", json!(true)),
        text("older", json!(null)),
        text("fixed", json!(false)),
    ]));
    // A text that grows with what is typed is as wide as it is long.
    assert_eq!(with_id(&imported, "grows").width, None);
    assert_eq!(with_id(&imported, "older").width, None);
    assert_eq!(with_id(&imported, "fixed").width, Some(120.0));
    // `text` is the text when there is no `originalText`; none at all is nothing to draw.
    assert_eq!(with_id(&imported, "grows").text.as_deref(), Some("Hello"));
    let none = read(json!([{"id": "x", "type": "text", "x": 0, "y": 0}]));
    assert!(none.skeletons.is_empty());
    assert_eq!(none.notes, Vec::<String>::new());
}

#[test]
fn a_box_the_engine_would_refuse_is_held_to_the_sides_it_takes() {
    // A rectangle of no width is a line to the eye, and `board.shape` takes none under 1.
    let scene = scene(json!([
        {"id": "thin", "type": "rectangle", "x": 0, "y": 0, "width": 0, "height": 80},
        {"id": "huge", "type": "ellipse", "x": 0, "y": 0, "width": 1.0e9, "height": 5},
        {"id": "frame", "type": "frame", "x": 0, "y": 0, "width": 0, "height": 0},
        {"id": "none", "type": "diamond", "x": 0, "y": 0},
    ]));
    let imported = to_skeletons(&scene).unwrap();
    let sides = |id: &str| (with_id(&imported, id).width, with_id(&imported, id).height);
    assert_eq!(sides("thin"), (Some(1.0), Some(80.0)));
    assert_eq!(sides("huge"), (Some(100_000.0), Some(5.0)));
    assert_eq!(sides("frame"), (Some(1.0), Some(1.0)));
    // A side that isn't given stays open: the engine draws its own size.
    assert_eq!(sides("none"), (None, None));

    let mut board = board();
    let notes = import(&mut board, &scene).unwrap();
    assert_eq!(notes, Vec::<String>::new());
    assert_eq!(elements(&mut board).len(), 4);
}

#[test]
fn rounded_corners_come_across() {
    let rounded = |id: &str, roundness: Value| {
        let mut item = rectangle(id, 0.0, 0.0);
        item["roundness"] = roundness;
        item
    };
    let mut board = board();
    import(
        &mut board,
        &scene(json!([
            rounded("round", json!({"type": 3})),
            rounded("sharp", json!(null)),
            {"id": "oval", "type": "ellipse", "x": 0, "y": 0, "width": 50, "height": 50,
             "roundness": {"type": 2}},
        ])),
    )
    .unwrap();
    let elements = elements(&mut board);
    assert_eq!(of_type(&elements, "roundedRectangle").len(), 1);
    assert_eq!(of_type(&elements, "rectangle").len(), 1);
    assert_eq!(of_type(&elements, "ellipse").len(), 1);
}
