//! Convert to Layers on a small page as its web view would read it: a header row, a card column
//! with a title, a body text and a button, a badge pinned to the card's corner and an icon.

use figcraft_doc::{FileKind, LayoutMode, NodeKind};
use serde_json::{Value, json};

use super::plan::{Kind, Sizing};
use super::{Capture, build, plan};
use crate::doc::BoardDoc;

fn text(t: &str, r: Value, fs: f64, color: &str) -> Value {
    json!({"t": "#text", "tx": t, "r": r, "s": {"c": color, "ff": "-apple-system, system-ui, sans-serif",
        "fs": fs, "fw": 400, "lh": fs * 1.5, "ta": "start"}})
}

/// A 393-wide page: body (margin 0) holds a header (flex row, 16 padding, gap 12: an icon and a
/// title) and a card (flex column, 24 padding, gap 8, white with a border and radius 12: a title,
/// a two-line body and a button), and a badge absolutely placed on the card.
pub(super) fn page() -> Value {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M4 12h16" stroke="rgb(17, 24, 39)" fill="none" stroke-width="2"/></svg>"##;
    json!({"w": 393, "h": 852, "bg": "rgb(245, 246, 248)", "title": "Today",
    "skipped": {"pseudo": 1, "backgrounds": 0, "images": 0, "over": 0},
    "root": {"t": "body", "r": [0, 0, 393, 852], "s": {"d": "block", "p": [0,0,0,0], "bw": [0,0,0,0]}, "k": [
        {"t": "header", "r": [0, 0, 393, 56], "nm": "Header",
         "s": {"d": "flex", "fd": "row", "jc": "flex-start", "ai": "center", "g": [12, 12],
               "p": [16, 16, 16, 16], "bw": [0,0,0,0], "bg": "rgb(255, 255, 255)"},
         "k": [
            {"t": "svg", "r": [16, 16, 24, 24], "svg": svg, "nm": "Menu", "s": {"grow": 0}},
            {"t": "h1", "r": [52, 16, 120, 24], "s": {"d": "block", "grow": 0},
             "k": [text("Today", json!([52, 16, 60, 24]), 18.0, "rgb(17, 24, 39)")]}
         ]},
        {"t": "section", "r": [16, 72, 361, 200], "nm": "card",
         "s": {"d": "flex", "fd": "column", "jc": "normal", "ai": "stretch", "g": [8, 8],
               "p": [23, 23, 23, 23], "bw": [1, 1, 1, 1], "bd": [1, 1, 1, 1],
               "bc": "rgb(229, 231, 235)", "bg": "rgb(255, 255, 255)", "rd": [12, 12, 12, 12],
               "sh": "rgba(0, 0, 0, 0.06) 0px 8px 24px 0px", "pos": "relative"},
         "k": [
            text("Morning run", json!([40, 96, 313, 27]), 18.0, "rgb(17, 24, 39)"),
            text("Five kilometres along the river, at an easy pace before work.", json!([40, 131, 313, 48]), 16.0, "rgb(75, 85, 99)"),
            {"t": "button", "r": [40, 187, 313, 44], "nm": "Start run",
             "s": {"d": "flex", "fd": "row", "jc": "center", "ai": "center", "g": [0, 0],
                   "p": [12, 16, 12, 16], "bw": [0,0,0,0], "bg": "rgb(37, 99, 235)", "rd": [10,10,10,10], "grow": 0},
             "k": [text("Start run", json!([160, 199, 73, 20]), 16.0, "rgb(255, 255, 255)")]},
            {"t": "span", "r": [313, 80, 48, 20], "nm": "badge",
             "s": {"d": "block", "pos": "absolute", "bg": "rgb(254, 243, 199)", "rd": [6,6,6,6]},
             "k": [text("New", json!([321, 80, 32, 20]), 12.0, "rgb(17, 24, 39)")]}
         ]}
    ]}})
}

fn capture() -> Capture {
    Capture::parse(&page().to_string()).unwrap()
}

#[test]
fn the_plan_keeps_rows_columns_and_what_is_pinned() {
    let root = plan(&capture());
    assert_eq!(root.rect, [0.0, 0.0, 393.0, 852.0]);
    let layout = root.layout.as_ref().expect("the body stacks its children");
    assert!(layout.vertical);
    assert_eq!(layout.gap, 16.0, "header bottom 56, card top 72");
    let header = &root.kids[0];
    let l = header.layout.as_ref().unwrap();
    assert!(!l.vertical);
    assert_eq!((l.gap, l.cross, l.padding), (12.0, "center", [16.0; 4]));
    // The h1 that draws nothing is its text.
    assert!(matches!(&header.kids[1].kind, Kind::Text(t) if t == "Today"));
    assert!(matches!(header.kids[0].kind, Kind::Svg(_)));
    let card = &root.kids[1];
    let l = card.layout.as_ref().unwrap();
    assert!(l.vertical);
    assert_eq!((l.gap, l.main, l.cross), (8.0, "min", "min"));
    assert_eq!(l.padding, [24.0; 4], "padding and border");
    // Across the column, the texts and the button fill it; the badge stays where it is.
    assert_eq!(card.kids[0].sizing.0, Sizing::Fill);
    assert_eq!(card.kids[2].sizing.0, Sizing::Fill);
    assert_eq!(card.kids[0].sizing.1, Sizing::Hug);
    assert!(card.kids[3].absolute);
    // The button centres its label.
    let button = &card.kids[2];
    let l = button.layout.as_ref().unwrap();
    assert_eq!((l.main, l.cross), ("center", "center"));
    assert_eq!(button.kids[0].sizing.0, Sizing::Hug);
}

#[test]
fn uneven_gaps_keep_their_places() {
    let mut page = page();
    // The card's body moved 9 px further down: no even gap, no auto layout.
    page["root"]["k"][1]["k"][1]["r"][1] = json!(140);
    let capture = Capture::parse(&page.to_string()).unwrap();
    let root = plan(&capture);
    assert!(root.kids[1].layout.is_none());
}

#[test]
fn a_screen_converts_to_editable_layers_in_one_step() {
    let mut board = BoardDoc::new_board("Design").unwrap();
    board.session.doc_mut().unwrap().kind = FileKind::Design;
    let page_id = board.session.page().unwrap();
    let capture = capture();
    let root = plan(&capture);
    let before = board.doc().unwrap().node_count();
    let report = build(
        &mut board,
        &root,
        &capture,
        page_id,
        (500.0, 0.0),
        "Today / Mobile",
        None,
    )
    .unwrap();
    let doc = board.doc().unwrap();
    let frame = doc.node(report.frame).unwrap();
    assert_eq!(frame.name, "Today / Mobile");
    assert_eq!((frame.width, frame.height), (393.0, 852.0));
    assert!(report.texts >= 5 && report.pictures == 1, "{report:?}");
    assert_eq!(report.auto_layouts, 5, "{report:?}");
    assert_eq!(report.skipped.pseudo, 1);
    // Colours and type sizes became variables, and the layers are bound to them.
    assert!(
        report.variables.iter().any(|v| v.starts_with("text/")),
        "{report:?}"
    );
    assert!(report.variables.iter().any(|v| v == "type/16"));
    let card = doc
        .subtree(report.frame)
        .into_iter()
        .filter_map(|id| doc.node(id))
        .find(|n| n.name == "card")
        .unwrap();
    assert_eq!(card.layout.as_ref().unwrap().mode, LayoutMode::Vertical);
    assert!(!card.bound.is_empty(), "the card's fill is a token");
    let texts: Vec<_> = (doc.subtree(report.frame).into_iter())
        .filter_map(|id| doc.node(id))
        .filter(|n| matches!(n.kind, NodeKind::Text(_)))
        .collect();
    assert!(texts.iter().any(|t| t.name.starts_with("Five kilometres")));
    // One undo takes it all away.
    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(board.doc().unwrap().node_count(), before);
}

#[test]
fn fonts_colours_and_shadows_read_as_the_canvas_takes_them() {
    use super::looks::{family, hex, shadows};
    assert_eq!(
        family("-apple-system, BlinkMacSystemFont, sans-serif"),
        "SF Pro"
    );
    assert_eq!(family("\"No Such Face\", ui-monospace, monospace"), "Menlo");
    assert_eq!(hex("rgb(37, 99, 235)").as_deref(), Some("#2563eb"));
    assert_eq!(hex("rgba(0, 0, 0, 0.5)").as_deref(), Some("#00000080"));
    let s = shadows("rgba(0, 0, 0, 0.06) 0px 8px 24px 0px, rgb(255, 0, 0) 0px 0px 0px 1px inset");
    assert_eq!(s.len(), 2);
    assert_eq!(s[0]["offset"], json!([0.0, 8.0]));
    assert_eq!(s[0]["radius"], json!(24.0));
    assert_eq!(s[1]["type"], "innerShadow");
}
