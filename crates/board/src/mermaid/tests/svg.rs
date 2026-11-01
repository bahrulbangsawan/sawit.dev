//! Reading Pele's SVG: where a text sits and how wide it is, transforms, boxes and number lists.

use figcraft_doc::TextData;
use kurbo::Rect;
use roxmltree::Document;

use super::*;
use crate::mermaid::geometry::{numbers, shape_box};
use crate::mermaid::style::is_shape;
use crate::mermaid::text::{ems, set_widest_word, set_width, widest_line};

#[test]
fn a_text_keeps_its_anchor_at_the_size_pele_set_it_in() {
    let items = converted(
        r#"<svg xmlns="http://www.w3.org/2000/svg" font-size="16">
                 <g transform="translate(100, 0)">
                   <text x="0" y="50">abcd</text>
                   <text x="0" y="50" text-anchor="middle">abcd</text>
                   <text x="0" y="50" style="text-anchor: end">abcd</text>
                 </g>
               </svg>"#,
    );
    assert_eq!(items.len(), 3, "{items:#?}");
    // It comes out at Pele's size, not the engine's for new text, so that is the size its
    // width is guessed at.
    let size = 16.0;
    assert_ne!(TextData::default().style.font_size, size);
    assert!(items.iter().all(|text| text.font_size == Some(size)));
    let width = set_width("abcd", size);
    let lefts: Vec<f64> = items.iter().map(|text| text.x).collect();
    assert!(near(lefts[0], 100.0), "{lefts:?}");
    assert!(near(lefts[1], 100.0 - width / 2.0), "{lefts:?}");
    assert!(near(lefts[2], 100.0 - width), "{lefts:?}");
    // Each is centred on the middle of Pele's line, BASELINE ems above its baseline.
    for text in &items {
        assert_eq!(text.kind, "text");
        assert_eq!(text.text.as_deref(), Some("abcd"));
        let middle = text.y + size * LINE_HEIGHT / 2.0;
        assert!(near(middle, 50.0 - BASELINE * 16.0), "{text:?}");
    }
}

#[test]
fn text_is_estimated_wider_for_wider_letters() {
    assert!(ems("iiii") < ems("aaaa") && ems("aaaa") < ems("AAAA"));
    assert!(ems("AAAA") < ems("WWWW") && ems("WWWW") <= ems("漢字漢字"));
    assert!(near(ems("漢"), 1.0));
    assert!(near(widest_line("ab\nabcd\nabc"), ems("abcd")));
    assert!(set_widest_word("a abcd abc", 16.0) >= ems("abcd") * 16.0);
    assert!(set_widest_word("a abcd abc", 16.0) > set_widest_word("a abc", 16.0));
    assert_eq!(set_widest_word("", 16.0), 0.0);
}

fn assert_box(actual: Rect, (x0, y0, x1, y1): (f64, f64, f64, f64)) {
    let expected = Rect::new(x0, y0, x1, y1);
    let same = near(actual.x0, x0) && near(actual.y0, y0);
    assert!(
        same && near(actual.x1, x1) && near(actual.y1, y1),
        "{actual:?} is not {expected:?}"
    );
}

#[test]
fn transforms_compose_down_the_tree_and_boxes_come_from_the_geometry() {
    // The root's own transform is not the diagram's: its coordinates are.
    let svg = Document::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg" transform="translate(1000, 1000)">
                 <g transform="translate(100,50)">
                   <g transform="scale(2)"><rect x="10" y="10" width="20" height="10"/></g>
                   <g transform="rotate(90)"><rect width="10" height="20"/></g>
                   <g transform="matrix(1 0 0 1 5 6) translate(1)"><circle r="5"/></g>
                   <path d="M0,0 C0,-10 10,-10 10,0"/>
                   <polygon points="0,0 10-5 20,0" transform="scale(1, 2)"/>
                   <line x1="5" y1="5" x2="-5" y2="15"/>
                   <ellipse cx="1" cy="2" rx="3" ry="4" transform="rotate(90 1 2)"/>
                 </g>
               </svg>"#,
    )
    .unwrap();
    let boxes: Vec<Rect> = svg
        .descendants()
        .filter(|node| is_shape(*node))
        .map(|shape| shape_box(shape).unwrap())
        .collect();
    assert_eq!(boxes.len(), 7);
    assert_box(boxes[0], (120.0, 70.0, 160.0, 90.0));
    assert_box(boxes[1], (80.0, 50.0, 100.0, 60.0));
    assert_box(boxes[2], (101.0, 51.0, 111.0, 61.0));
    // A curve's box is its extent, not its control points'.
    assert_box(boxes[3], (100.0, 42.5, 110.0, 50.0));
    assert_box(boxes[4], (100.0, 40.0, 120.0, 50.0));
    assert_box(boxes[5], (95.0, 55.0, 105.0, 65.0));
    assert_box(boxes[6], (97.0, 49.0, 105.0, 55.0));
}

#[test]
fn number_lists_are_read_as_svg_writes_them() {
    assert_eq!(numbers("1,2 3"), [1.0, 2.0, 3.0]);
    assert_eq!(numbers("10-5.5.5"), [10.0, -5.5, 0.5]);
    assert_eq!(numbers("-1e2, +.5e-1 x 7"), [-100.0, 0.05, 7.0]);
    assert_eq!(numbers("1e"), [1.0]);
    assert!(numbers("- . e ,").is_empty());
}

#[test]
fn an_svg_that_cannot_be_read_is_an_error() {
    let diagram = Diagram {
        svg: "<svg><g></svg>".into(),
        width: 10.0,
        height: 10.0,
        kind: "flowchart".into(),
    };
    let error = to_skeletons(&diagram).unwrap_err();
    assert!(
        error.starts_with("the diagram's SVG can't be read"),
        "{error}"
    );
}
