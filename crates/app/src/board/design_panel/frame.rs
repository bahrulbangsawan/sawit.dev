//! The Constraints, Resizing and Layout grid sections (`sawit_board::design::frame`):
//!
//! - **Constraints**: how a layer inside a frame, outside its auto layout, follows the frame when
//!   it is resized, one menu for each axis.
//! - **Resizing**: a frame's minimum and maximum width and height, a grid layout's columns and
//!   rows, and whether the layers ignore their parent's auto layout (absolute position).
//! - **Layout grid**: the frame's columns, rows or square grid, each its kind, count, size,
//!   gutter, offset and alignment, with its eye and remove button, and an add button.

use std::rc::Rc;

use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::frame::{
    self, GRID_ALIGNS, GRID_KINDS, GridRow, HORIZONTAL, LIMITS, VERTICAL,
};
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::controls::{
    Choices, choice_menu, eye_button, icon_button, on_click_edit, row, row_button, section,
};
use super::number_field::{NumberField, Rule, TextField};
use super::paints::{ADD_ICON, REMOVE_ICON};
use crate::board::BoardView;

/// The Constraints section.
pub(super) fn render_constraints(
    _view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let menu = |axis: &'static str, options: &'static [(&'static str, &'static str)]| {
        let current = frame::constraint(sel, axis);
        let current = options
            .iter()
            .map(|(v, _)| *v)
            .find(|v| Some(*v) == current.as_deref());
        let label = options
            .iter()
            .find(|(v, _)| Some(*v) == current)
            .map_or("Mixed", |(_, n)| *n);
        let view = this.clone();
        let pick = move |value: &&'static str, cx: &mut App| {
            let value = *value;
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(|sel| Some(frame::set_constraint(sel, axis, value)), cx);
            });
        };
        let tooltip = if axis == "horizontal" {
            "Horizontal constraint"
        } else {
            "Vertical constraint"
        };
        choice_menu(
            &format!("constraint-{axis}"),
            label,
            tooltip,
            Choices::new(
                move || options.iter().map(|(v, n)| (*v, (*n).into())).collect(),
                current,
                pick,
            ),
            false,
        )
    };
    let rows = vec![
        row()
            .child(menu("horizontal", &HORIZONTAL))
            .into_any_element(),
        row().child(menu("vertical", &VERTICAL)).into_any_element(),
    ];
    section("constraints", "Constraints", Vec::new(), rows, cx)
}

/// The Resizing section.
pub(super) fn render_resizing(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let field =
        |view: &mut BoardView, n: usize, window: &mut Window, cx: &mut Context<BoardView>| {
            let (key, label) = LIMITS[n];
            NumberField::new(key, label).render(view, sel, window, cx)
        };
    let mut rows = vec![
        row()
            .child(field(view, 0, window, cx))
            .child(field(view, 1, window, cx))
            .into_any_element(),
        row()
            .child(field(view, 2, window, cx))
            .child(field(view, 3, window, cx))
            .into_any_element(),
    ];
    if matches!(sel.get("layout"), Mixed::Same(Value::String(mode)) if mode == "grid") {
        rows.push(
            row()
                .child(NumberField::new("gridColumns", "Columns").render(view, sel, window, cx))
                .child(NumberField::new("gridRows", "Rows").render(view, sel, window, cx))
                .into_any_element(),
        );
    }
    let absolute = matches!(sel.get("absolute"), Mixed::Same(Value::Bool(true)));
    let switch = Switch::new("design-absolute")
        .small()
        .checked(absolute)
        .label("Ignore auto layout")
        .on_click(move |on, window, cx| {
            let on = *on;
            let _ = this.update(cx, |view, cx| {
                view.edit_layers(|sel| Some(sel.set("absolute", json!(on))), cx);
                window.focus(&view.focus, cx);
            });
        });
    rows.push(row().child(switch).into_any_element());
    rows.extend(mode_rows(view, sel, cx));
    section("resizing", "Resizing", Vec::new(), rows, cx)
}

/// One menu for each variable collection with more than one mode: the mode the frames show it
/// in, or Auto, the mode they inherit.
fn mode_rows(
    view: &BoardView,
    sel: &DesignSelection,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    use sawit_board::design::variables;
    let Some(doc) = view
        .board()
        .and_then(|b| b.session.ds().ok())
        .map(|ds| ds.doc.clone())
    else {
        return Vec::new();
    };
    let Some(first) = sel.ids.first().copied() else {
        return Vec::new();
    };
    let this = cx.entity().downgrade();
    variables::table(&doc)
        .into_iter()
        .filter(|c| c.modes.len() > 1)
        .map(|c| {
            let current = variables::mode_of(&doc, first, &c.id);
            let label = current
                .as_ref()
                .and_then(|m| c.modes.iter().find(|(id, _)| id == m))
                .map_or("Auto".to_string(), |(_, n)| n.clone());
            let modes = c.modes.clone();
            let (ids, collection) = (sel.ids.clone(), c.id.clone());
            let view = this.clone();
            let pick = move |mode: &Option<String>, cx: &mut App| {
                let e = variables::set_mode(&ids, &collection, mode.as_deref());
                let _ = view.update(cx, |view, cx| view.edit_layers(|_| Some(e), cx));
            };
            let items = move || {
                let mut items: Vec<(Option<String>, SharedString)> = vec![(None, "Auto".into())];
                items.extend(
                    modes
                        .iter()
                        .map(|(id, n)| (Some(id.clone()), n.clone().into())),
                );
                items
            };
            row()
                .items_center()
                .child(div().flex_1().text_xs().truncate().child(c.name.clone()))
                .child(choice_menu(
                    &format!("mode-{}", c.id),
                    label,
                    "Variable mode",
                    Choices::new(items, Some(current), pick),
                    false,
                ))
                .into_any_element()
        })
        .collect()
}

/// The Layout grid section.
pub(super) fn render_layout_grid(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let add = icon_button("design-grids-add", ADD_ICON, "Add layout grid")
        .debug_selector(|| "design-grids-add".into())
        .on_click(on_click_edit(&this, |sel| {
            Some(frame::add_grid(sel, "columns"))
        }))
        .into_any_element();
    let mut rows = Vec::new();
    let grids = frame::grids(sel).unwrap_or_default();
    for (index, grid) in grids.iter().enumerate().rev() {
        rows.extend(grid_rows(view, sel, index, grid, window, cx));
    }
    section("grids", "Layout grid", vec![add], rows, cx)
}

fn grid_rows(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    grid: &GridRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let id = format!("grids-{index}");
    let pick_menu = |field: &'static str,
                     options: &'static [(&'static str, &'static str)],
                     current: &str,
                     tooltip: &'static str| {
        let current = options.iter().map(|(v, _)| *v).find(|v| *v == current);
        let label = options
            .iter()
            .find(|(v, _)| Some(*v) == current)
            .map_or("", |(_, n)| *n);
        let view = this.clone();
        let pick = move |value: &&'static str, cx: &mut App| {
            let value = *value;
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(
                    |sel| Some(frame::set_grid(sel, index, field, json!(value))),
                    cx,
                );
            });
        };
        choice_menu(
            &format!("grids-{index}-{field}"),
            label,
            tooltip,
            Choices::new(
                move || options.iter().map(|(v, n)| (*v, (*n).into())).collect(),
                current,
                pick,
            ),
            false,
        )
    };
    let visible = grid.visible;
    let eye = eye_button(&id, visible, &this, move |sel| {
        Some(frame::set_grid(sel, index, "visible", json!(!visible)))
    });
    let remove = row_button(
        &id,
        "remove",
        REMOVE_ICON,
        "Remove layout grid",
        &this,
        move |sel| Some(frame::remove_grid(sel, index)),
    );
    let heading = row()
        .items_center()
        .child(pick_menu(
            "kind",
            &GRID_KINDS,
            &grid.kind,
            "Layout grid type",
        ))
        .child(eye)
        .child(remove);
    let number = |view: &mut BoardView,
                  field: &'static str,
                  label: &'static str,
                  value: f64,
                  window: &mut Window,
                  cx: &mut Context<BoardView>| {
        let rule = Rule::new(
            move |sel| {
                frame::grids(sel)
                    .and_then(|g| g.get(index).cloned())
                    .map_or_else(SharedString::default, |g| {
                        number_text(grid_value(&g, field))
                    })
            },
            move |sel, typed| {
                let n: f64 = typed.trim().parse().ok()?;
                let edit = frame::set_grid(sel, index, field, json!(n));
                (edit.command == "node.set" && edit.params["ids"] != json!([])).then_some(edit)
            },
        );
        h_flex()
            .flex_1()
            .gap_1()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                TextField::new(format!("{id}.{field}"), rule)
                    .showing(number_text(value))
                    .render(view, sel, window, cx),
            )
            .into_any_element()
    };
    let mut rows = vec![heading.into_any_element()];
    if grid.kind == "grid" {
        rows.push(
            row()
                .pl_4()
                .child(number(view, "size", "Size", grid.size, window, cx))
                .into_any_element(),
        );
    } else {
        rows.push(
            row()
                .pl_4()
                .child(number(view, "count", "Count", grid.count, window, cx))
                .child(number(view, "gutter", "Gutter", grid.gutter, window, cx))
                .into_any_element(),
        );
        rows.push(
            row()
                .pl_4()
                .child(pick_menu(
                    "align",
                    &GRID_ALIGNS,
                    &grid.align,
                    "Layout grid alignment",
                ))
                .child(number(view, "offset", "Offset", grid.offset, window, cx))
                .into_any_element(),
        );
        if grid.align != "stretch" {
            rows.push(
                row()
                    .pl_4()
                    .child(number(view, "size", "Width", grid.size, window, cx))
                    .into_any_element(),
            );
        }
    }
    rows
}

fn grid_value(grid: &GridRow, field: &str) -> f64 {
    match field {
        "size" => grid.size,
        "count" => grid.count,
        "gutter" => grid.gutter,
        _ => grid.offset,
    }
}

fn number_text(n: f64) -> SharedString {
    if n.fract() == 0.0 {
        format!("{}", n as i64).into()
    } else {
        format!("{n}").into()
    }
}
