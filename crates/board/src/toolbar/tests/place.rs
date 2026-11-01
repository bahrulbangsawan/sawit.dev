//! Placing templates and tables in the middle of the view.

use figcraft_doc::BoardItem;
use figcraft_engine::board_templates::TEMPLATES;
use figcraft_geom::Rect;

use super::*;

fn page_is_empty(b: &BoardDoc) -> bool {
    let page = b.session.page().unwrap();
    b.doc().unwrap().node(page).unwrap().children.is_empty()
}

/// The part of the page that the view shows, in page coordinates.
fn viewport(b: &BoardDoc) -> Rect {
    let view = &b.session.ds().unwrap().view;
    let (x, y) = view.origin();
    Rect::new(x, y, x + view.vw / view.zoom, y + view.vh / view.zoom)
}

/// The box around everything on the page, in page coordinates.
fn content(b: &BoardDoc) -> Rect {
    let page = b.session.page().unwrap();
    b.doc().unwrap().page_content_bounds(page).unwrap()
}

/// Whether `inner` lies inside `outer`.
fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

#[test]
fn template_is_one_undo_step() {
    let mut b = board();
    // Zoomed far out, on a part of the page far from its origin: the template goes where the
    // view is, in view, not to the origin.
    let view = json!({"zoom": 0.05, "cx": 20000, "cy": 20000, "vw": 800, "vh": 600});
    b.execute("view.set", &view).unwrap();
    let first = templates(&mut b).unwrap().into_iter().next().unwrap();
    assert!(page_is_empty(&b));

    place_template(&mut b, &first.id).unwrap();
    assert!(!page_is_empty(&b));
    let (bounds, shown) = (content(&b), viewport(&b));
    assert!(inside(bounds, shown), "{bounds:?} is not inside {shown:?}");

    b.execute("edit.undo", &json!({})).unwrap();
    assert!(page_is_empty(&b));
}

#[test]
fn every_template_is_listed_and_places() {
    let listed = templates(&mut board()).unwrap();
    let ids: Vec<&str> = listed.iter().map(|t| t.id.as_str()).collect();
    let engines: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
    assert_eq!(ids, engines);
    for template in &listed {
        assert!(!template.name.is_empty(), "{template:?}");
        assert!(!template.description.is_empty(), "{template:?}");
        let mut b = board();
        if let Err(e) = place_template(&mut b, &template.id) {
            panic!("{}: {e}", template.id);
        }
        assert!(!page_is_empty(&b), "{} placed nothing", template.id);
    }
}

#[test]
fn an_unknown_template_is_refused_and_places_nothing() {
    let mut b = board();
    assert!(matches!(
        place_template(&mut b, "no such template"),
        Err(BoardError::Engine(_))
    ));
    assert!(page_is_empty(&b));
}

#[test]
fn table_places_rows_and_columns() {
    let mut b = board();
    place_table(&mut b, 2, 3).unwrap();
    let doc = b.doc().unwrap();
    let page = doc.node(b.session.page().unwrap()).unwrap();
    let [table] = page.children[..] else {
        panic!("one new layer on the page: {:?}", page.children);
    };
    let table = doc.node(table).unwrap();
    assert!(matches!(table.board, Some(BoardItem::Table)));
    // The rows lay out one under another; the lines over them stay where they are.
    let rows: Vec<_> = table
        .children
        .iter()
        .filter_map(|id| doc.node(*id))
        .filter(|row| !row.child_layout.absolute)
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.children.len(), 3);
    }
    assert!(inside(content(&b), viewport(&b)));
}

#[test]
fn a_table_size_the_engine_refuses_is_an_error() {
    let mut b = board();
    for (rows, cols) in [(0, 3), (3, 0), (51, 3), (3, 21)] {
        assert!(
            matches!(place_table(&mut b, rows, cols), Err(BoardError::Engine(_))),
            "{rows} x {cols}"
        );
    }
    assert!(page_is_empty(&b));
}
