//! The Design panel's tests: its tabs, and what its footer and rows say.

use super::rows::hex_and_alpha;
use super::{DesignPanel, Segment, copied_note};
use crate::browser::panel::PANEL_WIDTH;
use gpui_kit::{TestAppContext, VisualTestContext, px, size};
use sawit_core::site_design::color;

/// The five tabs and the rescan button fit the panel's width side by side: none clipped,
/// none under another.
#[gpui_kit::test]
fn the_tabs_fit_the_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(PANEL_WIDTH), px(600.)), |_, _| DesignPanel::new());
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let mut right = px(0.);
    for selector in [
        "design-tab-inspect",
        "design-tab-colors",
        "design-tab-fonts",
        "design-tab-assets",
        "design-tab-system",
    ] {
        let tab = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} isn't drawn"));
        assert!(tab.left() >= right, "{selector} overlaps the tab before it");
        right = tab.right();
    }
    let rescan = cx.debug_bounds("design-rescan").expect("no rescan button");
    assert!(
        right <= rescan.left(),
        "the last tab runs under the rescan button"
    );
    assert!(
        rescan.right() <= px(PANEL_WIDTH),
        "the rescan button is clipped"
    );
}

#[test]
fn segments_keep_their_order() {
    let titles: Vec<&str> = Segment::ALL.iter().map(|s| s.title()).collect();
    assert_eq!(titles, ["Inspect", "Colors", "Fonts", "Assets", "System"]);
    assert_eq!(Segment::named("Colours"), Some(Segment::Colors));
    assert_eq!(Segment::named(" system "), Some(Segment::System));
    assert_eq!(Segment::named("layers"), None);
    for s in Segment::ALL {
        assert_eq!(Segment::named(s.name()), Some(s));
    }
}

/// An alpha hex (`#13264399`) reads as the colour and its opacity apart.
#[test]
fn alpha_reads_apart_from_the_hex() {
    let c = color::parse("#13264399").unwrap();
    assert_eq!(hex_and_alpha(&c), ("#132643".into(), Some("60%".into())));
    let c = color::parse("rgb(19, 38, 67)").unwrap();
    assert_eq!(hex_and_alpha(&c), ("#132643".into(), None));
}

#[test]
fn the_footer_names_what_was_copied() {
    assert_eq!(copied_note("#0071e3"), "Copied #0071e3");
    assert_eq!(copied_note(".a {\n  color: red;\n}"), "Copied .a { …");
    assert_eq!(
        copied_note(&"x".repeat(60)),
        format!("Copied {}…", "x".repeat(48))
    );
    assert_eq!(copied_note(""), "Copied");
}
