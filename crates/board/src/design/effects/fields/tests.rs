//! The fields of noise, texture, glass and a blur's progressive ramp.

use serde_json::json;

use super::*;
use crate::design::effects::{EffectKind, add_effect};
use crate::design::lists::testing::{apply, design_file, edit, read, rectangle};

fn field(fields: &[Field], label: &str) -> Field {
    *fields
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("{label}"))
}

#[test]
fn a_noise_is_edited_field_by_field_and_shows_the_fields_of_its_mode() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    edit(&mut board, &[r], |s| {
        add_effect(s, EffectKind::Other("noise".into()))
    });
    let shown = shown_fields(&read(&board, &[r]), 0);
    let labels: Vec<&str> = shown.iter().map(|f| f.label).collect();
    assert!(
        labels.contains(&"Size") && !labels.contains(&"Second colour"),
        "{labels:?}"
    );

    let size = field(&shown, "Size");
    let e = set_value(&read(&board, &[r]), 0, &size, "4");
    assert_eq!(e.command, "effect.set");
    apply(&mut board, &e);
    assert_eq!(value(&read(&board, &[r]), 0, &size).as_deref(), Some("4"));
    // Out of range or not a number: refused.
    assert_eq!(
        set_value(&read(&board, &[r]), 0, &size, "0").command,
        "node.set"
    );
    assert_eq!(
        set_value(&read(&board, &[r]), 0, &size, "big").command,
        "node.set"
    );

    let density = field(&shown, "Density");
    edit(&mut board, &[r], |s| set_value(s, 0, &density, "25%"));
    assert_eq!(
        value(&read(&board, &[r]), 0, &density).as_deref(),
        Some("25")
    );

    let colour = field(&shown, "Colour");
    edit(&mut board, &[r], |s| set_value(s, 0, &colour, "#ff8800"));
    assert_eq!(
        value(&read(&board, &[r]), 0, &colour).as_deref(),
        Some("#FF8800")
    );

    let mode = field(&shown, "Mode");
    edit(&mut board, &[r], |s| set_value(s, 0, &mode, "duo"));
    let labels: Vec<&str> = shown_fields(&read(&board, &[r]), 0)
        .iter()
        .map(|f| f.label)
        .collect();
    assert!(labels.contains(&"Second colour"), "{labels:?}");
}

#[test]
fn glass_texture_and_a_progressive_blur_take_their_fields() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    for kind in ["glass", "texture"] {
        edit(&mut board, &[r], |s| {
            add_effect(s, EffectKind::Other(kind.into()))
        });
    }
    edit(&mut board, &[r], |s| add_effect(s, EffectKind::LayerBlur));
    let glass = shown_fields(&read(&board, &[r]), 0);
    let frost = field(&glass, "Frost");
    edit(&mut board, &[r], |s| set_value(s, 0, &frost, "12"));
    assert_eq!(value(&read(&board, &[r]), 0, &frost).as_deref(), Some("12"));

    let texture = shown_fields(&read(&board, &[r]), 1);
    let clip = field(&texture, "Clip to shape");
    edit(&mut board, &[r], |s| set_value(s, 1, &clip, "true"));
    assert_eq!(
        value(&read(&board, &[r]), 1, &clip).as_deref(),
        Some("true")
    );

    let blur = shown_fields(&read(&board, &[r]), 2);
    assert_eq!(blur.len(), 1, "uniform: no ramp");
    edit(&mut board, &[r], |s| {
        set_value(s, 2, &blur[0], "progressive")
    });
    let blur = shown_fields(&read(&board, &[r]), 2);
    let start = field(&blur, "Start blur");
    edit(&mut board, &[r], |s| set_value(s, 2, &start, "3"));
    assert_eq!(value(&read(&board, &[r]), 2, &start).as_deref(), Some("3"));
    let _ = json!({});
}
