//! Tests of the token derivation, on the fixtures and on small scans.

use super::*;
use crate::site_design::scan::{ColorRole, TextSample};

fn fixture() -> Tokens {
    let scan: Scan = serde_json::from_str(include_str!("../fixtures/scan.json")).unwrap();
    derive(&scan, "2026-10-30")
}

fn mono() -> Tokens {
    let scan: Scan = serde_json::from_str(include_str!("../fixtures/mono.json")).unwrap();
    derive(&scan, "2026-10-30")
}

fn semantic<'a>(t: &'a Tokens, name: &str) -> Option<&'a Rgba> {
    t.semantic.iter().find(|(n, _)| n == name).map(|(_, c)| c)
}

fn role<'a>(t: &'a Tokens, name: &str) -> &'a TypeToken {
    &t.type_roles
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .1
}

fn named<T: Clone>(v: &[(String, T)]) -> Vec<(&str, T)> {
    v.iter().map(|(n, x)| (n.as_str(), x.clone())).collect()
}

#[test]
fn roles_from_the_fixture() {
    let t = fixture();
    let roles: Vec<(&str, String)> = t
        .palette
        .iter()
        .map(|r| (r.role.as_str(), r.base.to_hex()))
        .collect();
    assert_eq!(
        roles,
        vec![
            ("primary", "#0071e3".to_string()),
            ("secondary", "#bf4800".to_string()),
            ("accent", "#29a354".to_string()),
            ("neutral", "#1d1d1f".to_string()),
        ]
    );
    let neutral = &t.palette[3];
    assert_eq!(neutral.steps[10].to_hex(), "#1d1d1f");
    assert_eq!(neutral.uses, 300);
    assert_eq!(t.palette[0].uses, 14);
    assert_eq!(t.palette[0].steps[6].to_hex(), "#0071e3");
    assert_eq!(t.facts.primary_buttons, 12);
    assert_eq!(t.facts.primary_links, 0);
    assert_eq!(t.facts.colors_seen, 9);
    assert!(!t.facts.dark);
}

#[test]
fn semantic_from_the_fixture() {
    let t = fixture();
    let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
    assert_eq!(hex("background").as_deref(), Some("#ffffff"));
    assert_eq!(hex("foreground").as_deref(), Some("#1d1d1f"));
    assert_eq!(hex("muted").as_deref(), Some("#f5f5f7"));
    assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));
    assert_eq!(hex("border").as_deref(), Some("#d2d2d7"));
    assert_eq!(hex("primary").as_deref(), Some("#0071e3"));
    assert_eq!(hex("primary-foreground").as_deref(), Some("#ffffff"));
    assert_eq!(hex("ring").as_deref(), Some("#0071e3"));
}

#[test]
fn type_from_the_fixture() {
    let t = fixture();
    let body = role(&t, "body-md");
    assert_eq!((body.size, body.weight), (17.0, 400));
    assert_eq!(body.line_height, Some(1.47));
    assert_eq!(body.family, "SF Pro Text");
    assert_eq!(body.letter_spacing.as_deref(), Some("-0.022em"));
    assert_eq!(t.facts.body.as_ref(), Some(body));
    assert_eq!(t.facts.smallest_text, 12.0);

    let lg = role(&t, "body-lg");
    assert_eq!((lg.size, lg.weight), (21.0, 400));
    let sm = role(&t, "body-sm");
    assert_eq!(sm.size, 14.0);
    let display = role(&t, "display");
    assert_eq!((display.size, display.weight), (48.0, 600));
    assert_eq!(display.family, "SF Pro Display");
    assert_eq!(role(&t, "label").line_height, Some(1.18));
    assert_eq!(role(&t, "caption").size, 12.0);
    assert_eq!(role(&t, "title").size, 21.0);

    assert_eq!(
        named(&t.type_scale),
        vec![
            ("xs", 12.),
            ("sm", 14.),
            ("base", 17.),
            ("lg", 21.),
            ("xl", 28.),
            ("2xl", 40.),
            ("3xl", 48.)
        ]
    );

    let fam: Vec<&str> = t.families.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(fam, vec!["sans", "display"]);
    assert!(t.families[0].1.starts_with("\"SF Pro Text\""));
}

#[test]
fn layout_from_the_fixture() {
    let t = fixture();
    assert_eq!(t.spacing_base, Some(4.));
    assert_eq!(
        named(&t.spacing),
        vec![
            ("xs", 4.),
            ("sm", 8.),
            ("md", 12.),
            ("lg", 16.),
            ("xl", 20.),
            ("2xl", 24.),
            ("3xl", 32.)
        ]
    );
    assert_eq!(
        named(&t.radius),
        vec![
            ("sm", "4px".to_string()),
            ("md", "8px".to_string()),
            ("lg", "12px".to_string()),
            ("xl", "18px".to_string()),
            ("full", "9999px".to_string()),
        ]
    );
    let sh: Vec<&str> = t.shadows.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(sh, vec!["sm", "md", "lg"]);
    assert!(t.shadows[0].1.contains("1px 2px"));
    assert!(t.shadows[1].1.contains("2px 8px"));
    assert!(t.shadows[2].1.contains("8px 30px"));
    assert_eq!(t.breakpoints, vec![734, 1068]);
    assert_eq!(t.variables[0].0, "--sk-body-background-color");
}

#[test]
fn components_use_refs() {
    let t = fixture();
    let b = t
        .components
        .iter()
        .find(|c| c.name == "button-primary")
        .unwrap();
    let get = |k: &str| {
        b.props
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("backgroundColor"), Some("{colors.primary}"));
    assert_eq!(get("textColor"), Some("{colors.primary-foreground}"));
    assert_eq!(get("rounded"), Some("{rounded.full}"));
    assert_eq!(get("padding"), Some("8px 16px"));
    assert_eq!(get("typography"), Some("{typography.label}"));
    assert_eq!(get("height"), Some("36px"));
    assert_eq!(t.components.len(), 1);
}

#[test]
fn monochrome_has_no_roles() {
    let t = mono();
    let roles: Vec<&str> = t.palette.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(roles, vec!["neutral"]);
    assert!(semantic(&t, "primary").is_none());
    assert!(semantic(&t, "primary-foreground").is_none());
    assert!(semantic(&t, "ring").is_none());
    assert!(t.spacing_base.is_none());
    assert!(t.radius.is_empty() && t.shadows.is_empty() && t.spacing.is_empty());
    assert_eq!(t.site, "Mono");
}

#[test]
fn primary_falls_back_to_links_then_contrast() {
    use crate::site_design::scan::ColorUse;
    let use_ = |value: &str, role, count, on_link| ColorUse {
        value: value.into(),
        role,
        count,
        on_button: 0,
        on_link,
    };
    let scan = Scan {
        url: "https://x.dev".into(),
        colors: vec![
            use_("rgb(17, 17, 17)", ColorRole::Text, 100, 0),
            use_("rgb(250, 204, 21)", ColorRole::Background, 20, 0),
            use_("rgb(0, 102, 204)", ColorRole::Text, 10, 10),
            use_("transparent", ColorRole::Background, 500, 0),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-30");
    assert_eq!(t.palette[0].role, "primary");
    assert_eq!(t.palette[0].base.to_hex(), "#0066cc");
    assert_eq!(t.palette[1].base.to_hex(), "#facc15");
    assert_eq!(t.facts.primary_links, 10);
    assert_eq!(t.facts.colors_seen, 3);
    let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
    assert_eq!(hex("primary-foreground").as_deref(), Some("#ffffff"));
    assert_eq!(hex("ring").as_deref(), Some("#0066cc"));
    assert!(hex("background").is_none() && hex("border").is_none());
    assert!(t.components.is_empty() && t.type_roles.is_empty() && t.type_scale.is_empty());
}

fn use_(value: &str, role: ColorRole, count: u32) -> crate::site_design::scan::ColorUse {
    crate::site_design::scan::ColorUse {
        value: value.into(),
        role,
        count,
        on_button: 0,
        on_link: 0,
    }
}

#[test]
fn translucent_colors_are_not_roles() {
    let scan = Scan {
        colors: vec![
            use_("rgba(0, 0, 0, 0.1)", ColorRole::Shadow, 900),
            use_("rgba(0, 113, 227, 0.1)", ColorRole::Background, 400),
            use_("rgb(51, 51, 51)", ColorRole::Text, 100),
            use_("rgb(0, 0, 0)", ColorRole::Text, 50),
            use_("rgba(0, 0, 0, 0.12)", ColorRole::Border, 30),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-30");
    let roles: Vec<(&str, String)> = t
        .palette
        .iter()
        .map(|r| (r.role.as_str(), r.base.to_hex()))
        .collect();
    assert_eq!(roles, vec![("neutral", "#333333".to_string())]);
    assert_eq!(t.palette[0].uses, 100);
    // An observed translucent border stays translucent.
    let border = semantic(&t, "border").unwrap();
    assert_eq!(border.to_hex(), "#0000001f");
    // Equal hue and lightness but different alpha don't cluster.
    let scan = Scan {
        colors: vec![
            use_("rgb(0, 0, 0)", ColorRole::Text, 10),
            use_("rgba(0, 0, 0, 0.5)", ColorRole::Text, 50),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-30");
    assert_eq!(t.palette[0].uses, 10);
}

#[test]
fn muted_skips_the_body_colors() {
    let scan = Scan {
        body_background: Some("rgb(243, 244, 246)".into()),
        colors: vec![
            use_("rgb(255, 255, 255)", ColorRole::Background, 300),
            use_("rgb(243, 244, 246)", ColorRole::Background, 100),
            use_("rgb(30, 41, 59)", ColorRole::Text, 200),
            use_("rgb(107, 114, 128)", ColorRole::Text, 50),
            use_("rgb(156, 163, 175)", ColorRole::Text, 20),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-30");
    let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
    assert_eq!(hex("background").as_deref(), Some("#f3f4f6"));
    assert_eq!(hex("muted").as_deref(), Some("#ffffff"));
    assert_eq!(hex("foreground").as_deref(), Some("#1e293b"));
    assert_eq!(hex("muted-foreground").as_deref(), Some("#6b7280"));
}

#[test]
fn foreground_is_weighted_by_text_length() {
    let text = |color: &str, tag: &str, chars: u32| TextSample {
        tag: tag.into(),
        in_main: tag != "li",
        family: "Inter".into(),
        used_family: Some("Inter".into()),
        size: 16.0,
        weight: 400,
        line_height: None,
        letter_spacing: None,
        color: color.into(),
        chars,
    };
    let scan = Scan {
        body_background: Some("rgb(255, 255, 255)".into()),
        // Elements: 9 links, 3 paragraphs, 1 muted note.
        colors: vec![
            use_("rgb(0, 102, 204)", ColorRole::Text, 9),
            use_("rgb(29, 29, 31)", ColorRole::Text, 3),
            use_("rgb(110, 110, 115)", ColorRole::Text, 1),
        ],
        // Text: the paragraphs carry most of it.
        texts: vec![
            text("rgb(0, 102, 204)", "a", 60),
            text("rgb(29, 29, 31)", "p", 900),
            text("rgb(110, 110, 115)", "p", 40),
            // Footer fine print outside main: longest, but not the foreground.
            text("rgba(0, 0, 0, 0.56)", "li", 3000),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-31");
    let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
    assert_eq!(hex("foreground").as_deref(), Some("#1d1d1f"));
    assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));

    // With nothing in main, every sample counts.
    let mut outside = scan.clone();
    outside.texts.iter_mut().for_each(|t| t.in_main = false);
    let t = derive(&outside, "2026-10-31");
    let fg = semantic(&t, "foreground").map(|c| c.to_hex());
    assert_eq!(fg.as_deref(), Some("#0000008f"));
}

#[test]
fn foreground_is_readable_on_the_background() {
    let text = |color: &str, chars: u32| TextSample {
        tag: "p".into(),
        in_main: true,
        family: "Inter".into(),
        used_family: Some("Inter".into()),
        size: 17.0,
        weight: 400,
        line_height: None,
        letter_spacing: None,
        color: color.into(),
        chars,
    };
    let mut scan = Scan {
        body_background: Some("rgb(255, 255, 255)".into()),
        // Light text on dark hero tiles has the most characters.
        texts: vec![
            text("rgb(245, 245, 247)", 860),
            text("rgb(29, 29, 31)", 250),
        ],
        ..Default::default()
    };
    let fg = |s: &Scan| {
        let t = derive(s, "2026-10-31");
        semantic(&t, "foreground").map(|c| c.to_hex())
    };
    assert_eq!(fg(&scan).as_deref(), Some("#1d1d1f"));
    // Nothing readable on the background: the unfiltered pick.
    scan.texts.truncate(1);
    assert_eq!(fg(&scan).as_deref(), Some("#f5f5f7"));
    // Unknown background: no filter.
    scan.body_background = None;
    scan.texts.push(text("rgb(29, 29, 31)", 250));
    assert_eq!(fg(&scan).as_deref(), Some("#f5f5f7"));
}

#[test]
fn muted_text_is_not_the_background() {
    let scan = Scan {
        body_background: Some("rgb(255, 255, 255)".into()),
        colors: vec![
            use_("rgb(29, 29, 31)", ColorRole::Text, 9),
            use_("rgb(255, 255, 255)", ColorRole::Text, 3),
            use_("rgb(245, 245, 247)", ColorRole::Text, 2),
            use_("rgb(110, 110, 115)", ColorRole::Text, 1),
            use_("rgb(245, 245, 247)", ColorRole::Background, 5),
        ],
        ..Default::default()
    };
    let t = derive(&scan, "2026-10-31");
    let hex = |n: &str| semantic(&t, n).map(|c| c.to_hex());
    assert_eq!(hex("muted-foreground").as_deref(), Some("#6e6e73"));
}

#[test]
fn site_names() {
    assert_eq!(site_name("https://www.apple.com/iphone/"), "Apple");
    assert_eq!(site_name("localhost:5173"), "Localhost");
    assert_eq!(site_name("http://localhost:5173/x"), "Localhost");
    assert_eq!(site_name("https://docs.github.com"), "Github");
    assert_eq!(site_name("https://www.bbc.co.uk/news"), "Bbc");
}

#[test]
fn box_shadows() {
    let s = parse_box_shadow("rgba(0, 0, 0, 0.08) 0px 2px 8px 0px").unwrap();
    assert_eq!(
        (s.x, s.y, s.blur, s.spread, s.inset),
        (0., 2., 8., 0., false)
    );
    assert!((s.color.a - 0.08).abs() < 0.01);
    let s = parse_box_shadow("inset 0 0 0 1px #ccc, 0 1px 2px red").unwrap();
    assert!(s.inset);
    assert_eq!(s.spread, 1.);
    assert_eq!(s.color.to_hex(), "#cccccc");
    assert!(parse_box_shadow("none").is_none());
}
