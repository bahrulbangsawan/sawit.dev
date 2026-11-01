//! Typed input as a number: what a number field takes, and what it refuses.

use super::*;

#[test]
fn parse_rejects_bad_numbers() {
    assert_eq!(parse_number("width", "abc"), None);
    assert_eq!(parse_number("width", "-5"), None);
    assert_eq!(parse_number("x", "1e400"), None);
    assert_eq!(parse_number("x", ""), None);
    assert_eq!(parse_number("x", "-5"), Some(-5.0));
    assert_eq!(parse_number("opacity", "50%"), Some(0.5));
    assert_eq!(parse_number("opacity", "3"), Some(1.0));
    assert_eq!(parse_number("paddingTop", "-5"), None);
}

#[test]
fn parse_refuses_what_is_not_a_finite_number() {
    for input in [
        "NaN", "inf", "-inf", "Infinity", "  ", "1,5", "12px", "0x10", "--3",
    ] {
        assert_eq!(parse_number("x", input), None, "{input:?}");
    }
    assert_eq!(parse_number("opacity", "%"), None);
    assert_eq!(parse_number("opacity", "abc%"), None);
    assert_eq!(parse_number("opacity", "1e400%"), None);
}

#[test]
fn parse_trims_and_reads_plain_numbers() {
    assert_eq!(parse_number("x", " 12 "), Some(12.0));
    assert_eq!(parse_number("y", "\t-3.5\n"), Some(-3.5));
    assert_eq!(parse_number("rotation", "-90"), Some(-90.0));
    assert_eq!(parse_number("width", "0"), Some(0.0));
    assert_eq!(parse_number("width", "1e2"), Some(100.0));
    // A percent sign belongs to opacity alone.
    assert_eq!(parse_number("width", "50%"), None);
    assert_eq!(parse_number("x", "50%"), None);
}

#[test]
fn opacity_is_a_fraction_or_a_percent_kept_between_0_and_1() {
    assert_eq!(parse_number("opacity", "0.25"), Some(0.25));
    assert_eq!(parse_number("opacity", " 50 % "), Some(0.5));
    assert_eq!(parse_number("opacity", "100%"), Some(1.0));
    assert_eq!(parse_number("opacity", "150%"), Some(1.0));
    assert_eq!(parse_number("opacity", "-0.5"), Some(0.0));
    assert_eq!(parse_number("opacity", "-20%"), Some(0.0));
}

#[test]
fn only_sizes_gaps_paddings_and_radii_refuse_a_negative() {
    for key in [
        "width",
        "height",
        "strokeWeight",
        "fontSize",
        "gap",
        "crossGap",
        "padding",
        "paddingTop",
        "paddingRight",
        "paddingBottom",
        "paddingLeft",
        "cornerRadius",
        "topLeftRadius",
        "topRightRadius",
        "bottomRightRadius",
        "bottomLeftRadius",
    ] {
        assert_eq!(parse_number(key, "-1"), None, "{key}");
        assert_eq!(parse_number(key, "1"), Some(1.0), "{key}");
        assert_eq!(parse_number(key, "0"), Some(0.0), "{key}");
    }
    for key in ["x", "y", "rotation"] {
        assert_eq!(parse_number(key, "-1"), Some(-1.0), "{key}");
    }
    // Minus zero is zero.
    for key in ["width", "x", "opacity"] {
        let zero = parse_number(key, "-0").unwrap();
        assert!(zero == 0.0 && zero.is_sign_positive(), "{key}");
    }
}
