//! The Colors tab's tests: colours grouped by role, the forms they copy as, and contrast.

use super::{contrast_pairs, group_colors, rgb_css, rgba_u32, role_class, variable_line};
use sawit_core::site_design::color;
use sawit_core::site_design::scan::{ColorRole, ColorUse, Scan};

fn use_of(value: &str, role: ColorRole, count: u32) -> ColorUse {
    ColorUse {
        value: value.into(),
        role,
        count,
        on_button: 0,
        on_link: 0,
    }
}

#[test]
fn groups_merge_duplicates_and_follow_role_order() {
    let scan = Scan {
        colors: vec![
            use_of("#111111", ColorRole::Shadow, 1),
            use_of("rgb(0, 113, 227)", ColorRole::Background, 4),
            use_of("#ffffff", ColorRole::Background, 9),
            use_of("#0071e3", ColorRole::Background, 3),
            use_of("#222", ColorRole::Stroke, 2),
            use_of("#333", ColorRole::Fill, 2),
            use_of("#000000", ColorRole::Text, 5),
            use_of("not a color", ColorRole::Border, 1),
        ],
        ..Scan::default()
    };
    let groups = group_colors(&scan);
    let roles: Vec<ColorRole> = groups.iter().map(|(r, _)| *r).collect();
    assert_eq!(
        roles,
        [
            ColorRole::Text,
            ColorRole::Background,
            ColorRole::Fill,
            ColorRole::Stroke,
            ColorRole::Shadow
        ]
    );
    let bg: Vec<(String, u32)> = groups[1].1.iter().map(|(c, n)| (c.to_hex(), *n)).collect();
    assert_eq!(bg, [("#ffffff".to_string(), 9), ("#0071e3".to_string(), 7)]);
}

#[test]
fn copy_formats() {
    let c = color::parse("#0071e3").unwrap();
    assert_eq!(rgb_css(&c), "rgb(0, 113, 227)");
    assert_eq!(
        role_class(ColorRole::Text, "#0071e3").unwrap(),
        "text-[#0071e3]"
    );
    assert_eq!(role_class(ColorRole::Shadow, "#0071e3"), None);
    let scan = Scan {
        variables: vec![("--brand".into(), "rgb(0, 113, 227)".into())],
        ..Scan::default()
    };
    assert_eq!(
        variable_line(&scan, &c).as_deref(),
        Some("--brand: rgb(0, 113, 227);")
    );
    assert_eq!(rgba_u32(&c), 0x0071e3ff);
}

/// The most used text colours are checked on the page's background, most used first; a
/// page with no background of its own reads on white.
#[test]
fn text_colours_are_checked_on_the_background() {
    let mut scan = Scan {
        colors: vec![
            use_of("#767676", ColorRole::Text, 3),
            use_of("#000000", ColorRole::Text, 9),
            use_of("#ffffff", ColorRole::Background, 9),
        ],
        ..Scan::default()
    };
    let pairs = contrast_pairs(&scan);
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].text.to_hex(), "#000000");
    assert!((pairs[0].ratio - 21.).abs() < 0.01);
    assert!(
        pairs[1].ratio >= 4.5 && pairs[1].ratio < 4.6,
        "{}",
        pairs[1].ratio
    );
    // On a black page, black text must sit on another surface: it isn't listed.
    scan.body_background = Some("#000000".into());
    let on_black = contrast_pairs(&scan);
    assert_eq!(on_black.len(), 1);
    assert_eq!(on_black[0].text.to_hex(), "#767676");
}
