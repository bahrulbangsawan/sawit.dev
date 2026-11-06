//! The Typography section's details, under its fields (`sawit_board::design::typography`):
//! decoration, case, list and wrap menus; paragraph spacing, indent and the most lines; and the
//! OpenType features and variable axes, typed as tags.

use std::rc::Rc;

use gpui_kit::*;
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::controls::{Choices, choice_menu, row};
use super::number_field::{NumberField, Rule, TextField};
use crate::board::BoardView;

/// The type details under the fields: decoration, case, list and wrap menus; paragraph spacing,
/// indent and the most lines; and the OpenType features and variable axes, typed as tags
/// (`sawit_board::design::typography`).
pub(super) fn details(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    use sawit_board::design::typography::{
        DETAIL_CHOICES, parse_features, parse_variations, tags_text,
    };
    let this = cx.entity().downgrade();
    let mut menus = Vec::new();
    for (key, label, options) in DETAIL_CHOICES {
        let current = match sel.get(key) {
            Mixed::Same(Value::String(v)) => options.iter().map(|(o, _)| *o).find(|o| *o == v),
            _ => None,
        };
        let shown = current
            .and_then(|c| options.iter().find(|(o, _)| *o == c))
            .map_or(label.to_string(), |(_, n)| (*n).to_string());
        let view = this.clone();
        let pick = move |value: &&'static str, cx: &mut App| {
            let value = *value;
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(|sel| Some(sel.set(key, json!(value))), cx);
            });
        };
        menus.push(choice_menu(
            &format!("text-{key}"),
            shown,
            label,
            Choices::new(
                move || options.iter().map(|(o, n)| (*o, (*n).into())).collect(),
                current,
                pick,
            ),
            false,
        ));
    }
    let mut menus = menus.into_iter();
    let mut rows = Vec::new();
    while let Some(first) = menus.next() {
        let line = row().child(first).children(menus.next());
        rows.push(line.into_any_element());
    }
    rows.push(
        row()
            .child(
                NumberField::new("paragraphSpacing", "Paragraph spacing")
                    .render(view, sel, window, cx),
            )
            .child(NumberField::new("paragraphIndent", "Indent").render(view, sel, window, cx))
            .into_any_element(),
    );
    rows.push(
        row()
            .child(NumberField::new("maxLines", "Max lines").render(view, sel, window, cx))
            .child(div().flex_1())
            .into_any_element(),
    );
    let tags = |key: &'static str, parse: fn(&str) -> Option<Value>| {
        Rule::new(
            move |sel| match sel.get(key) {
                Mixed::Same(v) => tags_text(&v).into(),
                _ => SharedString::default(),
            },
            move |sel, typed| Some(sel.set(key, parse(typed)?)),
        )
    };
    rows.push(
        TextField::new("features", tags("features", parse_features))
            .label("OpenType features (liga=off, tnum)")
            .render(view, sel, window, cx),
    );
    rows.push(
        TextField::new("fontVariations", tags("fontVariations", parse_variations))
            .label("Variable axes (wght=550)")
            .render(view, sel, window, cx),
    );
    rows
}
