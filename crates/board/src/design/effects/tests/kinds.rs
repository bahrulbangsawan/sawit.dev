//! Changing the kind of an effect: what it keeps, and when it starts from the defaults.

use figcraft_color::{BlendMode, Color};
use figcraft_doc::{BlurType, EffectKind as Held};

use super::*;

#[test]
fn changing_the_kind_of_a_shadow_or_a_blur_keeps_everything() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([
            {"type": "dropShadow", "offset": [4, 5], "radius": 8, "spread": 2,
             "color": {"r": 1, "g": 0, "b": 0, "a": 0.5}, "blend": "multiply"},
            {"type": "layerBlur", "radius": 10, "blurType": "progressive", "startRadius": 2},
        ]),
    );
    let sel = read(&board, &[r]);
    let rows = effects(&sel).unwrap();
    let mut inner = rows[0].clone();
    inner.kind = EffectKind::InnerShadow;
    let mut background = rows[1].clone();
    background.kind = EffectKind::BackgroundBlur;
    apply(&mut board, &set_effect(&sel, 0, &inner));
    edit(&mut board, &[r], |sel| set_effect(sel, 1, &background));

    let after = held(&board, r);
    let Held::InnerShadow(shadow) = &after[0].kind else {
        panic!("{after:?}");
    };
    assert_eq!(shadow.offset, (4.0, 5.0));
    assert_eq!((shadow.radius, shadow.spread), (8.0, 2.0));
    assert_eq!(shadow.color, Color::new(1.0, 0.0, 0.0, 0.5));
    assert_eq!(shadow.blend, BlendMode::Multiply);
    let Held::BackgroundBlur(blur) = &after[1].kind else {
        panic!("{after:?}");
    };
    assert_eq!(blur.radius, 10.0);
    assert_eq!(blur.blur_type, BlurType::Progressive);
    assert_eq!(blur.start_radius, 2.0);
}

#[test]
fn changing_a_shadow_into_a_blur_or_back_starts_from_the_defaults_and_keeps_the_radius() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "dropShadow", "offset": [4, 5], "radius": 8, "visible": false}]),
    );
    let sel = read(&board, &[r]);
    let mut blur = effects(&sel).unwrap().remove(0);
    blur.kind = EffectKind::LayerBlur;
    apply(&mut board, &set_effect(&sel, 0, &blur));
    let mut expected = Effect::layer_blur();
    expected.blur_mut().unwrap().radius = 8.0;
    expected.visible = false;
    assert_eq!(held(&board, r), [expected]);

    let sel = read(&board, &[r]);
    let mut shadow = effects(&sel).unwrap().remove(0);
    assert_eq!(shadow.kind, EffectKind::LayerBlur);
    shadow.kind = EffectKind::InnerShadow;
    apply(&mut board, &set_effect(&sel, 0, &shadow));
    let rows = rows_of(&board, &[r]);
    assert_eq!(
        rows,
        [EffectRow {
            kind: EffectKind::InnerShadow,
            visible: false,
            x: 0.0,
            y: 4.0,
            blur: 8.0,
            spread: 0.0,
            hex: "#000000".into(),
            opacity: 0.25,
        }],
        "the engine's inner shadow, at the radius the blur had"
    );

    // To an effect the panel doesn't edit: its defaults, and the eye.
    let sel = read(&board, &[r]);
    let mut glass = rows[0].clone();
    glass.kind = other("glass");
    apply(&mut board, &set_effect(&sel, 0, &glass));
    let mut expected = Effect::glass();
    expected.visible = false;
    assert_eq!(held(&board, r), [expected]);

    // One of those into another: nothing of the glass goes into the noise.
    let sel = read(&board, &[r]);
    let mut noise = effects(&sel).unwrap().remove(0);
    noise.kind = other("noise");
    apply(&mut board, &set_effect(&sel, 0, &noise));
    let mut expected = Effect::noise();
    expected.visible = false;
    assert_eq!(held(&board, r), [expected]);
}
