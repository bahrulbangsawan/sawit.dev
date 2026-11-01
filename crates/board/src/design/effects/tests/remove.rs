//! Removing an effect, and the variable bindings of the effects after it.

use super::*;
use crate::design::lists::testing::{bind, bound, set_variable, variables};

#[test]
fn remove_effect_leaves_the_others() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([{"type": "dropShadow"}, {"type": "layerBlur"}, {"type": "backgroundBlur"}]),
    );
    let before = held(&board, r);
    edit(&mut board, &[r], |sel| remove_effect(sel, 1));
    assert_eq!(held(&board, r), vec![before[0].clone(), before[2].clone()]);
    edit(&mut board, &[r], |sel| remove_effect(sel, 0));
    edit(&mut board, &[r], |sel| remove_effect(sel, 0));
    assert_eq!(held(&board, r), []);
    assert_eq!(effects(&read(&board, &[r])), Some(vec![]));
}

/// Final review m1: removing an effect moves the variable bindings of the effects after it
/// down one with their effects, as one undo step, so that none is left on the effect that
/// moves into the place of the one bound.
#[test]
fn removing_an_effect_keeps_each_binding_on_its_effect() {
    let mut board = design_file();
    let r = with_effects(
        &mut board,
        json!([
            {"type": "layerBlur", "radius": 1},
            {"type": "layerBlur", "radius": 2},
            {"type": "layerBlur", "radius": 3},
        ]),
    );
    variables(&mut board, &[("soft", "number", json!(2))]);
    bind(&mut board, r, "effects/1/radius", "soft");
    let steps = undo_steps(&board);
    let made = remove_effect(&read(&board, &[r]), 0);
    assert_eq!(made.command, "effect.remove");
    apply(&mut board, &made);
    assert_eq!(undo_steps(&board), steps + 1);
    assert_eq!(bound(&board, r), ["effects/0/radius"]);
    // The variable moves the radius of the effect it was bound to, and of no other.
    set_variable(&mut board, "soft", json!(20));
    let radii: Vec<f64> = rows_of(&board, &[r]).iter().map(|row| row.blur).collect();
    assert_eq!(radii, [20.0, 3.0]);
}
