//! The Typography section: the fonts it offers, and how a text with styled runs reads.
//!
//! The section's fields read and write through [`DesignSelection::get`] and
//! [`DesignSelection::set`], under the keys `node.set` takes. A write goes to the text layers of
//! the selection only, and restyles the whole text, runs included:
//!
//! - `fontFamily` and `fontStyle` are names: `"Helvetica"` and `"Bold Oblique"`. [`families`] and
//!   [`styles`] say which the fonts have.
//! - `fontSize` is a number.
//! - `lineHeight` and `letterSpacing` read as the engine labels them: `"Auto"`, `"24"` (pixels) or
//!   `"150%"` for a line height, and `"0%"`, `"2"` (pixels) or `"2%"` for a letter spacing.
//!   `node.set` takes the same labels back, and a number of pixels.
//! - `textAlign` is `"left"`, `"center"`, `"right"` or `"justified"`; `textAlignVertical` is
//!   `"top"`, `"middle"` or `"bottom"`; `textAutoResize` is `"widthAndHeight"`, `"height"`,
//!   `"none"` or `"truncate"`.
//!
//! A text can hold runs of characters styled apart from the rest, and the engine's inspector tells
//! only the base style. `fold_runs` reads what the characters show, so that a text with two font
//! sizes reads as mixed in the size field.
//!
//! [`DesignSelection::get`]: super::DesignSelection::get
//! [`DesignSelection::set`]: super::DesignSelection::set

use figcraft_doc::{TextData, TextStyle};
use serde_json::{Map, Value, json};

/// What a character's style holds for one field, as the field reads.
type StyleValue = fn(&TextStyle) -> Value;

/// The Typography fields a run of characters can change, each with how a character's style holds
/// it. The others, `textAlign`, `textAlignVertical` and `textAutoResize`, belong to the text as a
/// whole.
const RUN_KEYS: [(&str, StyleValue); 5] = [
    ("fontFamily", |style| json!(style.font_family)),
    ("fontStyle", |style| json!(style.font_style)),
    ("fontSize", |style| json!(style.font_size)),
    ("lineHeight", |style| json!(style.line_height.label())),
    ("letterSpacing", |style| json!(style.letter_spacing.label())),
];

/// The font families the fonts have, sorted by name without regard to case, each once.
///
/// The list is empty until the system's fonts have been scanned (`figcraft_text::scan_system_fonts`),
/// so a list made before the scan ends has to be made again.
pub fn families() -> Vec<String> {
    by_name(figcraft_text::family_names())
}

/// `names` sorted by name without regard to case, each name once: of names that differ only in
/// case, the first is kept.
fn by_name(mut names: Vec<String>) -> Vec<String> {
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup_by_key(|name| name.to_lowercase());
    names
}

/// The styles of `family` as `figcraft_text` reports them, by weight and then italic: `"Regular"`,
/// `"Bold Italic"`. A family the fonts don't have offers `"Regular"` and `"Bold"`.
pub fn styles(family: &str) -> Vec<String> {
    let styles = figcraft_text::family_styles(family);
    if styles.is_empty() {
        return vec!["Regular".to_string(), "Bold".to_string()];
    }
    styles
}

/// What the characters of `text` hold for each of the Typography fields a run can change
/// (`fontFamily`, `fontStyle`, `fontSize`, `lineHeight` and `letterSpacing`), and which of them
/// they disagree on.
///
/// The inspector gives the text's base style, and the runs that differ from it apart, as ranges of
/// characters. The panel shows what the characters look like instead: a text with a run of another
/// size over every character shows that size, and one with two sizes shows none. So each key is
/// read over the spans of uniform style the text is made of ([`TextData::spans`]). The value they
/// agree on is written to `props`, over the base style's, and a key they disagree on is returned,
/// for [`DesignSelection::get`](super::DesignSelection::get) to read as
/// [`Mixed::Mixed`](super::Mixed::Mixed), alone or beside other layers.
pub(super) fn fold_runs(text: &TextData, props: &mut Map<String, Value>) -> Vec<&'static str> {
    let spans = text.spans();
    let mut mixed = Vec::new();
    for (key, value_of) in RUN_KEYS {
        let mut values = spans.iter().map(|(_, _, style)| value_of(style));
        let Some(first) = values.next() else {
            continue;
        };
        if values.all(|value| value == first) {
            props.insert(key.to_string(), first);
        } else {
            mixed.push(key);
        }
    }
    mixed
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use serde_json::{Value, json};

    use super::*;
    use crate::design::Mixed;
    use crate::design::testing::{
        apply, design_file, inspect, make, node_set, read, same, undo_steps,
    };
    use crate::doc::BoardDoc;

    /// A text reading "Hello world" at the origin.
    fn hello(board: &mut BoardDoc) -> NodeId {
        make(
            board,
            json!({"type": "text", "name": "Title", "text": "Hello world", "x": 0, "y": 0}),
        )
    }

    /// `text.style` on the characters `start..end` of text `id`, with `props` as its other params.
    fn style(board: &mut BoardDoc, id: NodeId, start: usize, end: usize, props: Value) {
        let mut params = props;
        params["id"] = json!(id);
        params["start"] = json!(start);
        params["end"] = json!(end);
        board.execute("text.style", &params).unwrap();
    }

    fn rectangle(board: &mut BoardDoc) -> NodeId {
        make(
            board,
            json!({"type": "rectangle", "x": 0, "y": 100, "width": 50, "height": 50}),
        )
    }

    #[test]
    fn mixed_runs_show_mixed_size() {
        let mut board = design_file();
        let text = hello(&mut board);
        assert_eq!(read(&board, &[text]).get("fontSize"), same(json!(12.0)));

        style(&mut board, text, 0, 5, json!({"fontSize": 24}));
        let sel = read(&board, &[text]);
        assert_eq!(sel.get("fontSize"), Mixed::Mixed);

        let steps = undo_steps(&board);
        apply(&mut board, &sel.set("fontSize", json!(18.0)));
        assert_eq!(undo_steps(&board), steps + 1);
        assert_eq!(read(&board, &[text]).get("fontSize"), same(json!(18.0)));
        assert!(inspect(&board, text).get("styledRuns").is_none());
    }

    /// The Typography section also shows for a text selected with a rectangle: the size edit
    /// goes to the text, restyles all of it, and leaves the rectangle as it was.
    #[test]
    fn a_size_edit_over_a_mixed_text_and_a_rectangle_runs_and_changes_only_the_text() {
        let mut board = design_file();
        let text = hello(&mut board);
        let shape = rectangle(&mut board);
        style(&mut board, text, 0, 5, json!({"fontSize": 24}));
        let before = read(&board, &[shape]);

        let sel = read(&board, &[text, shape]);
        assert_eq!(sel.get("fontSize"), Mixed::Mixed);
        apply(&mut board, &sel.set("fontSize", json!(18.0)));

        let sel = read(&board, &[text, shape]);
        assert_eq!(sel.get("fontSize"), same(json!(18.0)));
        assert_eq!(read(&board, &[shape]), before);
    }

    #[test]
    fn a_mixed_text_makes_the_selection_mixed_whatever_the_others_hold() {
        let mut board = design_file();
        let plain = hello(&mut board);
        let runs = hello(&mut board);
        style(&mut board, runs, 0, 5, json!({"fontSize": 24}));
        // The base size is 12 in both, and the plain one is 12 all through.
        assert_eq!(read(&board, &[plain]).get("fontSize"), same(json!(12.0)));
        assert_eq!(read(&board, &[plain, runs]).get("fontSize"), Mixed::Mixed);
        assert_eq!(read(&board, &[runs, plain]).get("fontSize"), Mixed::Mixed);
    }

    /// The inspector reports the base style, so a text whose every character has a run reads as
    /// the base size there, and as the run's in the panel, like on the canvas.
    #[test]
    fn characters_that_all_carry_a_run_read_as_the_runs_say() {
        let mut board = design_file();
        let text = hello(&mut board);
        style(&mut board, text, 0, 5, json!({"fontSize": 24}));
        style(&mut board, text, 5, 11, json!({"fontSize": 24}));
        assert_eq!(
            inspect(&board, text)["fontSize"],
            json!(12.0),
            "the inspector gives the base style"
        );

        let sel = read(&board, &[text]);
        assert_eq!(sel.get("fontSize"), same(json!(24.0)));
        apply(&mut board, &sel.set("fontSize", json!(18.0)));
        assert_eq!(read(&board, &[text]).get("fontSize"), same(json!(18.0)));
    }

    /// Every style key a run can change reads as mixed, and only that key.
    #[test]
    fn runs_of_every_style_key_read_as_mixed_for_that_key_only() {
        let keys = [
            "fontFamily",
            "fontStyle",
            "fontSize",
            "lineHeight",
            "letterSpacing",
        ];
        for (key, run) in [
            ("fontFamily", json!({"fontFamily": "Helvetica"})),
            ("fontStyle", json!({"fontStyle": "Bold"})),
            ("fontSize", json!({"fontSize": 24})),
            ("lineHeight", json!({"lineHeight": "150%"})),
            ("letterSpacing", json!({"letterSpacing": "2%"})),
        ] {
            let mut board = design_file();
            let text = hello(&mut board);
            let plain = read(&board, &[text]);
            style(&mut board, text, 0, 5, run);
            let sel = read(&board, &[text]);
            for other in keys {
                if other == key {
                    assert_eq!(sel.get(other), Mixed::Mixed, "{key} run: {other}");
                } else {
                    assert_eq!(sel.get(other), plain.get(other), "{key} run: {other}");
                }
            }
        }
    }

    /// A run of another decoration is a run, and no Typography field differs in it.
    #[test]
    fn a_run_that_differs_in_something_else_leaves_every_field_the_same() {
        let mut board = design_file();
        let text = hello(&mut board);
        let plain = read(&board, &[text]);
        style(
            &mut board,
            text,
            0,
            5,
            json!({"textDecoration": "underline"}),
        );
        assert!(inspect(&board, text).get("styledRuns").is_some());

        let sel = read(&board, &[text]);
        for key in [
            "fontFamily",
            "fontStyle",
            "fontSize",
            "lineHeight",
            "letterSpacing",
        ] {
            assert_eq!(sel.get(key), plain.get(key), "{key}");
            assert!(matches!(sel.get(key), Mixed::Same(_)), "{key}");
        }
    }

    /// What the text holds as a whole is never mixed.
    #[test]
    fn alignment_and_auto_resize_belong_to_the_whole_text() {
        let mut board = design_file();
        let text = hello(&mut board);
        style(
            &mut board,
            text,
            0,
            5,
            json!({"fontSize": 24, "fontStyle": "Bold"}),
        );
        let sel = read(&board, &[text]);
        assert_eq!(sel.get("textAlign"), same(json!("left")));
        assert_eq!(sel.get("textAlignVertical"), same(json!("top")));
        assert_eq!(sel.get("textAutoResize"), same(json!("widthAndHeight")));
    }

    #[test]
    fn a_whole_text_edit_clears_the_mix_of_its_key_and_leaves_the_others() {
        let mut board = design_file();
        let text = hello(&mut board);
        style(
            &mut board,
            text,
            0,
            5,
            json!({"fontSize": 24, "fontStyle": "Bold"}),
        );
        let sel = read(&board, &[text]);
        assert_eq!(sel.get("fontStyle"), Mixed::Mixed);

        apply(&mut board, &sel.set("fontStyle", json!("Italic")));
        let sel = read(&board, &[text]);
        assert_eq!(sel.get("fontStyle"), same(json!("Italic")));
        assert_eq!(
            sel.get("fontSize"),
            Mixed::Mixed,
            "the size runs are still there"
        );
    }

    #[test]
    fn a_text_read_after_a_plain_edit_has_no_mix() {
        let mut board = design_file();
        let text = hello(&mut board);
        node_set(&mut board, &[text], json!({"fontSize": 20}));
        let sel = read(&board, &[text]);
        assert_eq!(sel.get("fontSize"), same(json!(20.0)));
        assert_eq!(sel.get("lineHeight"), same(json!("Auto")));
        assert_eq!(sel.get("letterSpacing"), same(json!("0%")));
    }

    #[test]
    fn families_are_sorted_and_unique() {
        // The fonts of the system are not in the database until they are scanned.
        figcraft_text::scan_system_fonts();
        let names = families();
        assert!(!names.is_empty(), "the system has no fonts to list");
        for pair in names.windows(2) {
            let (a, b) = (pair[0].to_lowercase(), pair[1].to_lowercase());
            assert!(a < b, "{:?} is not before {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn names_are_sorted_without_regard_to_case_and_each_is_kept_once() {
        let names = ["b", "Arial", "a", "arial", "B", "Zed", "Zed"].map(String::from);
        assert_eq!(by_name(names.to_vec()), ["a", "Arial", "b", "Zed"]);
        assert!(by_name(Vec::new()).is_empty());
    }

    #[test]
    fn a_family_without_styles_offers_regular_and_bold() {
        assert_eq!(styles("No Such Family 7f3a91"), ["Regular", "Bold"]);
    }

    #[test]
    fn styles_are_the_ones_figcraft_text_reports() {
        figcraft_text::scan_system_fonts();
        let family = families()
            .into_iter()
            .find(|family| !figcraft_text::family_styles(family).is_empty())
            .expect("a system font with styles");
        let reported = figcraft_text::family_styles(&family);
        assert_eq!(styles(&family), reported);
    }
}
