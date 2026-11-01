//! Where a dragged layer lands, and the drops that are refused.

use super::*;

#[test]
fn drop_between_siblings_gives_bottom_first_index() {
    let mut t = tree();
    let rows = t.all_rows();
    // Dragging A above B: B is the top layer of F, so A goes on top of it.
    let reparent = t.target(&rows, &[t.a], t.b, DropPos::Above).unwrap();
    assert_eq!(reparent.parent, t.f);
    assert_eq!(reparent.ids, [t.a]);
    assert_eq!(reparent.index, 2);
    t.apply(&reparent);
    assert_eq!(t.children(t.f), [t.g, t.b, t.a]);
}

#[test]
fn drop_below_a_row_goes_under_it() {
    let mut t = tree();
    let rows = t.all_rows();
    // B below G: G is the bottom layer, so B goes under it.
    let reparent = t.target(&rows, &[t.b], t.g, DropPos::Below).unwrap();
    assert_eq!((reparent.parent, reparent.index), (t.f, 0));
    t.apply(&reparent);
    assert_eq!(t.children(t.f), [t.b, t.g, t.a]);
}

#[test]
fn drop_between_siblings_counts_layers_that_leave_the_gap() {
    let mut t = tree();
    let rows = t.all_rows();
    // G and A, which sit under B, leave the gap above B: of the three layers below it only
    // B stays, so the index is 1, and they land on top of B, in their order.
    let reparent = t.target(&rows, &[t.g, t.a], t.b, DropPos::Above).unwrap();
    assert_eq!((reparent.parent, reparent.index), (t.f, 1));
    t.apply(&reparent);
    assert_eq!(t.children(t.f), [t.b, t.g, t.a]);

    // B and A, above G, drop under it: nothing of theirs is below the gap, so the index is 0,
    // and they keep their order, A under B.
    let mut t = tree();
    let rows = t.all_rows();
    let reparent = t.target(&rows, &[t.b, t.a], t.g, DropPos::Below).unwrap();
    assert_eq!((reparent.parent, reparent.index), (t.f, 0));
    t.apply(&reparent);
    assert_eq!(t.children(t.f), [t.a, t.b, t.g]);
}

#[test]
fn drop_above_a_top_level_row_goes_to_the_page() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let first = add(&mut doc, page, NodeKind::Frame, "First");
    let second = add(&mut doc, page, NodeKind::Frame, "Second");
    let inner = add(&mut doc, first, NodeKind::Rectangle, "Inner");
    let mut board = file(doc);
    let doc = board.doc().unwrap();
    let rows = rows(&doc, page, &set(&[first]), &[], "");
    assert_eq!(ids(&rows), [second, first, inner]);

    // Inner above Second: out of First, onto the page above Second.
    let reparent = drop_on(&doc, &rows, &[inner], second, DropPos::Above).unwrap();
    assert_eq!((reparent.parent, reparent.index), (page, 2));
    apply(&mut board, &reparent);
    assert_eq!(
        board.doc().unwrap().node(page).unwrap().children,
        [first, second, inner]
    );
}

#[test]
fn drop_into_frame() {
    let mut t = tree();
    let rows = t.all_rows();
    let reparent = t.target(&rows, &[t.c], t.f, DropPos::Into).unwrap();
    assert_eq!(reparent.parent, t.f);
    assert_eq!(reparent.ids, [t.c]);
    // On top of what F holds.
    assert_eq!(reparent.index, 3);
    t.apply(&reparent);
    // C is on top of A and B. G held only C, and the engine removes a group that is left empty.
    assert_eq!(t.children(t.f), [t.a, t.b, t.c]);
    assert_eq!(t.doc().node(t.c).unwrap().parent, Some(t.f));
    assert!(t.doc().node(t.g).is_none());
}

#[test]
fn drop_into_a_parent_of_the_dragged_layer_puts_it_on_top() {
    let mut t = tree();
    let rows = t.all_rows();
    let reparent = t.target(&rows, &[t.g], t.f, DropPos::Into).unwrap();
    assert_eq!((reparent.parent, reparent.index), (t.f, 2));
    t.apply(&reparent);
    assert_eq!(t.children(t.f), [t.a, t.b, t.g]);
}

#[test]
fn drop_into_a_group_a_section_and_a_component() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let group = add(&mut doc, page, NodeKind::Group, "Group");
    let kept = add(&mut doc, group, NodeKind::Rectangle, "Kept");
    let outer = add(&mut doc, page, section(), "Section");
    let main = add(&mut doc, page, component(), "Main");
    let loose = add(&mut doc, page, NodeKind::Rectangle, "Loose");
    let board = file(doc);
    let doc = board.doc().unwrap();
    let rows = rows(&doc, page, &HashSet::new(), &[], "");
    for into in [group, outer, main] {
        let reparent = drop_on(&doc, &rows, &[loose], into, DropPos::Into).unwrap();
        assert_eq!(reparent.parent, into);
        let held = doc.node(into).unwrap().children.len();
        assert_eq!(reparent.index, held);
    }
    assert_eq!(doc.node(group).unwrap().children, [kept]);
}

#[test]
fn drop_into_own_subtree_is_refused() {
    let t = tree();
    let rows = t.all_rows();
    // F into its own group, and onto, above or below any row inside it.
    assert_eq!(t.target(&rows, &[t.f], t.g, DropPos::Into), None);
    for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
        assert_eq!(t.target(&rows, &[t.f], t.c, pos), None, "{pos:?}");
        assert_eq!(t.target(&rows, &[t.f], t.a, pos), None, "{pos:?}");
    }
    // G into C's layer, and next to C inside G.
    assert_eq!(t.target(&rows, &[t.g], t.c, DropPos::Above), None);
    assert_eq!(t.target(&rows, &[t.g], t.c, DropPos::Into), None);
    // Any of several dragged layers decides.
    assert_eq!(t.target(&rows, &[t.a, t.g], t.c, DropPos::Below), None);
    // A layer onto itself.
    for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
        assert_eq!(t.target(&rows, &[t.a], t.a, pos), None, "{pos:?}");
        assert_eq!(t.target(&rows, &[t.a, t.b], t.b, pos), None, "{pos:?}");
    }
    // Next to the dragged layer's parent is fine.
    assert!(t.target(&rows, &[t.g], t.f, DropPos::Above).is_some());
}

#[test]
fn nothing_dragged_or_nowhere_to_drop_is_no_drop() {
    let t = tree();
    let rows = t.all_rows();
    let doc = t.doc();
    assert_eq!(t.target(&rows, &[], t.f, DropPos::Into), None);
    assert_eq!(drop_target(&doc, &rows, &[t.a], 99, DropPos::Into), None);
    assert_eq!(drop_target(&doc, &[], &[t.a], 0, DropPos::Above), None);
    // A layer that is gone is left out, and a drag of nothing else has no drop.
    assert_eq!(t.target(&rows, &[NodeId(9_999)], t.f, DropPos::Into), None);
    let reparent = t
        .target(&rows, &[NodeId(9_999), t.c], t.f, DropPos::Into)
        .unwrap();
    assert_eq!(reparent.ids, [t.c]);
    // A page doesn't move into a layer, and a layer listed twice moves once.
    assert_eq!(t.target(&rows, &[t.page], t.f, DropPos::Into), None);
    let reparent = t.target(&rows, &[t.c, t.c], t.f, DropPos::Into).unwrap();
    assert_eq!(reparent.ids, [t.c]);
}

#[test]
fn drop_that_changes_nothing_is_no_drop() {
    // The single-layer drops on the fixture that leave the layer where it is: in the gap it
    // fills already, or on top of a parent it is on top of.
    let in_place = |t: &Tree| {
        [
            (t.b, DropPos::Above, t.a),
            (t.a, DropPos::Below, t.b),
            (t.a, DropPos::Above, t.g),
            (t.g, DropPos::Below, t.a),
            (t.b, DropPos::Into, t.f),
            (t.c, DropPos::Into, t.g),
        ]
    };
    let t = tree();
    // F's layers run [G, A, B] from the bottom, and G holds C.
    assert_eq!(t.children(t.f), [t.g, t.a, t.b]);
    assert_eq!(t.children(t.g), [t.c]);
    let rows = t.all_rows();
    for (dragged, pos, over) in in_place(&t) {
        let drop = t.target(&rows, &[dragged], over, pos);
        assert_eq!(drop, None, "{dragged:?} {pos:?} {over:?}");
    }

    // Every other drop of one layer on a row is refused (on the layer or inside it, or into a
    // layer that holds none), or changes the layers of its parent when the engine runs it. Each
    // one runs on a tree of its own.
    for who in 0..4 {
        for at in 0..rows.len() {
            for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
                let mut t = tree();
                let dragged = [t.a, t.b, t.g, t.c][who];
                let rows = t.all_rows();
                let over = rows[at].id;
                let what = format!("{dragged:?} {pos:?} {over:?}");
                match drop_target(&t.doc(), &rows, &[dragged], at, pos) {
                    Some(reparent) => {
                        let before = t.children(reparent.parent);
                        t.apply(&reparent);
                        assert_ne!(t.children(reparent.parent), before, "{what}");
                    }
                    None => {
                        let doc = t.doc();
                        let on_dragged = std::iter::once(over)
                            .chain(doc.ancestors(over))
                            .any(|id| id == dragged);
                        let into_a_leaf =
                            pos == DropPos::Into && !doc.node(over).unwrap().kind.is_container();
                        let stays = in_place(&t).contains(&(dragged, pos, over));
                        assert!(on_dragged || into_a_leaf || stays, "{what}");
                    }
                }
            }
        }
    }
}

#[test]
fn several_layers_that_stay_where_they_are_are_no_drop() {
    let t = tree();
    let rows = t.all_rows();
    // F's layers run [G, A, B]. Layers that stay together in the gap they fill, in whatever
    // order they are dragged...
    assert_eq!(t.target(&rows, &[t.g, t.a], t.b, DropPos::Below), None);
    assert_eq!(t.target(&rows, &[t.a, t.g], t.b, DropPos::Below), None);
    assert_eq!(t.target(&rows, &[t.a, t.b], t.g, DropPos::Above), None);
    assert_eq!(t.target(&rows, &[t.g, t.a, t.b], t.f, DropPos::Into), None);
    // ...also with a layer one of them holds: it goes along, and no more moves on its own than
    // the layer that holds it.
    assert_eq!(t.target(&rows, &[t.g, t.c], t.a, DropPos::Below), None);
    assert_eq!(
        t.target(&rows, &[t.c, t.g, t.a, t.b], t.f, DropPos::Into),
        None
    );

    // Layers that don't all stay are a move: under G, G and B round A, and G on top...
    assert!(t.target(&rows, &[t.a, t.b], t.g, DropPos::Below).is_some());
    assert!(t.target(&rows, &[t.g, t.b], t.a, DropPos::Above).is_some());
    assert!(t.target(&rows, &[t.g, t.c], t.f, DropPos::Into).is_some());
    // ...and so is a layer from another parent among them, even when the others stay: C leaves
    // G, and B is on top of F already.
    assert!(t.target(&rows, &[t.a, t.c], t.f, DropPos::Into).is_some());
    assert!(t.target(&rows, &[t.b, t.c], t.f, DropPos::Into).is_some());
}

#[test]
fn an_offered_drop_of_any_layers_changes_the_parent_they_land_in() {
    let mut offered = 0;
    // Every set of the layers below F, dragged onto every row at every position. Each drop
    // runs on a tree of its own.
    for set in 1..16_u32 {
        for at in 0..tree().all_rows().len() {
            for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
                let mut t = tree();
                let rows = t.all_rows();
                let layers = [t.a, t.b, t.g, t.c];
                let dragged: Vec<NodeId> = (0..4)
                    .filter(|n| set & (1 << n) != 0)
                    .map(|n| layers[n])
                    .collect();
                let Some(reparent) = drop_target(&t.doc(), &rows, &dragged, at, pos) else {
                    continue;
                };
                let before = t.children(reparent.parent);
                t.apply(&reparent);
                let after = t.children(reparent.parent);
                assert_ne!(after, before, "{dragged:?} {pos:?} row {at}");
                offered += 1;
            }
        }
    }
    assert!(offered > 0);
}

#[test]
fn drop_into_sticky_is_refused() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let first = make(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let second = make(&mut board, "board.sticky", json!({"x": 300, "y": 0}));
    let doc = board.doc().unwrap();
    let page = board.session.page().unwrap();
    let rows = rows(&doc, page, &HashSet::new(), &[], "");
    assert_eq!(ids(&rows), [second, first]);

    // Into a sticky the layer would vanish from the panel; next to it is fine.
    assert_eq!(drop_on(&doc, &rows, &[second], first, DropPos::Into), None);
    let reparent = drop_on(&doc, &rows, &[second], first, DropPos::Below).unwrap();
    assert_eq!((reparent.parent, reparent.index), (page, 0));
}

#[test]
fn board_items_that_hold_layers_take_none_into_them() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let shape = make(
        &mut board,
        "board.shape",
        json!({"shape": "ellipse", "x": 0, "y": 0}),
    );
    let table = make(
        &mut board,
        "board.table",
        json!({"rows": 2, "columns": 2, "x": 0, "y": 400}),
    );
    let stamp = make(
        &mut board,
        "board.stamp",
        json!({"x": 500, "y": 0, "stamp": "heart"}),
    );
    let loose = make(&mut board, "board.sticky", json!({"x": 700, "y": 0}));
    let doc = board.doc().unwrap();
    let page = board.session.page().unwrap();
    let rows = rows(&doc, page, &HashSet::new(), &[], "");
    for atom in [shape, table, stamp] {
        assert_eq!(drop_on(&doc, &rows, &[loose], atom, DropPos::Into), None);
        // Next to one is fine: under it, as `loose` is right above the stamp already.
        assert!(drop_on(&doc, &rows, &[loose], atom, DropPos::Below).is_some());
    }
}

#[test]
fn section_into_frame_is_refused() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let outer = add(&mut doc, page, section(), "Outer");
    let frame = add(&mut doc, page, NodeKind::Frame, "Frame");
    let inner = add(&mut doc, frame, NodeKind::Rectangle, "Inner");
    let group = add(&mut doc, page, NodeKind::Group, "Group");
    let moved = add(&mut doc, page, section(), "Moved");
    let board = file(doc);
    let doc = board.doc().unwrap();
    let rows = rows(&doc, page, &set(&[frame]), &[], "");
    assert_eq!(ids(&rows), [moved, group, frame, inner, outer]);

    // Sections go on the page or in sections, never in a frame or a group...
    assert_eq!(drop_on(&doc, &rows, &[moved], frame, DropPos::Into), None);
    assert_eq!(drop_on(&doc, &rows, &[moved], group, DropPos::Into), None);
    // ...also when the drop is next to one of the layers in the frame.
    assert_eq!(drop_on(&doc, &rows, &[moved], inner, DropPos::Above), None);
    assert_eq!(drop_on(&doc, &rows, &[moved], inner, DropPos::Below), None);
    // A section among other layers drags as one of them.
    assert_eq!(
        drop_on(&doc, &rows, &[inner, moved], frame, DropPos::Into),
        None
    );
    // On the page, and in another section, it is fine.
    let onto_page = drop_on(&doc, &rows, &[moved], frame, DropPos::Above).unwrap();
    assert_eq!(onto_page.parent, page);
    let into_section = drop_on(&doc, &rows, &[moved], outer, DropPos::Into).unwrap();
    assert_eq!(into_section.parent, outer);
    // Other layers go into frames and sections as before.
    let group_in_frame = drop_on(&doc, &rows, &[group], frame, DropPos::Into).unwrap();
    assert_eq!(group_in_frame.parent, frame);
    let rectangle = drop_on(&doc, &rows, &[inner], outer, DropPos::Into).unwrap();
    assert_eq!(rectangle.parent, outer);
}

#[test]
fn instances_and_what_is_inside_them_take_no_drops() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let main = add(&mut doc, page, component(), "Main");
    let instance = add(
        &mut doc,
        page,
        NodeKind::Instance {
            component: main,
            props: BTreeMap::new(),
        },
        "Instance",
    );
    let inside = add(&mut doc, instance, NodeKind::Frame, "Inside");
    let loose = add(&mut doc, page, NodeKind::Rectangle, "Loose");
    let board = file(doc);
    let doc = board.doc().unwrap();
    let rows = rows(&doc, page, &set(&[instance]), &[], "");
    assert_eq!(ids(&rows), [loose, instance, inside, main]);

    // The engine refuses to add, remove or reorder layers in an instance.
    for pos in [DropPos::Into, DropPos::Above, DropPos::Below] {
        assert_eq!(drop_on(&doc, &rows, &[loose], inside, pos), None, "{pos:?}");
    }
    assert_eq!(
        drop_on(&doc, &rows, &[loose], instance, DropPos::Into),
        None
    );
    // Next to the instance is the page (under it, as `loose` is right above it already), and
    // its main component takes layers.
    let beside = drop_on(&doc, &rows, &[loose], instance, DropPos::Below).unwrap();
    assert_eq!(beside.parent, page);
    let into_main = drop_on(&doc, &rows, &[loose], main, DropPos::Into).unwrap();
    assert_eq!(into_main.parent, main);
}
