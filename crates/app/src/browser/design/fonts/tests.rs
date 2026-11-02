//! The Fonts tab's tests: families grouped by the face used, their scale, specimens and weights.

use super::{css_line, group_fonts, scale, tailwind_line, weight_name};
use sawit_core::site_design::scan::{FontFace, Scan, TextSample};

fn text(stack: &str, used: Option<&str>, size: f64, weight: u16, chars: u32) -> TextSample {
    TextSample {
        tag: "p".into(),
        in_main: true,
        family: stack.into(),
        used_family: used.map(Into::into),
        size,
        weight,
        line_height: Some(size * 1.5),
        letter_spacing: None,
        color: "#000".into(),
        chars,
    }
}

#[test]
fn families_group_by_the_face_used() {
    let scan = Scan {
        texts: vec![
            text("Arial", Some("Arial"), 14., 400, 10),
            text("\"Inter\", sans-serif", Some("Inter"), 16., 700, 50),
            text("\"Inter\", sans-serif", Some("Inter"), 16., 400, 30),
            text("\"Inter\", sans-serif", Some("Inter"), 12., 400, 5),
        ],
        font_faces: vec![FontFace {
            family: "\"Inter\"".into(),
            weight: "400".into(),
            style: "normal".into(),
            src: vec!["https://a.dev/i.woff2".into()],
        }],
        ..Scan::default()
    };
    let groups = group_fonts(&scan);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].family, "Inter");
    assert_eq!(groups[0].weights, [400, 700]);
    assert_eq!(groups[0].sizes, [12., 16.]);
    assert_eq!(groups[0].files.len(), 1);
    assert!(groups[1].files.is_empty());
    assert_eq!(css_line(&groups[0]), "font-family: \"Inter\", sans-serif;");
    assert_eq!(
        tailwind_line(&groups[0], Some("sans")),
        "--font-sans: \"Inter\", sans-serif;"
    );
    assert_eq!(tailwind_line(&groups[1], None), "--font-arial: Arial;");
}

/// Fluid type's sub-pixel sizes read as the whole pixels a reader sees, each once.
#[test]
fn sizes_round_into_a_compact_scale() {
    assert_eq!(
        scale(&[15., 16.8, 18.85, 16.4, 0., f64::NAN]),
        [15, 16, 17, 19]
    );
    assert!(scale(&[]).is_empty());
}

/// A specimen is drawn in the face itself, or in the app's own for the system's CSS
/// names; a web font this Mac lacks gets none.
#[test]
fn specimens_use_a_face_this_mac_has() {
    let installed = vec!["Georgia".to_string(), "Menlo".to_string()];
    let face = |f: &str| super::face_for(f, &installed, ".SystemUIFont", "Menlo");
    assert_eq!(face("georgia").as_deref(), Some("Georgia"));
    assert_eq!(face("-apple-system").as_deref(), Some(".SystemUIFont"));
    assert_eq!(face("BlinkMacSystemFont").as_deref(), Some(".SystemUIFont"));
    assert_eq!(face("ui-monospace").as_deref(), Some("Menlo"));
    assert_eq!(face("Kebun Display"), None);
}

#[test]
fn weights_have_names() {
    assert_eq!(weight_name(400), "400 Regular");
    assert_eq!(weight_name(600), "600 Semibold");
    assert_eq!(weight_name(900), "900 Black");
}
