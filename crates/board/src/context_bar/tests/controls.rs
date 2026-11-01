//! The controls a selection gets, and the value each shows.

use figcraft_doc::{BOARD_COLORS, Document, FileKind, NodeKind, StrokeCap, StyleRun};
use figcraft_engine::board::text_of;

use super::*;
use crate::context_bar::selection::cap_name;
use crate::doc::DocKind;

#[test]
fn sticky_selection_shows_colours() {
    let mut b = board();
    sticky(&mut b);
    assert_eq!(bar(&b).controls, [colors(Some("yellow"))]);
}

#[test]
fn shape_selection_shows_colours_outline_and_size() {
    let mut b = board();
    shape(&mut b);
    assert_eq!(
        bar(&b).controls,
        [
            colors(Some("yellow")),
            Control::Shape {
                current: Some(BoardShape::Rectangle)
            },
            Control::FontSize { current: Some("M") },
        ]
    );
}

#[test]
fn connector_selection_shows_routing_and_caps() {
    let mut b = board();
    connector(&mut b);
    assert_eq!(
        bar(&b).controls,
        [
            // The line is drawn in a dark grey, which is no board colour's shade.
            colors(None),
            Control::Routing {
                current: Some(Routing::Elbow)
            },
            Control::StartCap {
                current: Some("none".into())
            },
            Control::EndCap {
                current: Some("triangleArrow".into())
            },
        ]
    );
}

#[test]
fn ink_selection_shows_colours() {
    let mut b = board();
    ink(&mut b);
    // The marker's ink is no board colour either.
    assert_eq!(bar(&b).controls, [colors(None)]);
}

#[test]
fn swatches_are_the_board_colours() {
    let mut b = board();
    sticky(&mut b);
    let bar = bar(&b);
    let Control::Colors { items, .. } = &bar.controls[0] else {
        panic!("{:?}", bar.controls);
    };
    assert_eq!(
        items,
        &[
            ("yellow", [0xFF, 0xE7, 0x8A, 0xFF]),
            ("orange", [0xFF, 0xC9, 0x94, 0xFF]),
            ("red", [0xFF, 0xAE, 0xA6, 0xFF]),
            ("pink", [0xF7, 0xB9, 0xDA, 0xFF]),
            ("violet", [0xD4, 0xC2, 0xFC, 0xFF]),
            ("blue", [0xAC, 0xD3, 0xFA, 0xFF]),
            ("green", [0xC2, 0xE8, 0xA6, 0xFF]),
            ("gray", [0xE3, 0xE1, 0xDD, 0xFF]),
        ]
    );
}

#[test]
fn mixed_selection_shows_shared_controls() {
    let mut b = board();
    let sticky = sticky(&mut b);
    let connector = connector(&mut b);
    select(&mut b, &[sticky, connector]);
    // The sticky is yellow and the connector's line is not a board colour: no swatch is on.
    assert_eq!(bar(&b).controls, [colors(None)]);
    // The order of the selection makes no difference.
    select(&mut b, &[connector, sticky]);
    assert_eq!(bar(&b).controls, [colors(None)]);

    // A sticky and a shape share the colour, and have the same one.
    let shape = shape(&mut b);
    select(&mut b, &[sticky, shape]);
    assert_eq!(bar(&b).controls, [colors(Some("yellow"))]);

    // A shape and a connector share the colour only.
    select(&mut b, &[shape, connector]);
    assert_eq!(kinds(&bar(&b)), ["colors"]);
}

#[test]
fn items_that_differ_show_no_current_value() {
    let mut b = board();
    let first = shape(&mut b);
    let second = make(
        &mut b,
        "board.shape",
        json!({"shape": "ellipse", "x": 600, "y": 0}),
    );
    select(&mut b, &[first, second]);
    // The same colour and size, a different outline.
    assert_eq!(
        bar(&b).controls,
        [
            colors(Some("yellow")),
            Control::Shape { current: None },
            Control::FontSize { current: Some("M") },
        ]
    );
    // Now the size differs too, and the colour.
    b.execute("board.setStyle", &json!({"ids": [second], "fontSize": 24}))
        .unwrap();
    b.execute("board.setColor", &json!({"ids": [second], "color": "blue"}))
        .unwrap();
    assert_eq!(
        bar(&b).controls,
        [
            colors(None),
            Control::Shape { current: None },
            Control::FontSize { current: None },
        ]
    );

    // Two connectors: the same caps, a different routing.
    let first = connector(&mut b);
    let second = make(
        &mut b,
        "board.connector",
        json!({"from": {"x": 0, "y": 800}, "to": {"x": 300, "y": 900}, "style": "straight"}),
    );
    select(&mut b, &[first, second]);
    assert_eq!(
        bar(&b).controls,
        [
            colors(None),
            Control::Routing { current: None },
            Control::StartCap {
                current: Some("none".into())
            },
            Control::EndCap {
                current: Some("triangleArrow".into())
            },
        ]
    );
}

#[test]
fn other_layers_are_left_out_of_what_is_shared() {
    let mut b = board();
    let sticky = sticky(&mut b);
    let connector = connector(&mut b);
    let others = [
        make(
            &mut b,
            "board.stamp",
            json!({"stamp": "heart", "x": 700, "y": 700}),
        ),
        make(
            &mut b,
            "board.table",
            json!({"rows": 2, "columns": 2, "x": 800, "y": 0}),
        ),
        make(
            &mut b,
            "node.create",
            json!({"type": "section", "x": -500, "y": 0, "width": 300, "height": 300}),
        ),
        make(
            &mut b,
            "shape.rectangle",
            json!({"x": -500, "y": 400, "width": 100, "height": 100}),
        ),
        // The label on the connector.
        NodeId(
            b.execute("board.label", &json!({"id": connector, "text": "yes"}))
                .unwrap()["label"]
                .as_u64()
                .unwrap(),
        ),
    ];
    for other in others {
        // On their own they take nothing from the bar.
        select(&mut b, &[other]);
        assert_eq!(context_bar(&b.session), None, "{other:?}");
        // Beside a sticky they don't take its controls away, or put theirs in.
        select(&mut b, &[sticky, other]);
        assert_eq!(bar(&b).controls, [colors(Some("yellow"))], "{other:?}");
        // And beside a connector the connector's own controls stay.
        select(&mut b, &[connector, other]);
        assert_eq!(
            kinds(&bar(&b)),
            ["colors", "routing", "start cap", "end cap"],
            "{other:?}"
        );
    }
}

#[test]
fn nothing_selected_has_no_context_bar() {
    let mut b = board();
    assert_eq!(context_bar(&b.session), None);
    let sticky = sticky(&mut b);
    assert!(context_bar(&b.session).is_some());
    select(&mut b, &[]);
    assert_eq!(context_bar(&b.session), None);
    select(&mut b, &[sticky]);
    assert!(context_bar(&b.session).is_some());
}

#[test]
fn design_doc_has_no_context_bar() {
    // A design file, with a sticky selected in it (pasted from a board, say).
    let mut b = board();
    sticky(&mut b);
    assert!(context_bar(&b.session).is_some());
    b.session.doc_mut().unwrap().kind = FileKind::Design;
    assert_eq!(b.kind(), DocKind::Design);
    assert_eq!(context_bar(&b.session), None);

    // A design file with nothing in it, and a session with no file.
    let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
    let design = BoardDoc::from_bytes(&design).unwrap();
    assert_eq!(context_bar(&design.session), None);
    assert_eq!(context_bar(&Session::empty()), None);
}

#[test]
fn a_shape_without_its_text_or_outline_offers_only_what_still_works() {
    let mut b = board();
    let shape = shape(&mut b);
    let layers = b.doc().unwrap().node(shape).unwrap().children.clone();
    let [outline, text] = layers[..] else {
        panic!("a shape holds an outline and a text: {layers:?}");
    };

    // Without its text there is no size to set, which the engine refuses.
    b.execute("edit.delete", &json!({"ids": [text]})).unwrap();
    select(&mut b, &[shape]);
    assert_eq!(kinds(&bar(&b)), ["colors", "shape"]);
    assert!(
        b.execute("board.setStyle", &json!({"fontSize": 12}))
            .is_err()
    );

    // Without its outline it has nothing to colour either, and that is refused too.
    b.execute("edit.delete", &json!({"ids": [outline]}))
        .unwrap();
    select(&mut b, &[shape]);
    assert_eq!(kinds(&bar(&b)), ["shape"]);
    assert!(
        b.execute("board.setColor", &json!({"color": "blue"}))
            .is_err()
    );
    // What is left, the outline, still works: the first outline the shape doesn't have.
    let choices = options(&bar(&b).controls[0]);
    let choice = choices.iter().find(|option| !option.selected).unwrap();
    b.execute(choice.command, &choice.params).unwrap();
}

#[test]
fn a_text_size_is_current_only_when_all_of_the_text_has_it() {
    let mut b = board();
    let shape = make(&mut b, "board.shape", json!({"x": 0, "y": 0, "text": "ab"}));
    let text = text_of(&b.doc().unwrap(), shape).unwrap();
    // The second letter in the size XL, the rest in M.
    let doc = b.session.doc_mut().unwrap();
    let Some(NodeKind::Text(data)) = doc.node_mut(text).map(|n| &mut n.kind) else {
        panic!("{text:?} is not text");
    };
    let mut style = data.style.clone();
    style.font_size = 36.0;
    data.runs.push(StyleRun {
        start: 1,
        end: 2,
        style,
    });
    assert_eq!(bar(&b).controls[2], Control::FontSize { current: None });

    // Picking a size sizes all of the text, and then that is the size.
    b.execute("board.setStyle", &json!({"fontSize": 12}))
        .unwrap();
    assert_eq!(
        bar(&b).controls[2],
        Control::FontSize { current: Some("S") }
    );
}

#[test]
fn current_colour_is_the_board_colour_each_item_was_set_to() {
    let mut b = board();
    let items = [
        sticky(&mut b),
        shape(&mut b),
        connector(&mut b),
        ink(&mut b),
    ];
    for item in items {
        select(&mut b, &[item]);
        for (name, _) in BOARD_COLORS {
            b.execute("board.setColor", &json!({"color": name}))
                .unwrap();
            assert_eq!(bar(&b).controls[0], colors(Some(name)), "{item:?}");
        }
        // A colour that is not one of the board's is no swatch's.
        b.execute("board.setColor", &json!({"color": "#123456"}))
            .unwrap();
        assert_eq!(bar(&b).controls[0], colors(None), "{item:?}");
    }
}

#[test]
fn cap_names_are_the_ones_the_engine_reads() {
    let mut b = board();
    let id = connector(&mut b);
    for cap in StrokeCap::ALL {
        b.execute("board.setStyle", &json!({"endCap": cap_name(cap)}))
            .unwrap();
        assert_eq!(b.doc().unwrap().node(id).unwrap().stroke.cap_end, cap);
    }
    // The bar offers some of them, and reads them back.
    for name in CAPS {
        assert!(StrokeCap::ALL.iter().any(|cap| cap_name(*cap) == *name));
        let params = json!({"startCap": name, "endCap": name});
        b.execute("board.setStyle", &params).unwrap();
        let controls = bar(&b).controls;
        let (start, end) = (&controls[2], &controls[3]);
        assert_eq!(
            start,
            &Control::StartCap {
                current: Some(name.to_string())
            }
        );
        assert_eq!(
            end,
            &Control::EndCap {
                current: Some(name.to_string())
            }
        );
    }
}
