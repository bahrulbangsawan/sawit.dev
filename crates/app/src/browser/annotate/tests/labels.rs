//! What the cards and the group headers say, and the order and the boxes they come in.

use sawit_core::annotations::{Rect, SourceHint};

use super::at;
use crate::browser::annotate::card::{element_summary, first_line, real_component, source_line};
use crate::browser::annotate::rows::clear_label;
use crate::browser::annotate::view::{route_line, split_url};
use crate::browser::annotate::{group_order, padded_rect};

#[test]
fn padding_clamps_to_the_viewport() {
    let r = Rect {
        x: 5.,
        y: 5.,
        width: 100.,
        height: 40.,
    };
    assert_eq!(padded_rect(&r, (393, 852), 8.), (0., 0., 113., 53.));
    let r = Rect {
        x: 380.,
        y: 100.,
        width: 50.,
        height: 10.,
    };
    let (x, _, w, _) = padded_rect(&r, (393, 852), 8.);
    assert_eq!((x, w), (372., 21.));
}

#[test]
fn current_page_comes_first() {
    let items = [
        at("http://a/1", 1),
        at("http://a/2", 1),
        at("http://a/1", 2),
    ];
    assert_eq!(
        group_order("http://a/2", items.iter().map(|a| a.url.as_str())),
        ["http://a/2", "http://a/1"]
    );
    assert_eq!(
        group_order("http://a/9#x", items.iter().map(|a| a.url.as_str())),
        ["http://a/1", "http://a/2"]
    );
}

#[test]
fn urls_split_into_path_and_host() {
    assert_eq!(
        split_url("http://localhost:5173/a?b=1"),
        ("/a?b=1".into(), "localhost:5173".into())
    );
    assert_eq!(split_url("http://x.dev"), ("/".into(), "x.dev".into()));
}

/// A card says what the element is in words before its size, and where its source is.
#[test]
fn cards_name_the_element_and_its_source() {
    let mut a = at("http://a/1", 1);
    a.element.rect.width = 180.4;
    a.element.rect.height = 40.;
    assert_eq!(element_summary(&a), "180×40");
    a.element.text = Some("  Shop  ".into());
    assert_eq!(element_summary(&a), "“Shop” · 180×40");
    a.element.role = Some("button".into());
    a.element.name = Some("Shop the harvest".into());
    assert_eq!(element_summary(&a), "button “Shop the harvest” · 180×40");
    assert_eq!(source_line(&a), None);
    a.element.source = Some(SourceHint {
        file: "src/Hero.tsx".into(),
        line: Some(24),
        framework: "React".into(),
        component: Some("HeroActions".into()),
    });
    assert_eq!(
        source_line(&a).as_deref(),
        Some("src/Hero.tsx:24 · <HeroActions>")
    );
    assert_eq!(clear_label(1), "Clear annotation");
    assert_eq!(clear_label(3), "Clear 3 annotations");
}

/// A minified or placeholder component name (`<$u>` on a production page) says nothing, so a
/// card leaves it out and keeps the source file; a name someone wrote stays.
#[test]
fn junk_component_names_are_hidden() {
    for junk in ["$u", "$", "Xe", "t", "_c", "$Foo", "Anonymous", "  ", "a b"] {
        assert!(!real_component(junk), "{junk:?}");
    }
    for real in [
        "HeroActions",
        "nav-bar",
        "Card.Header",
        "ForwardRef(Button)",
        "App",
    ] {
        assert!(real_component(real), "{real:?}");
    }
    let mut a = at("http://a/1", 1);
    a.element.source = Some(SourceHint {
        file: String::new(),
        line: None,
        framework: "React".into(),
        component: Some("$u".into()),
    });
    assert_eq!(source_line(&a), None, "nothing worth a line");
    a.element.source = Some(SourceHint {
        file: "src/Hero.tsx".into(),
        line: Some(3),
        framework: "React".into(),
        component: Some("$u".into()),
    });
    assert_eq!(source_line(&a).as_deref(), Some("src/Hero.tsx:3"));
}

/// A group header shows the page's path only when it says more than the title: not the home
/// page's `/`. A folded card shows the first line of its note that has words.
#[test]
fn headers_and_folded_cards_say_only_what_adds() {
    assert_eq!(route_line("/"), None);
    assert_eq!(
        route_line("/pricing?plan=pro").as_deref(),
        Some("/pricing?plan=pro")
    );
    assert_eq!(first_line("\n  fix this.\nand that"), "fix this.");
    assert_eq!(first_line(" \n "), "");
}
