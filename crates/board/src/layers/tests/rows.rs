//! The rows of the layer tree: their order and depth, what each says about its layer, and a
//! deep tree.

use std::time::{Duration, Instant};

use super::*;

#[test]
fn rows_list_top_first_with_depth() {
    let t = tree();
    // The fixture's own z order, bottom first.
    assert_eq!(t.children(t.f), [t.g, t.a, t.b]);

    let rows = t.all_rows();
    assert_eq!(
        outline(&rows),
        [(t.f, 0), (t.b, 1), (t.a, 1), (t.g, 1), (t.c, 2)]
    );
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["F", "B", "A", "G", "C"]);
    let icons: Vec<LayerIcon> = rows.iter().map(|row| row.icon).collect();
    assert_eq!(
        icons,
        [
            LayerIcon::Frame,
            LayerIcon::Rectangle,
            LayerIcon::Rectangle,
            LayerIcon::Group,
            LayerIcon::Rectangle
        ]
    );
    // F and G hold layers and show them; the rest are leaves. Nothing is hidden, locked or
    // selected.
    let holds: Vec<(bool, bool)> = rows
        .iter()
        .map(|row| (row.has_children, row.expanded))
        .collect();
    assert_eq!(
        holds,
        [
            (true, true),
            (false, false),
            (false, false),
            (true, true),
            (false, false)
        ]
    );
    assert!(rows.iter().all(|row| row.visible));
    assert!(rows.iter().all(|row| !row.locked));
    assert!(rows.iter().all(|row| !row.selected));
}

#[test]
fn collapsed_hides_children() {
    let t = tree();
    let rows = t.rows(&[], &[], "");
    assert_eq!(ids(&rows), [t.f]);
    assert!(rows[0].has_children);
    assert!(!rows[0].expanded);

    // F open, G still closed: C stays out, and G says it holds something.
    let rows = t.rows(&[t.f], &[], "");
    assert_eq!(ids(&rows), [t.f, t.b, t.a, t.g]);
    assert!(row(&rows, t.g).has_children);
    assert!(!row(&rows, t.g).expanded);
}

#[test]
fn expanding_a_leaf_or_an_unlisted_layer_changes_nothing() {
    let t = tree();
    // A has no layers; C is under a closed G.
    let rows = t.rows(&[t.f, t.a, t.c], &[], "");
    assert_eq!(ids(&rows), [t.f, t.b, t.a, t.g]);
    assert!(!row(&rows, t.a).expanded);
}

#[test]
fn hidden_and_locked_layers_say_so() {
    let mut t = tree();
    t.board
        .execute("object.toggleVisible", &json!({"ids": [t.a]}))
        .unwrap();
    t.board
        .execute("object.toggleLock", &json!({"ids": [t.g]}))
        .unwrap();
    let rows = t.all_rows();
    // Each row says what its own layer is, not what an ancestor makes of it.
    assert!(!row(&rows, t.a).visible && !row(&rows, t.a).locked);
    assert!(row(&rows, t.g).locked && row(&rows, t.g).visible);
    assert!(row(&rows, t.c).visible && !row(&rows, t.c).locked);
    assert!(row(&rows, t.b).visible && !row(&rows, t.b).locked);
}

#[test]
fn selected_rows_are_marked() {
    let t = tree();
    let rows = t.rows(&[t.f, t.g], &[t.a, t.c], "");
    let selected: Vec<NodeId> = rows
        .iter()
        .filter(|row| row.selected)
        .map(|row| row.id)
        .collect();
    assert_eq!(selected, [t.a, t.c]);
    // A selection under a closed branch marks nothing, since the row isn't listed.
    let rows = t.rows(&[], &[t.a], "");
    assert!(rows.iter().all(|row| !row.selected));
    // The filter marks what it lists.
    let rows = t.rows(&[], &[t.c, t.a], "c");
    assert_eq!(
        rows.iter()
            .filter(|row| row.selected)
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [t.c]
    );
}

#[test]
fn rows_of_something_that_is_not_there_are_empty() {
    let t = tree();
    assert!(!rows(&t.doc(), t.page, &HashSet::new(), &[], "").is_empty());
    assert!(rows(&t.doc(), NodeId(9_999), &HashSet::new(), &[], "").is_empty());
    assert!(rows(&t.doc(), NodeId(9_999), &HashSet::new(), &[], "a").is_empty());
}

#[test]
fn sticky_has_no_children_rows() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let sticky = make(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "needle"}),
    );
    let doc = board.doc().unwrap();
    let page = board.session.page().unwrap();
    // The note holds its text, but the panel shows one row, open or not.
    assert!(!doc.node(sticky).unwrap().children.is_empty());
    for expanded in [HashSet::new(), set(&[sticky])] {
        let rows = rows(&doc, page, &expanded, &[], "");
        assert_eq!(ids(&rows), [sticky]);
        assert_eq!(rows[0].icon, LayerIcon::Sticky);
        assert!(!rows[0].has_children);
        assert!(!rows[0].expanded);
    }
    // Nor does the filter go inside it: its text is not a layer to find, and the note
    // itself is not listed as the ancestor of one.
    let inside = doc.node(sticky).unwrap().children[0];
    let name = doc.node(inside).unwrap().name.clone();
    assert!(!name.is_empty());
    assert!(rows(&doc, page, &HashSet::new(), &[], &name).is_empty());
    let by_name = rows(&doc, page, &HashSet::new(), &[], "sticky");
    assert_eq!(ids(&by_name), [sticky]);
    assert!(!by_name[0].has_children);
}

#[test]
fn a_layer_without_a_name_is_listed_and_found_by_its_kind() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let blank = add(
        &mut doc,
        page,
        NodeKind::Ellipse {
            start: 0.0,
            sweep: std::f64::consts::TAU,
            inner: 0.0,
        },
        "",
    );
    let spaces = add(&mut doc, page, NodeKind::Frame, "  ");
    let rows_all = rows(&doc, page, &HashSet::new(), &[], "");
    assert_eq!(row(&rows_all, blank).name, "Ellipse");
    assert_eq!(row(&rows_all, spaces).name, "Frame");
    assert_eq!(
        ids(&rows(&doc, page, &HashSet::new(), &[], "ellip")),
        [blank]
    );
}

#[test]
fn deep_tree_rows_are_linear() {
    // 200 nested groups with a layer in the innermost: 201 rows, one per level.
    let mut doc = Document::new("Deep");
    let page = doc.first_page().unwrap();
    let mut parent = page;
    let mut groups = Vec::new();
    for n in 0..200 {
        parent = add(&mut doc, parent, NodeKind::Group, &format!("Group {n}"));
        groups.push(parent);
    }
    let leaf = add(&mut doc, parent, NodeKind::Rectangle, "Leaf");
    let expanded: HashSet<NodeId> = groups.iter().copied().collect();

    // Only the rows are timed, best of three so a busy machine doesn't fail it.
    let mut best = Duration::MAX;
    let mut listed = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        listed = rows(&doc, page, &expanded, &[leaf], "");
        best = best.min(start.elapsed());
    }
    assert!(best < Duration::from_millis(20), "{best:?}");

    assert_eq!(listed.len(), 201);
    let depths: Vec<usize> = listed.iter().map(|row| row.depth).collect();
    assert_eq!(depths, (0..=200).collect::<Vec<_>>());
    let order: Vec<NodeId> = groups.iter().copied().chain([leaf]).collect();
    assert_eq!(ids(&listed), order);
    assert!(row(&listed, leaf).selected);

    // The filter and the reveal walk the same chain.
    let found = rows(&doc, page, &HashSet::new(), &[], "leaf");
    assert_eq!(ids(&found), order);
    assert_eq!(reveal(&doc, &[leaf]), expanded);
}
