//! Tests of the cards' sizes and labels.

use super::card::name_lines;
use super::layout::short_type;
use super::*;

#[::core::prelude::v1::test]
fn long_types_are_shortened() {
    assert_eq!(short_type("character varying(255)"), "varchar(255)");
    assert_eq!(short_type("timestamp with time zone"), "timestamptz");
    assert_eq!(short_type("bigint"), "bigint");
}

#[::core::prelude::v1::test]
fn cards_grow_with_columns_within_limits() {
    let col = |name: &str| diagram::ErdColumn {
        name: name.into(),
        data_type: "text".into(),
        nullable: false,
        pk: false,
        fk: false,
    };
    let t = diagram::ErdTable {
        schema: "public".into(),
        name: "t".into(),
        columns: vec![col("a"), col(&"x".repeat(200))],
    };
    let s = card_size(&t);
    assert_eq!(s.width, MAX_W);
    assert_eq!(s.height, HEADER_H + CARD_PAD + ROW_H * 2.);
}

#[::core::prelude::v1::test]
fn zoomed_out_names_break_only_at_underscores() {
    let (_, lines) = name_lines("help_article_authors", 50., 200.);
    assert_eq!(lines, ["help_", "article_", "authors"]);
    let (_, lines) = name_lines("help_article_authors", 400., 200.);
    assert_eq!(lines, ["help_article_authors"]);
    // Too short for three lines: the rest joins the last one.
    let (fit, lines) = name_lines("help_article_authors", 50., 12.);
    assert_eq!(lines, ["help_article_authors"]);
    assert_eq!(fit, 10.);
}
