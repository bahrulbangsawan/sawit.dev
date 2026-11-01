//! The filter: the layers whose name has it, under the layers that hold them.

use super::*;

#[test]
fn filter_keeps_ancestors() {
    let t = tree();
    let rows = t.rows(&[], &[], "c");
    assert_eq!(outline(&rows), [(t.f, 0), (t.g, 1), (t.c, 2)]);
    // F and G open although nothing was expanded: they hold what was found.
    assert!(row(&rows, t.f).has_children && row(&rows, t.f).expanded);
    assert!(row(&rows, t.g).has_children && row(&rows, t.g).expanded);
    assert!(!row(&rows, t.c).has_children && !row(&rows, t.c).expanded);
}

#[test]
fn filter_ignores_case_and_surrounding_space() {
    let t = tree();
    let found = t.rows(&[], &[], "c");
    assert_eq!(t.rows(&[], &[], "C"), found);
    assert_eq!(t.rows(&[], &[], "  c "), found);
    // Matching is by a part of the name, and several layers can match.
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let card = add(&mut doc, page, NodeKind::Frame, "Card");
    let title = add(&mut doc, card, NodeKind::Rectangle, "Title");
    let other = add(&mut doc, page, NodeKind::Frame, "Other");
    let by_part = rows(&doc, page, &HashSet::new(), &[], "CAR");
    assert_eq!(ids(&by_part), [card]);
    // Card matches and its child doesn't: nothing is listed under it, so it has nothing to open.
    assert!(!by_part[0].has_children && !by_part[0].expanded);
    let several = rows(&doc, page, &HashSet::new(), &[], "t");
    assert_eq!(ids(&several), [other, card, title]);
    assert!(row(&several, card).expanded);
}

#[test]
fn a_blank_filter_lists_everything() {
    let t = tree();
    let all = t.rows(&[t.f], &[], "");
    assert_eq!(ids(&all), [t.f, t.b, t.a, t.g]);
    assert_eq!(t.rows(&[t.f], &[], "   "), all);
}

#[test]
fn filter_without_matches_is_empty() {
    let t = tree();
    // The same rows without the filter, so an empty answer is the filter's.
    assert_eq!(t.rows(&[t.f, t.g], &[], "").len(), 5);
    assert!(t.rows(&[t.f, t.g], &[], "zzz").is_empty());
    assert!(t.rows(&[], &[], "zzz").is_empty());
}

#[test]
fn filter_restores_expansion() {
    let t = tree();
    let doc = t.doc();
    // F open, G closed.
    let expanded = set(&[t.f]);
    let before = rows(&doc, t.page, &expanded, &[], "");
    assert_eq!(ids(&before), [t.f, t.b, t.a, t.g]);
    assert!(!row(&before, t.g).expanded);

    // The filter lists G open to show C...
    let found = rows(&doc, t.page, &expanded, &[], "c");
    assert_eq!(ids(&found), [t.f, t.g, t.c]);
    assert!(row(&found, t.g).expanded);
    // ...without touching the set, so clearing it gives back the rows with G closed.
    assert_eq!(expanded, set(&[t.f]));
    assert_eq!(rows(&doc, t.page, &expanded, &[], ""), before);
}
