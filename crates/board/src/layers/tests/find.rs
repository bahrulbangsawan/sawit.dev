//! ⌘F's layers: by name, in the panel's order, on the pages asked for.

use sawit_core::find::{FindOptions, matcher};

use super::*;
use crate::layers::find::find_layers;

fn re(query: &str) -> sawit_core::find::Regex {
    let regex = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    matcher(query, regex).unwrap().unwrap()
}

#[test]
fn layers_are_found_in_the_panels_order() {
    let t = tree();
    let doc = t.doc();
    // F holds G (with C), A and B; the panel lists B, A, G: the top first.
    assert_eq!(
        find_layers(&doc, &[t.page], &re("^[abc]$")),
        [t.b, t.a, t.c]
    );
    assert_eq!(find_layers(&doc, &[t.page], &re("f")), [t.f]);
    assert!(
        find_layers(&doc, &[], &re("f")).is_empty(),
        "no page, nothing"
    );
}

#[test]
fn other_pages_only_when_asked() {
    let mut doc = Document::new("Design");
    let first = doc.first_page().unwrap();
    let card = add(&mut doc, first, NodeKind::Frame, "Card");
    let second = doc.add_page("Second");
    let other = add(&mut doc, second, NodeKind::Frame, "Card copy");
    assert_eq!(find_layers(&doc, &[first], &re("card")), [card]);
    assert_eq!(
        find_layers(&doc, &[first, second], &re("card")),
        [card, other]
    );
}
