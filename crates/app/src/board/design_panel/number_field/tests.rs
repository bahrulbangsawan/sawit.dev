//! How a field shows a value, reads typed input, and keeps a scrub in its range.

use sawit_board::design::Mixed;
use serde_json::json;

// Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
use super::label_scrub::{number, scrubbed, step};
use super::{parse, text};

#[test]
fn fields_show_numbers_percentages_and_mixed() {
    let same = |value| Mixed::Same(value);
    assert_eq!(text("x", &same(json!(100.0))).as_ref(), "100");
    assert_eq!(text("x", &same(json!(12.5))).as_ref(), "12.5");
    assert_eq!(text("x", &same(json!(-0.001))).as_ref(), "0");
    assert_eq!(text("width", &same(json!(33.333_333))).as_ref(), "33.33");
    // An opacity as the engine reads it back, an f32 widened.
    let opacity = f64::from(0.3_f32);
    assert_eq!(text("opacity", &same(json!(opacity))).as_ref(), "30%");
    assert_eq!(text("x", &Mixed::Mixed).as_ref(), "Mixed");
    // Sides that differ.
    assert_eq!(
        text("padding", &same(json!([1.0, 2.0, 3.0, 4.0]))).as_ref(),
        "Mixed"
    );
    assert_eq!(text("gap", &Mixed::Absent).as_ref(), "");
}

#[test]
fn typed_input_is_read_as_parse_number_reads_it() {
    assert_eq!(parse("x", " -5 "), Some(-5.0));
    assert_eq!(parse("width", "-5"), None);
    assert_eq!(parse("x", "abc"), None);
    assert_eq!(parse("x", "Mixed"), None);
    assert_eq!(parse("x", ""), None);
    // An opacity is a percentage, with its % or without (ruling P5-C7).
    assert_eq!(parse("opacity", "50"), Some(0.5));
    assert_eq!(parse("opacity", "50%"), Some(0.5));
    assert_eq!(parse("opacity", "300"), Some(1.0));
    assert_eq!(parse("opacity", "abc"), None);
}

#[test]
fn a_scrub_stays_in_the_fields_range() {
    assert_eq!(scrubbed("x", -3.0), -3.0);
    assert_eq!(scrubbed("rotation", -45.0), -45.0);
    // Sizes, gaps, paddings and radii stop at 0.
    for key in [
        "width",
        "gap",
        "paddingTop",
        "topLeftRadius",
        "cornerRadius",
    ] {
        assert_eq!(scrubbed(key, -3.0), 0.0, "{key}");
    }
    assert_eq!(scrubbed("opacity", 1.2), 1.0);
    assert_eq!(scrubbed("opacity", -0.2), 0.0);
    // Rounded to a hundredth, as the field shows it.
    assert_eq!(scrubbed("opacity", f64::from(0.3_f32) + 0.05), 0.35);
    assert_eq!(step("opacity"), 0.01);
    assert_eq!(step("x"), 1.0);
}

#[test]
fn a_list_scrubs_from_its_largest() {
    assert_eq!(number(&json!(4.0)), Some(4.0));
    assert_eq!(number(&json!([1.0, 8.0, 3.0, 2.0])), Some(8.0));
    assert_eq!(number(&json!("auto")), None);
}
