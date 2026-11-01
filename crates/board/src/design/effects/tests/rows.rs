//! The rows a layer's effects read as, and the effects [`add_effect`] adds.

use figcraft_color::Color;
use figcraft_doc::EffectKind as Held;

use super::*;

#[test]
fn drop_shadow_round_trip() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    // A rectangle starts without an effect: an empty list, not a mixed one.
    assert_eq!(effects(&read(&board, &[r])), Some(vec![]));

    let edit = add_effect(&read(&board, &[r]), EffectKind::DropShadow);
    assert_eq!(edit.command, "node.set");
    apply(&mut board, &edit);
    let sel = read(&board, &[r]);
    let mut rows = effects(&sel).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, EffectKind::DropShadow);
    assert!(rows[0].visible);

    let mut shadow = rows.remove(0);
    shadow.x = 4.0;
    shadow.y = 4.0;
    shadow.blur = 8.0;
    shadow.hex = "#000000".into();
    shadow.opacity = 0.25;
    apply(&mut board, &set_effect(&sel, 0, &shadow));

    assert_eq!(effects(&read(&board, &[r])), Some(vec![shadow.clone()]));
    // The engine holds what the row says.
    let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
        panic!("{:?}", held(&board, r));
    };
    assert_eq!(held_shadow.offset, (4.0, 4.0));
    assert_eq!(held_shadow.radius, 8.0);
    assert_eq!(held_shadow.spread, 0.0);
    assert_eq!(held_shadow.color, Color::new(0.0, 0.0, 0.0, 0.25));
}

#[test]
fn a_shadow_colour_and_spread_round_trip() {
    let mut board = design_file();
    let r = rectangle(&mut board, "R", 0.0);
    edit(&mut board, &[r], |sel| {
        add_effect(sel, EffectKind::InnerShadow)
    });
    let sel = read(&board, &[r]);
    let mut shadow = effects(&sel).unwrap().remove(0);
    assert_eq!(shadow.kind, EffectKind::InnerShadow);
    shadow.x = -2.0;
    shadow.y = 6.5;
    shadow.blur = 12.0;
    shadow.spread = -3.0;
    shadow.hex = "#ff8800".into();
    shadow.opacity = 0.3;
    apply(&mut board, &set_effect(&sel, 0, &shadow));

    let back = rows_of(&board, &[r]).remove(0);
    // A colour is stored in single precision: 30% reads back as the nearest float.
    assert!((back.opacity - 0.3).abs() < 1e-6, "{}", back.opacity);
    assert_eq!(
        back,
        EffectRow {
            hex: "#FF8800".into(),
            opacity: back.opacity,
            ..shadow
        }
    );
}

#[test]
fn add_effect_gives_each_kind_the_engines_defaults() {
    let cases = [
        (EffectKind::DropShadow, Effect::drop_shadow()),
        (EffectKind::InnerShadow, Effect::inner_shadow()),
        (EffectKind::LayerBlur, Effect::layer_blur()),
        (EffectKind::BackgroundBlur, Effect::background_blur()),
        (other("noise"), Effect::noise()),
        (other("texture"), Effect::texture()),
        (other("glass"), Effect::glass()),
    ];
    for (kind, engine) in cases {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        edit(&mut board, &[r], |sel| add_effect(sel, kind.clone()));
        assert_eq!(held(&board, r), [engine], "{kind:?}");
        assert_eq!(rows_of(&board, &[r])[0].kind, kind);
    }
}

#[test]
fn rows_read_what_each_kind_holds() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([
            {"type": "dropShadow", "color": {"r": 1, "g": 0, "b": 0, "a": 0.5}, "offset": [2, 3], "radius": 5, "spread": 1},
            {"type": "layerBlur", "radius": 9, "visible": false},
            {"type": "backgroundBlur", "radius": 20},
        ]),
    );
    assert_eq!(
        rows_of(&board, &[r]),
        [
            EffectRow {
                kind: EffectKind::DropShadow,
                visible: true,
                x: 2.0,
                y: 3.0,
                blur: 5.0,
                spread: 1.0,
                hex: "#FF0000".into(),
                opacity: 0.5,
            },
            EffectRow {
                visible: false,
                blur: 9.0,
                ..row(EffectKind::LayerBlur)
            },
            EffectRow {
                blur: 20.0,
                ..row(EffectKind::BackgroundBlur)
            },
        ]
    );
}

#[test]
fn hidden_effects_stay_listed() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "dropShadow", "visible": false}]),
    );
    let rows = rows_of(&board, &[r]);
    assert!(!rows[0].visible);
    assert_eq!(rows[0].kind, EffectKind::DropShadow);

    let mut shown = rows[0].clone();
    shown.visible = true;
    edit(&mut board, &[r], |sel| set_effect(sel, 0, &shown));
    assert_eq!(held(&board, r), [Effect::drop_shadow()]);
}

#[test]
fn mixed_effects_are_none() {
    let mut board = design_file();
    let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
    let r2 = rectangle(&mut board, "R2", 300.0);
    assert_eq!(effects(&read(&board, &[r1, r2])), None);
    // Layers that agree hold a list.
    edit(&mut board, &[r2], |sel| {
        add_effect(sel, EffectKind::DropShadow)
    });
    assert_eq!(rows_of(&board, &[r1, r2]).len(), 1);
    // And so do layers with no effect at all.
    let r3 = rectangle(&mut board, "R3", 600.0);
    let r4 = rectangle(&mut board, "R4", 900.0);
    assert_eq!(effects(&read(&board, &[r3, r4])), Some(vec![]));
    // Nothing selected has no effects.
    assert_eq!(effects(&read(&board, &[])), Some(vec![]));
}

#[test]
fn replace_all_makes_one_new_effect_the_only_effect_of_every_layer() {
    let mut board = design_file();
    let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
    let r2 = with_effects(&mut board, json!([{"type": "glass"}, {"type": "noise"}]));
    let sel = read(&board, &[r1, r2]);
    assert_eq!(effects(&sel), None);
    let steps = undo_steps(&board);

    let edit = add_effect(&sel, EffectKind::LayerBlur);
    assert_eq!(edit.params["ids"], json!([r1, r2]));
    apply(&mut board, &edit);
    assert_eq!(undo_steps(&board), steps + 1);
    for id in [r1, r2] {
        assert_eq!(held(&board, id), [Effect::layer_blur()]);
    }
    board.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(held(&board, r1), [Effect::drop_shadow()]);
    assert_eq!(held(&board, r2), [Effect::glass(), Effect::noise()]);
}
