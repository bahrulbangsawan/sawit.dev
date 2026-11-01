//! The auto layout keys, which count every selected frame, and a frame without auto layout as a
//! value of its own.

use super::*;

/// A 200 × 100 frame called `name` at (`x`, 400), given a vertical auto layout with a gap of 12
/// when `auto_layout` is set.
fn frame_at(board: &mut BoardDoc, name: &str, x: f64, auto_layout: bool) -> NodeId {
    let params =
        json!({"type": "frame", "name": name, "x": x, "y": 400, "width": 200, "height": 100});
    let id = make(board, params);
    if auto_layout {
        node_set(board, &[id], json!({"layout": "vertical", "gap": 12.0}));
    }
    id
}

/// The auto layout keys, listed by hand: what the panel has to treat as a frame's own.
const AUTO_LAYOUT_KEYS: [&str; 11] = [
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
];

/// A frame without auto layout lacks every auto layout key, yet `node.set` of one gives it an
/// auto layout. So next to a frame that has them it is a value of its own: the other frame's
/// value is not the selection's, and a field that showed it as the selection's would, once
/// committed, change this frame.
#[test]
fn a_frame_without_auto_layout_makes_layout_keys_mixed() {
    let mut f = fixture();
    let with = frame_at(&mut f.board, "A", 0.0, true);
    let without = frame_at(&mut f.board, "B", 300.0, false);
    let also_without = frame_at(&mut f.board, "C", 600.0, false);

    // The frame with auto layout holds every key.
    let a = read(&f.board, &[with]);
    assert_eq!(a.get("layout"), same(json!("vertical")));
    assert_eq!(a.get("gap"), same(json!(12.0)));
    for key in AUTO_LAYOUT_KEYS {
        assert!(matches!(a.get(key), Mixed::Same(_)), "A holds {key}");
    }

    // The one without holds none of them: there is nothing to show for it alone.
    let b = read(&f.board, &[without]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(b.get(key), Mixed::Absent, "B alone: {key}");
    }
    let both_without = read(&f.board, &[without, also_without]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(both_without.get(key), Mixed::Absent, "B and C: {key}");
    }

    // Together they disagree, whichever comes first.
    for ids in [[with, without], [without, with]] {
        let both = read(&f.board, &ids);
        assert_eq!(both.get("layout"), Mixed::Mixed, "{ids:?}");
        assert_eq!(both.get("gap"), Mixed::Mixed, "{ids:?}");
        for key in AUTO_LAYOUT_KEYS {
            assert_eq!(both.get(key), Mixed::Mixed, "{ids:?}: {key}");
        }
    }
    let three = read(&f.board, &[without, with, also_without]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(three.get(key), Mixed::Mixed, "A, B and C: {key}");
    }

    // A rectangle is no frame, so it doesn't count: A with a rectangle reads as A does.
    let with_rectangle = read(&f.board, &[with, f.r1]);
    assert_eq!(with_rectangle.get("gap"), same(json!(12.0)));
}

/// `node.set` skips the layers that are not frames when it writes an auto layout key, so they
/// don't count for one.
#[test]
fn layers_that_are_not_frames_do_not_count_for_layout_keys() {
    let mut f = fixture();
    let with = frame_at(&mut f.board, "A", 0.0, true);
    let without = frame_at(&mut f.board, "B", 300.0, false);

    // A rectangle beside a frame with auto layout leaves the answer to the frame.
    let with_rectangle = read(&f.board, &[with, f.r1]);
    assert_eq!(with_rectangle.get("gap"), same(json!(12.0)));
    assert_eq!(with_rectangle.get("layout"), same(json!("vertical")));
    for key in AUTO_LAYOUT_KEYS {
        let alone = read(&f.board, &[with]).get(key);
        assert_eq!(with_rectangle.get(key), alone, "A and a rectangle: {key}");
    }

    // Beside a frame without auto layout they add nothing: still nothing to show.
    let without_others = read(&f.board, &[without, f.r1, f.text]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(
            without_others.get(key),
            Mixed::Absent,
            "B and others: {key}"
        );
    }
    // With no frame at all there is nothing to count.
    let no_frame = read(&f.board, &[f.r1, f.text]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(no_frame.get(key), Mixed::Absent, "no frame: {key}");
    }

    // The rectangle doesn't hide that the frames differ.
    let all = read(&f.board, &[f.r1, with, without]);
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(all.get(key), Mixed::Mixed, "{key}");
    }
}

/// The frames all have auto layout: the keys read as they do for any other property.
#[test]
fn frames_that_all_have_auto_layout_read_as_they_hold() {
    let mut f = fixture();
    let a = frame_at(&mut f.board, "A", 0.0, true);
    let b = frame_at(&mut f.board, "B", 300.0, true);

    let both = read(&f.board, &[a, b]);
    assert_eq!(both.get("layout"), same(json!("vertical")));
    assert_eq!(both.get("gap"), same(json!(12.0)));
    for key in AUTO_LAYOUT_KEYS {
        assert_eq!(both.get(key), read(&f.board, &[a]).get(key), "{key}");
    }

    node_set(&mut f.board, &[b], json!({"gap": 4.0}));
    let both = read(&f.board, &[a, b]);
    assert_eq!(both.get("gap"), Mixed::Mixed);
    assert_eq!(both.get("layout"), same(json!("vertical")));
    assert_eq!(both.get("wrap"), same(json!(false)));
}

/// What `set` says about the auto layout keys, as the engine does it: the frame without auto
/// layout gets one, horizontal because that is the engine's default, and a rectangle in the
/// selection is skipped, not refused.
#[test]
fn writing_a_layout_key_gives_a_frame_without_auto_layout_one() {
    let mut f = fixture();
    let with = frame_at(&mut f.board, "A", 0.0, true);
    let without = frame_at(&mut f.board, "B", 300.0, false);
    assert_eq!(read(&f.board, &[without]).get("layout"), Mixed::Absent);
    let rectangle = read(&f.board, &[f.r1]);

    let edit = read(&f.board, &[with, without, f.r1]).set("gap", json!(12.0));
    assert_eq!(edit.params["ids"], json!([with, without, f.r1]));
    apply(&mut f.board, &edit);

    let after = read(&f.board, &[without]);
    assert_eq!(after.get("layout"), same(json!("horizontal")));
    assert_eq!(after.get("gap"), same(json!(12.0)));
    assert_eq!(
        read(&f.board, &[with]).get("layout"),
        same(json!("vertical")),
        "the frame that had one keeps its direction"
    );
    assert_eq!(read(&f.board, &[f.r1]), rectangle);
}

/// `fields::LAYOUT_KEYS` is what `get` counts every frame for, so it must be the keys a layer
/// gains when it gets an auto layout, and no more or fewer: those the layout lifts, and the
/// sizing that comes with it.
#[test]
fn layout_keys_are_the_keys_an_auto_layout_adds_to_a_frame() {
    let mut f = fixture();
    let id = frame_at(&mut f.board, "F", 0.0, false);
    let keys = |board: &BoardDoc| -> Vec<String> {
        let selection = read(board, &[id]);
        selection.layers[0].props.keys().cloned().collect()
    };
    let before = keys(&f.board);
    node_set(&mut f.board, &[id], json!({"layout": "horizontal"}));
    let after = keys(&f.board);

    let mut added: Vec<&str> = after
        .iter()
        .filter(|key| !before.contains(key))
        .map(String::as_str)
        .collect();
    added.sort_unstable();
    let mut expected: Vec<&str> = fields::LAYOUT_KEYS.to_vec();
    expected.extend(["sizingH", "sizingV"]);
    expected.sort_unstable();
    assert_eq!(added, expected);
    // The list the tests above go by is the same one.
    let mut by_hand = AUTO_LAYOUT_KEYS.to_vec();
    by_hand.sort_unstable();
    let mut named = fields::LAYOUT_KEYS.to_vec();
    named.sort_unstable();
    assert_eq!(named, by_hand);
}
