//! The Fill and Stroke sections (spec "Design panel" 5 and 6): the layers' paints, the top one
//! first as in FigCraft's own panel, and for the strokes their weight, position and dashes.
//!
//! - A solid paint is a swatch, its hex and its opacity. The swatch opens the colour picker
//!   ([`colour_swatch`], which the shadows of the Effects section share): the colours it goes
//!   through while it is open are one scrub of the list, one undo step when the picker closes, and
//!   none when Escape closes it (ruling P5-C15). A press on the canvas, an agent's op that isn't a
//!   read, and the swatch no longer drawn for the layers the scrub began on end the scrub first,
//!   kept, and close the picker ([`BoardView::end_colour_scrub`]), so that nothing else joins the
//!   colour's undo step.
//! - A gradient, an image or a pattern shows what it is, and becomes a solid only when its
//!   "Replace with solid" button is clicked (Review Focus 4).
//! - Every paint has its eye and a button that removes it. Layers whose paints differ show
//!   "Mixed", and its "Replace all" gives them all one new paint, as the section's add button
//!   does then.
//!
//! The paints change only through `sawit_board::design::paints`, which sends a list back with
//! the rows it doesn't change as they were (delegate ruling on P5.T2), and removes a row with the
//! engine's own command, so that the rows after it keep their variable bindings (final review
//! m1); the stroke's weight, position and dashes are plain properties.

mod picker;

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::paints::{
    PaintKind, PaintRow, add_paint, paints, remove_paint, replace_with_solid, set_solid,
    set_visible,
};
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::controls::{
    Choices, choice_menu, eye_button, guarded, icon_button, mixed_row, on_click_edit, row,
    row_button, section,
};
use super::number_field::{self, NumberField, Rule, TextField};
pub(super) use picker::{ColourOf, Picker, colour_swatch};

use crate::board::BoardView;
use crate::board::option_icons;

/// The picture of the buttons that add a paint or an effect, and of those that remove one.
pub(super) const ADD_ICON: Lucide = Lucide::Plus;
pub(super) const REMOVE_ICON: Lucide = Lucide::Minus;
/// The picture of a gradient, image or pattern row's "Replace with solid" button.
pub(super) const REPLACE_ICON: Lucide = Lucide::Replace;
/// What an image paint and a pattern paint show in place of a swatch.
pub(super) const IMAGE_ICON: Lucide = Lucide::Image;
pub(super) const PATTERN_ICON: Lucide = Lucide::Shapes;

/// The width of an opacity field, beside a hex.
pub(super) const OPACITY_WIDTH: f32 = 56.;
/// The side of a swatch.
const SWATCH: f32 = 20.;

/// A dashed stroke's dashes: 4 drawn, 4 left out.
const DASHES: [f64; 2] = [4.0, 4.0];

/// Where a stroke can sit on the layer's edge: what `node.set` takes, and what the menu says.
pub(super) const STROKE_ALIGNS: [(&str, &str); 3] = [
    ("inside", "Inside"),
    ("center", "Center"),
    ("outside", "Outside"),
];

/// The Fill section for the layers of `sel`.
pub(super) fn render_fill(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let rows = paint_rows(view, sel, "fills", window, cx);
    let add = add_button("fills", "Add fill", &this);
    section("fill", "Fill", vec![add], rows, cx)
}

/// The Stroke section for the layers of `sel`: their stroke paints, then, while they have a
/// stroke, its weight, its position and whether it is dashed.
pub(super) fn render_stroke(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let mut rows = paint_rows(view, sel, "strokes", window, cx);
    if has_strokes(sel) {
        let weight = NumberField::new("strokeWeight", "Weight").render(view, sel, window, cx);
        let align = stroke_align(sel, &this);
        rows.push(row().child(weight).child(align).into_any_element());
        rows.push(dashed(sel, &this));
    }
    let add = add_button("strokes", "Add stroke", &this);
    section("stroke", "Stroke", vec![add], rows, cx)
}

/// The button that adds a solid paint to list `key` of every layer: after their paints, or, when
/// they differ, as the only one.
fn add_button(
    key: &'static str,
    tooltip: &'static str,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let selector = format!("design-{key}-add");
    icon_button(SharedString::from(selector.clone()), ADD_ICON, tooltip)
        .debug_selector(move || selector)
        .on_click(on_click_edit(view, move |sel| Some(add_paint(sel, key))))
        .into_any_element()
}

/// Whether any of the layers of `sel` has a stroke.
fn has_strokes(sel: &DesignSelection) -> bool {
    match sel.get("strokes") {
        Mixed::Same(Value::Array(strokes)) => !strokes.is_empty(),
        Mixed::Mixed => true,
        Mixed::Same(_) | Mixed::Absent => false,
    }
}

/// The rows of list `key` of the layers of `sel`, the top paint first; "Mixed" when the layers
/// hold different lists.
fn paint_rows(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    key: &'static str,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let Some(rows) = guarded("ui.paints", Some(Vec::new()), || paints(sel, key)) else {
        let replace = move |sel: &DesignSelection| Some(add_paint(sel, key));
        return vec![mixed_row(key, &this, replace, cx)];
    };
    let mut drawn = Vec::with_capacity(rows.len());
    for (index, paint) in rows.iter().enumerate().rev() {
        drawn.push(paint_row(view, sel, key, index, paint, window, cx));
    }
    drawn
}

/// Row `index` of list `key`: a solid's swatch, hex and opacity, or what a gradient, an image or a
/// pattern is with its "Replace with solid" button; then the eye and the remove button.
fn paint_row(
    view: &mut BoardView,
    sel: &DesignSelection,
    key: &'static str,
    index: usize,
    paint: &PaintRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let id = format!("{key}-{index}");
    let row = row().items_center().min_h(px(28.));
    let row = match &paint.kind {
        PaintKind::Solid { hex, opacity } => {
            let of = ColourOf::Paint { key, index };
            let swatch = colour_swatch(view, &id, of, hex, *opacity, window, cx);
            // The row's hex and opacity as the section read them, which is what their rules read.
            let hex = TextField::new(format!("{key}.{index}.hex"), hex_rule(key, index))
                .showing(hex_text(hex))
                .render(view, sel, window, cx);
            let opacity =
                TextField::new(format!("{key}.{index}.opacity"), opacity_rule(key, index))
                    .width(OPACITY_WIDTH)
                    .showing(number_field::percent(*opacity))
                    .render(view, sel, window, cx);
            row.child(swatch).child(hex).child(opacity)
        }
        other => {
            let (preview, name) = match other {
                PaintKind::Gradient(kind) => (gradient_swatch(cx), gradient_name(kind)),
                PaintKind::Image => (icon_swatch(IMAGE_ICON, cx), "Image".to_string()),
                _ => (icon_swatch(PATTERN_ICON, cx), "Pattern".to_string()),
            };
            let replace = move |sel: &DesignSelection| Some(replace_with_solid(sel, key, index));
            let replace = row_button(
                &id,
                "replace",
                REPLACE_ICON,
                "Replace with solid",
                &this,
                replace,
            );
            row.child(preview)
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(replace)
        }
    };
    let visible = paint.visible;
    let eye = eye_button(&id, visible, &this, move |sel| {
        Some(set_visible(sel, key, index, !visible))
    });
    let remove = row_button(&id, "remove", REMOVE_ICON, "Remove", &this, move |sel| {
        Some(remove_paint(sel, key, index))
    });
    row.child(eye).child(remove).into_any_element()
}

/// What a gradient row says it is: "Linear gradient".
fn gradient_name(kind: &str) -> String {
    if kind.eq_ignore_ascii_case("gradient") {
        "Gradient".to_string()
    } else {
        format!("{kind} gradient")
    }
}

/// A swatch of a gradient, for a row that holds one.
fn gradient_swatch(cx: &App) -> AnyElement {
    let theme = cx.theme();
    swatch_frame(cx)
        .bg(linear_gradient(
            90.,
            linear_color_stop(theme.background, 0.),
            linear_color_stop(theme.foreground, 1.),
        ))
        .into_any_element()
}

/// A swatch showing `icon`, for an image or a pattern row.
fn icon_swatch(icon: Lucide, cx: &App) -> AnyElement {
    swatch_frame(cx)
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).xsmall())
        .into_any_element()
}

/// The frame of a swatch that isn't a colour picker's.
fn swatch_frame(cx: &App) -> Div {
    div()
        .flex_shrink_0()
        .size(px(SWATCH))
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().input)
        .overflow_hidden()
}

/// The hex and opacity of solid row `index` of list `key`, or `None` when it isn't a solid.
fn solid(sel: &DesignSelection, key: &str, index: usize) -> Option<(String, f64)> {
    match paints(sel, key)?.into_iter().nth(index)?.kind {
        PaintKind::Solid { hex, opacity } => Some((hex, opacity)),
        _ => None,
    }
}

/// The rule of the hex field of solid row `index` of list `key`: the colour without its `#`. A
/// typed colour (`RRGGBB` or `RGB`, the `#` left out or not;
/// [`parse_hex`](sawit_board::design::paints::parse_hex)) keeps the paint's opacity, and anything
/// else is refused.
fn hex_rule(key: &'static str, index: usize) -> Rule {
    Rule::new(
        move |sel| {
            solid(sel, key, index).map_or_else(SharedString::default, |(hex, _)| hex_text(&hex))
        },
        move |sel, typed| {
            let (_, opacity) = solid(sel, key, index)?;
            set_solid(sel, key, index, typed, opacity)
        },
    )
}

/// The rule of the opacity field of solid row `index` of list `key`: a percentage, which keeps the
/// paint's colour. A number typed without `%` is one (ruling P5-C7).
fn opacity_rule(key: &'static str, index: usize) -> Rule {
    Rule::new(
        move |sel| {
            let shown = solid(sel, key, index);
            shown.map_or_else(SharedString::default, |(_, opacity)| {
                number_field::percent(opacity)
            })
        },
        move |sel, typed| {
            let (hex, _) = solid(sel, key, index)?;
            let opacity = number_field::parse("opacity", typed)?;
            set_solid(sel, key, index, &hex, opacity)
        },
    )
}

/// A colour as a hex field shows it: `#D9D9D9` is "D9D9D9".
pub(super) fn hex_text(hex: &str) -> SharedString {
    hex.trim_start_matches('#').to_string().into()
}

/// The stroke position menu: inside, centre or outside the layers' edges, theirs checked.
fn stroke_align(sel: &DesignSelection, view: &WeakEntity<BoardView>) -> AnyElement {
    let current = match sel.get("strokeAlign") {
        Mixed::Same(Value::String(align)) => STROKE_ALIGNS
            .iter()
            .map(|(value, _)| *value)
            .find(|value| *value == align),
        _ => None,
    };
    let label = STROKE_ALIGNS
        .iter()
        .find(|(value, _)| Some(*value) == current)
        .map_or("Mixed", |(_, name)| *name);
    let choices = || {
        let named = STROKE_ALIGNS.iter();
        named
            .map(|(value, name)| (*value, (*name).into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |align: &&'static str, cx: &mut App| {
        let align = *align;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|sel| Some(sel.set("strokeAlign", json!(align))), cx);
        });
    };
    let choices = Choices::new(choices, current, pick)
        .with_pictures(|align: &&'static str| option_icons::stroke_align(align));
    choice_menu("strokeAlign", label, "Stroke position", choices, false).into_any_element()
}

/// The dashed switch: on while every layer's stroke is dashed. Switched on, the strokes are
/// dashed 4 by 4; off, they are solid. A click on it gives it the keyboard, which goes back to the
/// canvas at once, so that the canvas's keys go on working.
fn dashed(sel: &DesignSelection, view: &WeakEntity<BoardView>) -> AnyElement {
    let on = matches!(sel.get("dashes"), Mixed::Same(Value::Array(dashes)) if !dashes.is_empty());
    let view = view.clone();
    let switch = Switch::new("design-dashes")
        .small()
        .checked(on)
        .label("Dashed")
        .on_click(move |dashed, window, cx| {
            let dashes = if *dashed { json!(DASHES) } else { json!([]) };
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(|sel| Some(sel.set("dashes", dashes)), cx);
                window.focus(&view.focus, cx);
            });
        });
    // In a row, so that it is as wide as the switch and its label, and a click on it is theirs.
    row()
        .child(
            div()
                .debug_selector(|| "design-dashes".into())
                .child(switch),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
