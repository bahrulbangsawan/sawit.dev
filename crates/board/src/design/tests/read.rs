//! Reading the selection: which layers count for a key, the defaults, and the keys that read
//! under the names `node.set` writes them with.

use super::*;

#[test]
fn same_values_read_same() {
    let f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(sel.get("width"), same(json!(100.0)));
    assert_eq!(sel.get("height"), same(json!(100.0)));
    assert_eq!(sel.get("y"), same(json!(0.0)));
    assert_eq!(sel.get("name"), Mixed::Mixed);
    assert_eq!(read(&f.board, &[f.r1]).get("name"), same(json!("R1")));
}

#[test]
fn different_values_read_mixed() {
    let f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(sel.get("x"), Mixed::Mixed);
    assert_eq!(read(&f.board, &[f.r1]).get("x"), same(json!(0.0)));
    assert_eq!(read(&f.board, &[f.r2]).get("x"), same(json!(50.0)));
}

#[test]
fn defaults_fill_omitted_keys() {
    let mut f = fixture();
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(sel.get("visible"), same(json!(true)));
    assert_eq!(sel.get("opacity"), same(json!(1.0)));
    assert_eq!(sel.get("locked"), same(json!(false)));
    assert_eq!(sel.get("rotation"), same(json!(0.0)));
    assert_eq!(sel.get("cornerRadius"), same(json!(0.0)));

    // A default never hides a value the layer really has.
    node_set(
        &mut f.board,
        &[f.r1],
        json!({"visible": false, "opacity": 0.5}),
    );
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert_eq!(sel.get("visible"), Mixed::Mixed);
    assert_eq!(sel.get("opacity"), Mixed::Mixed);
    assert_eq!(read(&f.board, &[f.r1]).get("visible"), same(json!(false)));
    assert_eq!(read(&f.board, &[f.r1]).get("opacity"), same(json!(0.5)));
}

#[test]
fn empty_lists_read_as_empty_lists_not_as_absent() {
    let mut f = fixture();
    // A rectangle starts with a fill, and with no stroke, effect or export setting.
    let sel = read(&f.board, &[f.r1]);
    for key in ["strokes", "effects", "exports", "dashes"] {
        assert_eq!(sel.get(key), same(json!([])), "{key}");
    }
    assert!(matches!(sel.get("fills"), Mixed::Same(Value::Array(a)) if a.len() == 1));

    // Without its fill R2 reads an empty list too, and differs from R1.
    node_set(&mut f.board, &[f.r2], json!({"fills": []}));
    assert_eq!(read(&f.board, &[f.r2]).get("fills"), same(json!([])));
    assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fills"), Mixed::Mixed);
}

#[test]
fn the_blend_is_the_layers_own() {
    let mut f = fixture();
    let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
    // A rectangle is Normal and a frame passes through, and the engine's inspector leaves
    // both out.
    assert_eq!(read(&f.board, &[f.r1]).get("blend"), same(json!("normal")));
    assert_eq!(
        read(&f.board, &[frame]).get("blend"),
        same(json!("passThrough"))
    );
    assert_eq!(read(&f.board, &[f.r1, frame]).get("blend"), Mixed::Mixed);

    // A frame set to Normal is not the same as one that passes through.
    let edit = read(&f.board, &[frame]).set("blend", json!("normal"));
    apply(&mut f.board, &edit);
    assert_eq!(read(&f.board, &[frame]).get("blend"), same(json!("normal")));
    let edit = read(&f.board, &[f.r1, frame]).set("blend", json!("multiply"));
    apply(&mut f.board, &edit);
    assert_eq!(
        read(&f.board, &[f.r1, frame]).get("blend"),
        same(json!("multiply"))
    );
}

#[test]
fn numbers_read_as_floats() {
    let mut f = fixture();
    let frame = make(&mut f.board, json!({"type": "frame", "name": "F"}));
    make(
        &mut f.board,
        json!({"type": "rectangle", "parent": frame, "width": 10, "height": 10}),
    );
    // Even a count the engine reports as an integer: the values compare by variant.
    assert_eq!(read(&f.board, &[frame]).get("childCount"), same(json!(1.0)));
}

#[test]
fn every_default_is_valid_json() {
    for (key, json) in DEFAULTS {
        assert!(serde_json::from_str::<Value>(json).is_ok(), "{key}: {json}");
    }
}

#[test]
fn lists_and_objects_compare_whole() {
    let mut f = fixture();
    // Both rectangles start with the same fill.
    let sel = read(&f.board, &[f.r1, f.r2]);
    assert!(matches!(sel.get("fills"), Mixed::Same(Value::Array(a)) if a.len() == 1));
    // A fill of another colour, or of another kind, is another value.
    node_set(&mut f.board, &[f.r2], json!({"fill": "#FF0000"}));
    assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fills"), Mixed::Mixed);
    node_set(&mut f.board, &[f.r1], json!({"fill": "#FF0000"}));
    let red = read(&f.board, &[f.r1, f.r2]).get("fills");
    assert_eq!(red, same(json!([{"type": "solid", "color": "#FF0000"}])));
}

/// One key space: a key is read, written and read again under the same name, whatever shape
/// `node_json` gives it (nested layout, sizing and corner values, or not given at all).
#[test]
fn flattened_keys_read_back_what_node_set_wrote() {
    let mut f = fixture();
    let frame = make(
        &mut f.board,
        json!({"type": "frame", "name": "F", "width": 200, "height": 100}),
    );
    let child = make(
        &mut f.board,
        json!({"type": "rectangle", "parent": frame, "width": 10, "height": 10}),
    );

    // Without auto layout the frame has none of its keys; every layer has a corner and a lock.
    let sel = read(&f.board, &[frame]);
    for key in [
        "layout",
        "gap",
        "crossGap",
        "padding",
        "paddingTop",
        "paddingRight",
        "paddingBottom",
        "paddingLeft",
        "mainAlign",
        "crossAlign",
        "wrap",
        "sizingH",
        "sizingV",
    ] {
        assert_eq!(sel.get(key), Mixed::Absent, "{key}");
    }
    assert_eq!(sel.get("constrainProportions"), same(json!(false)));
    for key in [
        "topLeftRadius",
        "topRightRadius",
        "bottomRightRadius",
        "bottomLeftRadius",
    ] {
        assert_eq!(sel.get(key), same(json!(0.0)), "{key}");
    }

    let writes = [
        ("layout", json!("vertical"), json!("vertical")),
        ("gap", json!(12.0), json!(12.0)),
        ("crossGap", json!(7.0), json!(7.0)),
        ("paddingTop", json!(1.0), json!(1.0)),
        ("paddingRight", json!(2.0), json!(2.0)),
        ("paddingBottom", json!(3.0), json!(3.0)),
        ("paddingLeft", json!(4.0), json!(4.0)),
        ("mainAlign", json!("center"), json!("center")),
        ("crossAlign", json!("max"), json!("max")),
        ("wrap", json!(true), json!(true)),
        ("sizingH", json!("hug"), json!("hug")),
        ("topLeftRadius", json!(5.0), json!(5.0)),
        ("topRightRadius", json!(6.0), json!(6.0)),
        ("bottomRightRadius", json!(7.0), json!(7.0)),
        ("bottomLeftRadius", json!(8.0), json!(8.0)),
        ("constrainProportions", json!(true), json!(true)),
    ];
    for (key, write, back) in writes {
        let edit = read(&f.board, &[frame]).set(key, write);
        apply(&mut f.board, &edit);
        assert_eq!(read(&f.board, &[frame]).get(key), same(back), "{key}");
    }
    let sel = read(&f.board, &[frame]);
    assert_eq!(sel.get("sizingV"), same(json!("fixed")));
    // The sides that differ are one list, the way `node.set` takes them.
    assert_eq!(sel.get("padding"), same(json!([1.0, 2.0, 3.0, 4.0])));
    assert_eq!(sel.get("cornerRadius"), same(json!([5.0, 6.0, 7.0, 8.0])));

    // The sides that agree are one number.
    let edit = sel.set("padding", json!(8.0));
    apply(&mut f.board, &edit);
    let edit = sel.set("cornerRadius", json!(6.0));
    apply(&mut f.board, &edit);
    let sel = read(&f.board, &[frame]);
    assert_eq!(sel.get("padding"), same(json!(8.0)));
    assert_eq!(sel.get("cornerRadius"), same(json!(6.0)));
    for key in ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] {
        assert_eq!(sel.get(key), same(json!(8.0)), "{key}");
    }
    for key in [
        "topLeftRadius",
        "topRightRadius",
        "bottomRightRadius",
        "bottomLeftRadius",
    ] {
        assert_eq!(sel.get(key), same(json!(6.0)), "{key}");
    }

    // A child of an auto layout frame has a sizing of its own.
    assert_eq!(
        read(&f.board, &[child]).get("sizingV"),
        same(json!("fixed"))
    );
    let edit = read(&f.board, &[child]).set("sizingV", json!("fill"));
    apply(&mut f.board, &edit);
    assert_eq!(read(&f.board, &[child]).get("sizingV"), same(json!("fill")));
    // ... and a rectangle outside one has none.
    assert_eq!(read(&f.board, &[f.r1]).get("sizingH"), Mixed::Absent);

    // Only a text has a vertical alignment.
    assert_eq!(
        read(&f.board, &[f.r1]).get("textAlignVertical"),
        Mixed::Absent
    );
    assert_eq!(
        read(&f.board, &[f.text]).get("textAlignVertical"),
        same(json!("top"))
    );
    let edit = read(&f.board, &[f.text]).set("textAlignVertical", json!("middle"));
    apply(&mut f.board, &edit);
    assert_eq!(
        read(&f.board, &[f.text]).get("textAlignVertical"),
        same(json!("middle"))
    );
}

#[test]
fn layers_without_a_key_do_not_count_for_it() {
    let f = fixture();
    let size = read(&f.board, &[f.text]).get("fontSize");
    assert!(matches!(size, Mixed::Same(_)), "{size:?}");
    // A text and a rectangle: the rectangle has no font size to disagree about.
    assert_eq!(read(&f.board, &[f.r1, f.text]).get("fontSize"), size);
    // Rectangles alone have none at all.
    assert_eq!(read(&f.board, &[f.r1, f.r2]).get("fontSize"), Mixed::Absent);
    assert_eq!(read(&f.board, &[f.r1]).get("characters"), Mixed::Absent);
}
