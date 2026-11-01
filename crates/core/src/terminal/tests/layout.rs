//! A tab's split layout: splitting, removing panes, their areas, dividers and neighbours.

use crate::terminal::{Direction, Layout, Rect, Removed, SplitDir, SplitPath, new_pane_id};

#[test]
fn split_right_puts_the_new_pane_after_the_target() {
    let mut l = Layout::Pane(1);
    assert!(l.split(1, SplitDir::Right, 2));
    assert!(l.split(1, SplitDir::Down, 3));
    assert_eq!(l.panes(), vec![1, 3, 2]);
    assert!(!l.split(9, SplitDir::Right, 4));
}

#[test]
fn rects_divide_the_area_by_ratio() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let r = l.rects(Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    });
    assert_eq!(
        r[0],
        (
            1,
            Rect {
                x: 0.,
                y: 0.,
                w: 400.,
                h: 400.
            }
        )
    );
    assert_eq!(
        r[1],
        (
            2,
            Rect {
                x: 400.,
                y: 0.,
                w: 400.,
                h: 200.
            }
        )
    );
    assert_eq!(
        r[2],
        (
            3,
            Rect {
                x: 400.,
                y: 200.,
                w: 400.,
                h: 200.
            }
        )
    );
}

#[test]
fn rects_never_go_negative_in_a_tiny_area() {
    let mut l = Layout::Pane(1);
    for n in 2..8 {
        l.split(n - 1, SplitDir::Right, n);
    }
    for (_, r) in l.rects(Rect {
        x: 0.,
        y: 0.,
        w: 3.,
        h: 0.,
    }) {
        assert!(r.w >= 0. && r.h >= 0. && !r.w.is_nan() && !r.h.is_nan());
    }
}

#[test]
fn removing_a_pane_gives_its_space_to_the_sibling() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    assert_eq!(l.remove(1), Removed::Focus(2));
    assert_eq!(l.panes(), vec![2, 3]);
    assert_eq!(l.remove(3), Removed::Focus(2));
    assert_eq!(l, Layout::Pane(2));
    assert_eq!(l.remove(2), Removed::Last);
    assert_eq!(l.remove(7), Removed::NotFound);
}

#[test]
fn neighbor_follows_the_direction() {
    // 1 | 2
    //   | 3
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let a = Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    };
    assert_eq!(l.neighbor(1, Direction::Right, a), Some(2));
    assert_eq!(l.neighbor(3, Direction::Left, a), Some(1));
    assert_eq!(l.neighbor(2, Direction::Down, a), Some(3));
    assert_eq!(l.neighbor(3, Direction::Up, a), Some(2));
    assert_eq!(l.neighbor(1, Direction::Left, a), None);
}

#[test]
fn set_ratio_keeps_both_sides_at_least_min() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    let root = SplitPath(vec![]);
    l.set_ratio(&root, 0.05, 800., (160., 80.));
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.2).abs() < 1e-6));
    l.set_ratio(&root, 0.99, 800., (160., 80.));
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.8).abs() < 1e-6));
    l.even(&root, 800., (160., 80.));
    assert!(matches!(l, Layout::Split { ratio, .. } if ratio == 0.5));
    assert_eq!(
        l.dividers(Rect {
            x: 0.,
            y: 0.,
            w: 800.,
            h: 400.
        })[0]
            .at,
        400.
    );
    // Too short for two minimums: back to even.
    l.set_ratio(&root, 0.9, 300., (160., 80.));
    assert!(matches!(l, Layout::Split { ratio, .. } if ratio == 0.5));
}

fn root_ratio(l: &Layout) -> f32 {
    match l {
        Layout::Split { ratio, .. } => *ratio,
        Layout::Pane(_) => panic!("not split"),
    }
}

#[test]
fn dragging_a_parent_divider_stops_at_the_nested_panes_minimum() {
    // A | (B | C): the right side needs room for two panes.
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Right, 3);
    let root = SplitPath(vec![]);
    l.set_ratio(&root, 0.9, 800., (160., 80.));
    assert!((root_ratio(&l) - 0.6).abs() < 1e-6, "{}", root_ratio(&l));
    let area = Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    };
    for (_, r) in l.rects(area) {
        assert!(r.w >= 160. - 1e-3, "{r:?}");
    }
    l.set_ratio(&root, 0.0, 800., (160., 80.));
    assert!((root_ratio(&l) - 0.2).abs() < 1e-6);

    // A | (B / C): stacked panes only need one pane's width.
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    l.set_ratio(&root, 0.9, 800., (160., 80.));
    assert!((root_ratio(&l) - 0.8).abs() < 1e-6);
}

#[test]
fn even_respects_nested_minimums() {
    // A | (B | C | D) in 800: the right side needs 480, so even stops at 0.4.
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Right, 3);
    l.split(3, SplitDir::Right, 4);
    let root = SplitPath(vec![]);
    l.set_ratio(&root, 0.2, 800., (160., 80.));
    l.even(&root, 800., (160., 80.));
    assert!((root_ratio(&l) - 0.4).abs() < 1e-6, "{}", root_ratio(&l));
}

#[test]
fn pane_ids_are_unique() {
    let ids: std::collections::HashSet<_> = (0..1000).map(|_| new_pane_id()).collect();
    assert_eq!(ids.len(), 1000);
}
