//! A tab's state: focus, zoom, names and the order panes are stepped through.

use crate::terminal::{Layout, Rect, Removed, SplitDir, TabState};

fn tab_area() -> Rect {
    Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    }
}
const TAB_MIN: (f32, f32) = (160., 80.);

#[test]
fn split_focuses_the_new_pane_and_unzooms() {
    let mut t = TabState::new(1);
    assert!(t.split(SplitDir::Right, 2, tab_area(), TAB_MIN));
    assert_eq!(t.focused, 2);
    assert!(t.toggle_zoom());
    assert!(t.split(SplitDir::Down, 3, tab_area(), TAB_MIN));
    assert_eq!(t.zoomed, None);
    assert_eq!(t.panes.len(), 3);
}

/// An agent splits the pane it names, which may not be the one the user has focused, and the
/// focus stays with the user's pane; the new pane goes beside the named one.
#[test]
fn split_pane_splits_the_named_pane_and_leaves_the_focus() {
    let mut t = TabState::new(1);
    assert!(t.split(SplitDir::Right, 2, tab_area(), TAB_MIN));
    t.focus(1);
    assert!(t.toggle_zoom());
    assert!(t.split_pane(2, SplitDir::Down, 3, tab_area(), TAB_MIN));
    assert_eq!(t.focused, 1, "the agent's split took the focus");
    assert_eq!(t.zoomed, None, "the new half is hidden behind the zoom");
    assert_eq!(t.layout.panes(), vec![1, 2, 3]);
    assert!(t.panes.contains_key(&3));
    // A pane the tab doesn't have, or one too small to halve, is refused and nothing changes.
    assert!(!t.split_pane(9, SplitDir::Down, 4, tab_area(), TAB_MIN));
    let small = Rect {
        x: 0.,
        y: 0.,
        w: 300.,
        h: 400.,
    };
    assert!(!t.split_pane(1, SplitDir::Right, 5, small, TAB_MIN));
    assert_eq!(t.layout.panes(), vec![1, 2, 3]);
    assert_eq!(t.panes.len(), 3);
}

#[test]
fn split_is_refused_when_the_pane_is_too_small() {
    let mut t = TabState::new(1);
    let small = Rect {
        x: 0.,
        y: 0.,
        w: 300.,
        h: 400.,
    };
    assert!(!t.split(SplitDir::Right, 2, small, TAB_MIN));
    assert_eq!(t.layout, Layout::Pane(1));
    assert_eq!(t.panes.len(), 1);
}

#[test]
fn closing_the_zoomed_pane_unzooms_and_focuses_the_sibling() {
    let mut t = TabState::new(1);
    t.split(SplitDir::Right, 2, tab_area(), TAB_MIN);
    t.toggle_zoom();
    assert_eq!(t.close(2), Removed::Focus(1));
    assert_eq!((t.focused, t.zoomed), (1, None));
    assert_eq!(t.close(1), Removed::Last);
}

#[test]
fn zoom_needs_two_panes_and_shows_only_the_zoomed_one() {
    let mut t = TabState::new(1);
    assert!(!t.toggle_zoom());
    t.split(SplitDir::Right, 2, tab_area(), TAB_MIN);
    t.toggle_zoom();
    assert_eq!(t.visible(tab_area()), vec![(2, tab_area())]);
    t.focus(1);
    assert_eq!(t.zoomed, None);
}

#[test]
fn rename_trims_and_clears() {
    let mut t = TabState::new(1);
    t.rename(1, "  api  ");
    assert_eq!(t.panes[&1].name.as_deref(), Some("api"));
    t.rename(1, "   ");
    assert_eq!(t.panes[&1].name, None);
    t.toggle_right_clicks(1);
    assert!(t.panes[&1].send_right_clicks);
}

/// A 2×2 tab made the way ⌘D then ⇧⌘D in each half makes it: its split tree lists the left
/// column before the right one, but the panes read row by row, and stepping wraps round.
#[test]
fn next_pane_goes_in_reading_order_and_wraps() {
    let mut t = TabState::new(1);
    assert_eq!(t.next_pane(Some(tab_area()), true), None, "one pane");
    assert!(t.split(SplitDir::Right, 2, tab_area(), TAB_MIN));
    t.focus(1);
    assert!(t.split(SplitDir::Down, 3, tab_area(), TAB_MIN));
    t.focus(2);
    assert!(t.split(SplitDir::Down, 4, tab_area(), TAB_MIN));
    // 1 2
    // 3 4
    assert_eq!(t.layout.panes(), vec![1, 3, 2, 4]);
    assert_eq!(t.reading_order(Some(tab_area())), vec![1, 2, 3, 4]);
    let mut seen = Vec::new();
    t.focus(1);
    for _ in 0..4 {
        let next = t.next_pane(Some(tab_area()), true).unwrap();
        seen.push(next);
        t.focus(next);
    }
    assert_eq!(seen, vec![2, 3, 4, 1]);
    assert_eq!(
        t.next_pane(Some(tab_area()), false),
        Some(4),
        "back from the first"
    );
    // Not drawn yet: the split tree's order.
    assert_eq!(t.next_pane(None, true), Some(3));
}

/// A tab's own name is apart from its panes': splitting, focusing another pane or closing one
/// leaves it as it is.
#[test]
fn a_tab_name_stays_with_the_tab() {
    let mut t = TabState::new(1);
    t.rename_tab("  server  ");
    assert_eq!(t.name.as_deref(), Some("server"));
    assert_eq!(t.panes[&1].name, None, "the pane took the tab's name");
    assert!(t.split(SplitDir::Right, 2, tab_area(), TAB_MIN));
    t.focus(1);
    t.close(2);
    assert_eq!(t.name.as_deref(), Some("server"));
    t.rename_tab(" ");
    assert_eq!(t.name, None);
}
