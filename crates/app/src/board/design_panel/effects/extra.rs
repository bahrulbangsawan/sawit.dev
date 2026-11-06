//! The fields of an effect beyond a shadow's and a blur's radius
//! (`sawit_board::design::effects::fields`): a blur's progressive ramp, and a noise's, a
//! texture's and a glass's fields, each a number field, a menu of choices, a switch or a colour's
//! hex, one to a row.

use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::DesignSelection;
use sawit_board::design::effects::fields::{self, Field, FieldKind};

use super::super::controls::{Choices, choice_menu, row};
use super::super::number_field::{Rule, TextField};
use crate::board::BoardView;

const FIELD_WIDTH: f32 = 72.;

/// The rows of effect `index`'s extra fields.
pub(super) fn rows(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    fields::shown_fields(sel, index)
        .into_iter()
        .map(|field| {
            let shown = fields::value(sel, index, &field).unwrap_or_default();
            let label = div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .truncate()
                .child(field.label);
            let control = control(view, sel, index, field, shown, &this, window, cx);
            row()
                .items_center()
                .child(label)
                .child(control)
                .into_any_element()
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn control(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    field: Field,
    shown: String,
    this: &WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let id = format!("effects-{index}-{}", field.key);
    match field.kind {
        FieldKind::Number { .. } | FieldKind::Colour => {
            let rule = Rule::new(
                move |sel| fields::value(sel, index, &field).unwrap_or_default().into(),
                move |sel, typed| {
                    let edit = fields::set_value(sel, index, &field, typed);
                    (edit.command == "effect.set").then_some(edit)
                },
            );
            TextField::new(id, rule)
                .width(FIELD_WIDTH)
                .showing(shown.into())
                .render(view, sel, window, cx)
        }
        FieldKind::Choice(options) => {
            let label = options
                .iter()
                .find(|(v, _)| *v == shown)
                .map_or(shown.clone(), |(_, n)| n.to_string());
            let current = options.iter().map(|(v, _)| *v).find(|v| *v == shown);
            let view = this.clone();
            let pick = move |value: &&'static str, cx: &mut App| {
                let value = *value;
                let _ = view.update(cx, |view, cx| {
                    view.edit_layers(|sel| Some(fields::set_value(sel, index, &field, value)), cx);
                });
            };
            div()
                .w(px(FIELD_WIDTH * 1.5))
                .child(choice_menu(
                    &id,
                    label,
                    "Effect setting",
                    Choices::new(
                        move || options.iter().map(|(v, n)| (*v, (*n).into())).collect(),
                        current,
                        pick,
                    ),
                    false,
                ))
                .into_any_element()
        }
        FieldKind::Toggle => {
            let view = this.clone();
            Switch::new(SharedString::from(id))
                .small()
                .checked(shown == "true")
                .on_click(move |on, window, cx| {
                    let typed = on.to_string();
                    let _ = view.update(cx, |view, cx| {
                        view.edit_layers(
                            |sel| Some(fields::set_value(sel, index, &field, &typed)),
                            cx,
                        );
                        window.focus(&view.focus, cx);
                    });
                })
                .into_any_element()
        }
    }
}
