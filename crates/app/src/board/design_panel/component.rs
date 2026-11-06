//! The Component section (`sawit_board::design::component`), for the one selected main
//! component, set or instance:
//!
//! - **An instance**: its main's name, with Go to main, Reset and Detach; then each property with
//!   its value: a menu of a variant's options, a switch for a boolean, a field for a text, a menu
//!   of the components a nested instance swaps to; and a Swap menu for the instance itself.
//! - **A main or a set**: each property with its default and a button that deletes it, its name
//!   a field that renames it; an add menu (Boolean, Text, Instance swap, Variant) and Add variant.

use std::rc::Rc;

use gpui_kit::component::button::*;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::component::{
    self, ComponentPanel, NEW_PROPERTY_TYPES, Prop, PropKind, Role,
};
use sawit_board::design::{DesignSelection, Edit};
use serde_json::{Value, json};

use super::controls::{Choices, choice_menu, icon_button, row, row_button, section};
use super::number_field::{Rule, TextField};
use super::paints::{ADD_ICON, REMOVE_ICON};
use crate::board::BoardView;

/// The Component section.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let Some(panel) = sel.component.clone() else {
        return div().into_any_element();
    };
    let this = cx.entity().downgrade();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut actions: Vec<AnyElement> = Vec::new();
    let instance = matches!(panel.role, Role::Instance { .. });
    match &panel.role {
        Role::Instance { main_name, .. } => {
            let command = |id: &'static str, label: &'static str, command: &'static str| {
                let view = this.clone();
                Button::new(SharedString::from(format!("design-component-{id}")))
                    .ghost()
                    .small()
                    .label(label)
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.choose(command, json!({}), cx));
                    })
            };
            rows.push(
                row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(main_name.clone()),
                    )
                    .child(command("main", "Go to main", "component.goToMain"))
                    .into_any_element(),
            );
            rows.push(
                row()
                    .child(command("reset", "Reset", "component.resetInstance"))
                    .child(command("detach", "Detach", "component.detach"))
                    .child(swap_menu(&panel, &this))
                    .into_any_element(),
            );
        }
        Role::Main { .. } | Role::Set => {
            actions.push(add_property_menu(&panel, &this));
            let id = panel.id;
            actions.push(
                icon_button("design-component-add-variant", ADD_ICON, "Add variant")
                    .debug_selector(|| "design-component-add-variant".into())
                    .on_click({
                        let view = this.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.edit_layers(|_| Some(component::add_variant(id)), cx);
                            });
                        }
                    })
                    .into_any_element(),
            );
        }
    }
    for (index, prop) in panel.props.iter().enumerate() {
        rows.push(prop_row(
            view, sel, &panel, index, prop, instance, window, cx,
        ));
    }
    section("component", "Component", actions, rows, cx)
}

/// The menu that adds a property of a type, named `Property n`.
fn add_property_menu(panel: &ComponentPanel, view: &WeakEntity<BoardView>) -> AnyElement {
    let (id, name) = (panel.id, component::new_property_name(panel));
    let view = view.clone();
    let pick = move |kind: &&'static str, cx: &mut App| {
        let (kind, name) = (*kind, name.clone());
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|_| Some(component::add_property(id, &name, kind)), cx);
        });
    };
    div()
        .w(px(120.))
        .child(choice_menu(
            "component-add-property",
            "Add property",
            "Add a component property",
            Choices::new(
                || {
                    NEW_PROPERTY_TYPES
                        .iter()
                        .map(|(k, n)| (*k, (*n).into()))
                        .collect()
                },
                None,
                pick,
            ),
            false,
        ))
        .into_any_element()
}

/// The instance's Swap menu: every component it can be an instance of instead.
fn swap_menu(panel: &ComponentPanel, view: &WeakEntity<BoardView>) -> AnyElement {
    let id = panel.id;
    let current = match panel.role {
        Role::Instance { main, .. } => Some(main),
        _ => None,
    };
    let view = view.clone();
    let pick = move |target: &figcraft_doc::NodeId, cx: &mut App| {
        let target = *target;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|_| Some(component::swap(id, target)), cx);
        });
    };
    let targets = view_targets(panel);
    choice_menu(
        "component-swap",
        "Swap",
        "Swap instance",
        Choices::new(move || targets.clone(), current, pick),
        false,
    )
    .into_any_element()
}

fn view_targets(panel: &ComponentPanel) -> Vec<(figcraft_doc::NodeId, SharedString)> {
    let found = panel.props.iter().find_map(|p| match &p.kind {
        PropKind::InstanceSwap(options) => Some(options.clone()),
        _ => None,
    });
    found
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name)| (id, name.into()))
        .collect()
}

/// One property: its name (a field that renames it, on a main) and its value or default.
#[allow(clippy::too_many_arguments)]
fn prop_row(
    view: &mut BoardView,
    sel: &DesignSelection,
    panel: &ComponentPanel,
    index: usize,
    prop: &Prop,
    instance: bool,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let id = panel.id;
    let name = prop.name.clone();
    let variant = matches!(prop.kind, PropKind::Variant(_));
    // How a value goes in: an instance's value, or a main's default.
    let edit = move |name: &str, value: Value| -> Edit {
        if instance {
            component::set_value(id, name, value)
        } else {
            component::set_default(id, name, value)
        }
    };
    let label: AnyElement = if instance || variant {
        div()
            .w(px(96.))
            .flex_shrink_0()
            .text_xs()
            .truncate()
            .child(prop.name.clone())
            .into_any_element()
    } else {
        let old = name.clone();
        let rule = Rule::new(
            {
                let old = old.clone();
                move |_| old.clone().into()
            },
            move |_, typed| {
                let typed = typed.trim();
                (!typed.is_empty() && typed != old)
                    .then(|| component::rename_property(id, &old, typed))
            },
        );
        div()
            .w(px(96.))
            .flex_shrink_0()
            .child(
                TextField::new(format!("component.{index}.name"), rule)
                    .showing(prop.name.clone().into())
                    .render(view, sel, window, cx),
            )
            .into_any_element()
    };
    let control: AnyElement = match &prop.kind {
        PropKind::Boolean => {
            let on = prop.value.as_bool().unwrap_or(false);
            let view = this.clone();
            let name = name.clone();
            Switch::new(SharedString::from(format!("design-component-{index}")))
                .small()
                .checked(on)
                .on_click(move |on, window, cx| {
                    let e = edit(&name, json!(*on));
                    let _ = view.update(cx, |view, cx| {
                        view.edit_layers(|_| Some(e), cx);
                        window.focus(&view.focus, cx);
                    });
                })
                .into_any_element()
        }
        PropKind::Text => {
            let shown = prop.value.as_str().unwrap_or_default().to_string();
            let name = name.clone();
            let rule = Rule::new(
                {
                    let shown = shown.clone();
                    move |_| shown.clone().into()
                },
                move |_, typed| Some(edit(&name, json!(typed))),
            );
            TextField::new(format!("component.{index}.value"), rule)
                .showing(shown.into())
                .render(view, sel, window, cx)
        }
        PropKind::Variant(options) => {
            let current = prop.value.as_str().map(str::to_string);
            let label = current.clone().unwrap_or_default();
            let options = options.clone();
            let view = this.clone();
            let name = name.clone();
            let pick = move |value: &String, cx: &mut App| {
                let e = edit(&name, json!(value));
                let _ = view.update(cx, |view, cx| view.edit_layers(|_| Some(e), cx));
            };
            choice_menu(
                &format!("component-{index}-variant"),
                label,
                "Variant",
                Choices::new(
                    move || {
                        options
                            .iter()
                            .map(|o| (o.clone(), o.clone().into()))
                            .collect()
                    },
                    current,
                    pick,
                ),
                false,
            )
            .into_any_element()
        }
        PropKind::InstanceSwap(targets) => {
            let current = figcraft_engine::params::id_of(&prop.value);
            let label = targets
                .iter()
                .find(|(t, _)| Some(*t) == current)
                .map_or("Component".to_string(), |(_, n)| n.clone());
            let targets = targets.clone();
            let view = this.clone();
            let name = name.clone();
            let pick = move |target: &figcraft_doc::NodeId, cx: &mut App| {
                let e = edit(&name, json!(target));
                let _ = view.update(cx, |view, cx| view.edit_layers(|_| Some(e), cx));
            };
            choice_menu(
                &format!("component-{index}-swap"),
                label,
                "Instance swap",
                Choices::new(
                    move || {
                        targets
                            .iter()
                            .map(|(t, n)| (*t, n.clone().into()))
                            .collect()
                    },
                    current,
                    pick,
                ),
                false,
            )
            .into_any_element()
        }
        PropKind::Slot => div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Slot")
            .into_any_element(),
    };
    let mut line = row().items_center().child(label).child(control);
    if !instance && !variant {
        let name = name.clone();
        line = line.child(row_button(
            &format!("component-{index}"),
            "delete",
            REMOVE_ICON,
            "Delete property",
            &this,
            move |_| Some(component::delete_property(id, &name)),
        ));
    }
    line.into_any_element()
}
