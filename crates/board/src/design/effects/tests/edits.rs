//! Editing an effect's row: what changes, what stays as stored, and the edits that are refused.

use figcraft_color::{BlendMode, Color};
use figcraft_doc::{BlurType, EffectKind as Held};

use super::*;
use crate::doc::BoardError;

/// A glass, noise or texture effect is a row of its own kind and stays what it is.
#[test]
fn editing_a_shadow_keeps_a_glass_neighbour() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "glass"}, {"type": "dropShadow"}, {"type": "noise"}, {"type": "texture"}]),
    );
    let before = held(&board, r);
    let sel = read(&board, &[r]);
    let rows = effects(&sel).unwrap();
    let kinds: Vec<_> = rows.iter().map(|row| row.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            other("glass"),
            EffectKind::DropShadow,
            other("noise"),
            other("texture")
        ],
        "every effect is listed, in order"
    );

    let mut shadow = rows[1].clone();
    shadow.x = 6.0;
    shadow.blur = 12.0;
    apply(&mut board, &set_effect(&sel, 1, &shadow));
    let after = held(&board, r);
    assert_eq!(after.len(), 4);
    for neighbour in [0, 2, 3] {
        assert_eq!(after[neighbour], before[neighbour], "effect {neighbour}");
    }
    assert_ne!(after[1], before[1]);
    assert_eq!(rows_of(&board, &[r])[1].x, 6.0);

    // Removing the shadow leaves them as well, and so does adding another effect.
    let neighbours = vec![before[0].clone(), before[2].clone(), before[3].clone()];
    edit(&mut board, &[r], |sel| remove_effect(sel, 1));
    assert_eq!(held(&board, r), neighbours);
    edit(&mut board, &[r], |sel| {
        add_effect(sel, EffectKind::LayerBlur)
    });
    let after = held(&board, r);
    assert_eq!(after.len(), 4);
    assert_eq!(after[..3], neighbours[..]);
    assert_eq!(after[3], Effect::layer_blur());
}

#[test]
fn a_row_the_panel_cannot_edit_changes_only_its_eye() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "glass"}, {"type": "noise", "mode": "duo"}]),
    );
    let before = held(&board, r);
    let sel = read(&board, &[r]);

    // The eye of the noise.
    let mut noise = effects(&sel).unwrap()[1].clone();
    noise.visible = false;
    apply(&mut board, &set_effect(&sel, 1, &noise));
    let after = held(&board, r);
    assert_eq!(after[0], before[0]);
    assert!(!after[1].visible);
    assert_eq!(after[1].kind, before[1].kind, "everything else is the same");
    assert!(!rows_of(&board, &[r])[1].visible);

    // The fields it doesn't use are ignored, not written.
    let sel = read(&board, &[r]);
    let noisy = EffectRow {
        visible: true,
        x: 9.0,
        blur: 7.0,
        hex: "#FF0000".into(),
        opacity: 0.1,
        ..noise
    };
    apply(&mut board, &set_effect(&sel, 1, &noisy));
    assert_eq!(held(&board, r), before);
}

#[test]
fn a_shadow_edit_keeps_its_blend_and_show_behind() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "dropShadow", "blend": "multiply", "showBehind": true}]),
    );
    let sel = read(&board, &[r]);
    let mut shadow = effects(&sel).unwrap().remove(0);
    shadow.x = 5.0;
    apply(&mut board, &set_effect(&sel, 0, &shadow));

    let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
        panic!("{:?}", held(&board, r));
    };
    assert_eq!(held_shadow.offset.0, 5.0);
    assert_eq!(held_shadow.blend, BlendMode::Multiply);
    assert!(held_shadow.show_behind);
}

#[test]
fn a_blur_edit_keeps_its_progressive_settings() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{
            "type": "layerBlur", "radius": 10, "blurType": "progressive",
            "startRadius": 2, "start": [0.5, 0.1], "end": [0.5, 0.9],
        }]),
    );
    let sel = read(&board, &[r]);
    let mut blur = effects(&sel).unwrap().remove(0);
    assert_eq!(blur.blur, 10.0);
    blur.blur = 20.0;
    apply(&mut board, &set_effect(&sel, 0, &blur));

    let held_blur = held(&board, r)[0].blur().unwrap().clone();
    assert_eq!(held_blur.radius, 20.0);
    assert_eq!(held_blur.blur_type, BlurType::Progressive);
    assert_eq!(held_blur.start_radius, 2.0);
    assert_eq!(held_blur.start, (0.5, 0.1));
    assert_eq!(held_blur.end, (0.5, 0.9));
}

#[test]
fn a_stored_shadow_colour_is_not_rounded_by_an_edit_that_leaves_it_alone() {
    let mut board = design_file();
    // Not a colour of 8 bits, as the engine stores it: JSON numbers are doubles, colours floats.
    let color = Color::new(0.123_f64 as f32, 0.456_f64 as f32, 0.789_f64 as f32, 0.5);
    let r = with_effects(
        &mut board,
        json!([{"type": "dropShadow", "color": {"r": 0.123, "g": 0.456, "b": 0.789, "a": 0.5}}]),
    );
    let sel = read(&board, &[r]);
    let mut shadow = effects(&sel).unwrap().remove(0);
    assert_eq!(shadow.hex, "#1F74C9");

    // The offset changes, and the colour as the row still shows it stays as stored.
    shadow.x = 3.0;
    apply(&mut board, &set_effect(&sel, 0, &shadow));
    let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
        panic!("{:?}", held(&board, r));
    };
    assert_eq!(held_shadow.offset.0, 3.0);
    assert_eq!(held_shadow.color, color);

    // A colour that was typed is the colour, to the bit.
    shadow.hex = "#1F74CA".into();
    edit(&mut board, &[r], |sel| set_effect(sel, 0, &shadow));
    let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
        panic!("{:?}", held(&board, r));
    };
    assert_eq!(held_shadow.color.to_rgba8(), [0x1F, 0x74, 0xCA, 128]);
}

#[test]
fn effect_edits_apply_to_every_selected_layer_in_one_undo_step() {
    let mut board = design_file();
    let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
    let r2 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
    let sel = read(&board, &[r1, r2]);
    let steps = undo_steps(&board);

    let mut shadow = effects(&sel).unwrap().remove(0);
    shadow.blur = 16.0;
    apply(&mut board, &set_effect(&sel, 0, &shadow));
    assert_eq!(undo_steps(&board), steps + 1);
    for id in [r1, r2] {
        assert_eq!(rows_of(&board, &[id])[0].blur, 16.0);
    }
    board.execute("edit.undo", &json!({})).unwrap();
    for id in [r1, r2] {
        assert_eq!(held(&board, id), [Effect::drop_shadow()]);
    }
}

/// With no list to change a row of, no such row, or a row the engine could not take, an edit
/// says so by being refused, and the file stays as it was.
#[test]
fn edits_that_cannot_apply_are_refused_and_change_nothing() {
    let mut board = design_file();
    let r1 = with_effects(
        &mut board,
        json!([{"type": "dropShadow"}, {"type": "layerBlur"}]),
    );
    let r2 = rectangle(&mut board, "R2", 300.0);
    let one = read(&board, &[r1]);
    let differing = read(&board, &[r1, r2]);
    let shadow = effects(&one).unwrap().remove(0);
    let blur = effects(&one).unwrap().remove(1);

    let edits = [
        remove_effect(&one, 2),
        remove_effect(&one, usize::MAX),
        remove_effect(&differing, 0),
        set_effect(&one, 2, &shadow),
        set_effect(&differing, 0, &shadow),
        // A shadow needs a colour and numbers.
        set_effect(
            &one,
            0,
            &EffectRow {
                hex: "#12".into(),
                ..shadow.clone()
            },
        ),
        set_effect(
            &one,
            0,
            &EffectRow {
                hex: "#FF000080".into(),
                ..shadow.clone()
            },
        ),
        set_effect(
            &one,
            0,
            &EffectRow {
                x: f64::NAN,
                ..shadow.clone()
            },
        ),
        set_effect(
            &one,
            0,
            &EffectRow {
                opacity: f64::INFINITY,
                ..shadow.clone()
            },
        ),
        // So does a blur.
        set_effect(
            &one,
            1,
            &EffectRow {
                blur: f64::NEG_INFINITY,
                ..blur
            },
        ),
    ];
    let snapshot = read(&board, &[r1, r2]);
    let steps = undo_steps(&board);
    for edit in edits {
        let revision = board.revision();
        let result = board.execute(edit.command, &edit.params);
        assert!(
            matches!(
                result,
                Err(BoardError::Engine(_) | BoardError::Unavailable { .. })
            ),
            "{edit:?}"
        );
        assert_eq!(board.revision(), revision, "{edit:?}");
    }
    assert_eq!(read(&board, &[r1, r2]), snapshot);
    assert_eq!(undo_steps(&board), steps);
}

/// A hex that isn't a colour, and numbers that aren't finite, don't matter for a row whose
/// kind doesn't use them.
#[test]
fn a_blur_or_other_row_ignores_the_fields_it_does_not_use() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "layerBlur", "radius": 4}, {"type": "glass"}]),
    );
    let sel = read(&board, &[r]);
    let rows = effects(&sel).unwrap();
    let junk = |row: &EffectRow| EffectRow {
        hex: "not a colour".into(),
        x: f64::NAN,
        y: f64::NAN,
        spread: f64::NAN,
        opacity: f64::NAN,
        ..row.clone()
    };
    let mut blur = junk(&rows[0]);
    blur.blur = 6.0;
    apply(&mut board, &set_effect(&sel, 0, &blur));
    assert_eq!(rows_of(&board, &[r])[0].blur, 6.0);

    // Each edit sends the whole list it was made from, so the next one reads the file again.
    let sel = read(&board, &[r]);
    apply(&mut board, &set_effect(&sel, 1, &junk(&rows[1])));
    assert_eq!(rows_of(&board, &[r])[0].blur, 6.0);
    assert_eq!(held(&board, r)[1], Effect::glass());
}
