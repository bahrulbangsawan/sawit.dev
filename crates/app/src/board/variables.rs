//! The Variables window (View ▸ Variables…, `sawit_board::design::variables`): the file's
//! collections down the side, and the one picked as a table, its modes across and its variables
//! down, each cell the variable's value in that mode. A click on a name or a value types in its
//! place, and Enter or a click elsewhere keeps it; Escape drops it. Buttons add a collection, a
//! mode, and a variable of each type, and delete a variable, a mode or a collection.

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::variables::{self, CollectionRow, TYPES};

use super::BoardView;

const WIDTH: f32 = 760.;
const TOP: f32 = 56.;
const NAME_WIDTH: f32 = 180.;
const CELL_WIDTH: f32 = 150.;

/// What a click types in.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Collection(String),
    Mode {
        collection: String,
        mode: String,
    },
    Name(String),
    Value {
        variable: String,
        kind: &'static str,
        mode: String,
    },
}

/// The window, while it is open.
pub(super) struct VariablesUi {
    /// The collection shown, by id.
    collection: Option<String>,
    editing: Option<(Cell, Entity<InputState>, Subscription)>,
    focus: FocusHandle,
}

impl BoardView {
    /// Opens the Variables window, or closes it when it is open.
    pub(super) fn toggle_variables(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.variables_ui.take().is_some() {
            window.focus(&self.focus, cx);
        } else {
            self.close_menus(cx);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            self.variables_ui = Some(VariablesUi {
                collection: None,
                editing: None,
                focus,
            });
        }
        cx.notify();
    }

    fn run_variable_edit(&mut self, edit: sawit_board::design::Edit, cx: &mut Context<Self>) {
        self.choose(edit.command, edit.params, cx);
    }

    /// Starts typing in `cell`, `text` its value now.
    fn edit_cell(&mut self, cell: Cell, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(text, window, cx);
            input
        });
        let events =
            cx.subscribe_in(
                &input,
                window,
                |view, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        view.commit_cell(window, cx)
                    }
                    _ => {}
                },
            );
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        if let Some(ui) = self.variables_ui.as_mut() {
            ui.editing = Some((cell, input, events));
        }
        cx.notify();
    }

    /// Keeps what was typed in the cell being edited.
    fn commit_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((cell, input, _)) = self.variables_ui.as_mut().and_then(|ui| ui.editing.take())
        else {
            return;
        };
        let typed = input.read(cx).value().trim().to_string();
        if let Some(ui) = self.variables_ui.as_ref() {
            window.focus(&ui.focus, cx);
        }
        if typed.is_empty() {
            cx.notify();
            return;
        }
        let edit = match cell {
            Cell::Collection(id) => Some(variables::rename_collection(&id, &typed)),
            Cell::Mode { collection, mode } => {
                Some(variables::rename_mode(&collection, &mode, &typed))
            }
            Cell::Name(id) => Some(variables::rename_variable(&id, &typed)),
            Cell::Value {
                variable,
                kind,
                mode,
            } => {
                let edit = variables::set_value(&variable, kind, &mode, &typed);
                if edit.is_none() {
                    self.show_toast(format!("“{typed}” isn't a {kind} value"), cx);
                }
                edit
            }
        };
        if let Some(edit) = edit {
            self.run_variable_edit(edit, cx);
        }
        cx.notify();
    }
}

/// The window, while it is open.
pub(super) fn render_variables(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let ui = view.variables_ui.as_ref()?;
    let doc = view.board()?.session.ds().ok()?.doc.clone();
    let table = variables::table(&doc);
    let shown = (ui.collection.as_ref())
        .and_then(|id| table.iter().find(|c| &c.id == id))
        .or_else(|| table.first())
        .cloned();
    let editing = ui
        .editing
        .as_ref()
        .map(|(cell, input, _)| (cell.clone(), input.clone()));
    let theme = cx.theme().clone();
    let taken: Vec<String> = table.iter().map(|c| c.name.clone()).collect();
    let new_collection = variables::fresh_name("Collection", &taken);
    let side = v_flex()
        .w(px(170.))
        .flex_shrink_0()
        .gap_1()
        .pr_3()
        .border_r_1()
        .border_color(theme.border)
        .children(table.iter().map(|c| {
            let (id, name) = (c.id.clone(), c.name.clone());
            let chosen = shown.as_ref().is_some_and(|s| s.id == c.id);
            let cell = Cell::Collection(c.id.clone());
            if let Some((_, input)) = editing.as_ref().filter(|(e, _)| *e == cell) {
                return Input::new(input).small().into_any_element();
            }
            Button::new(SharedString::from(format!("variables-collection-{id}")))
                .ghost()
                .small()
                .w_full()
                .selected(chosen)
                .label(name.clone())
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    if event.click_count() > 1 {
                        view.edit_cell(Cell::Collection(id.clone()), name.clone(), window, cx);
                    } else if let Some(ui) = view.variables_ui.as_mut() {
                        ui.collection = Some(id.clone());
                        cx.notify();
                    }
                }))
                .into_any_element()
        }))
        .child(
            Button::new("variables-add-collection")
                .ghost()
                .small()
                .label("+ Collection")
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.run_variable_edit(variables::create_collection(&new_collection), cx);
                })),
        );
    let body = match &shown {
        Some(collection) => grid(collection, editing.as_ref(), cx).into_any_element(),
        None => div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child("No variables yet. Add a collection to start.")
            .into_any_element(),
    };
    Some(
        v_flex()
            .id("variables-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .items_center()
            .pt(px(TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.toggle_variables(window, cx)),
            )
            .child(
                v_flex()
                    .id("variables-window")
                    .debug_selector(|| "variables-window".into())
                    .track_focus(&ui.focus)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            let typing =
                                view.variables_ui.as_mut().and_then(|ui| ui.editing.take());
                            if typing.is_none() {
                                view.toggle_variables(window, cx);
                            }
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(WIDTH))
                    .max_w_full()
                    .max_h(px(560.))
                    .p_4()
                    .gap_3()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Variables"),
                            )
                            .child(
                                Button::new("variables-close")
                                    .ghost()
                                    .small()
                                    .label("Close")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.toggle_variables(window, cx)
                                    })),
                            ),
                    )
                    .child(h_flex().items_start().gap_3().child(side).child(body)),
            )
            .into_any_element(),
    )
}

/// The table of `collection`: its modes across, its variables down.
fn grid(
    collection: &CollectionRow,
    editing: Option<&(Cell, Entity<InputState>)>,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let theme = cx.theme().clone();
    let cid = collection.id.clone();
    let cell = |cell: Cell, text: String, id: String, cx: &mut Context<BoardView>| -> AnyElement {
        if let Some((_, input)) = editing.filter(|(e, _)| *e == cell) {
            return div()
                .w(px(CELL_WIDTH))
                .child(Input::new(input).small())
                .into_any_element();
        }
        let shown = text.clone();
        div()
            .id(SharedString::from(id))
            .w(px(CELL_WIDTH))
            .px_2()
            .py_1()
            .rounded(px(4.))
            .truncate()
            .hover(|s| s.bg(cx.theme().secondary))
            .cursor_text()
            .child(if shown.is_empty() {
                "—".to_string()
            } else {
                shown
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                view.edit_cell(cell.clone(), text.clone(), window, cx);
            }))
            .into_any_element()
    };
    let mut header = h_flex()
        .gap_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(div().w(px(NAME_WIDTH)).px_2().child("Name"));
    let removable_mode = collection.modes.len() > 1;
    for (mode, name) in &collection.modes {
        let c = Cell::Mode {
            collection: cid.clone(),
            mode: mode.clone(),
        };
        let mut head = h_flex().w(px(CELL_WIDTH)).items_center().child(cell(
            c,
            name.clone(),
            format!("variables-mode-{mode}"),
            cx,
        ));
        if removable_mode {
            let (coll, mode) = (cid.clone(), mode.clone());
            head = head.child(
                Button::new(SharedString::from(format!("variables-remove-mode-{mode}")))
                    .ghost()
                    .xsmall()
                    .label("×")
                    .tooltip("Delete mode")
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.run_variable_edit(variables::remove_mode(&coll, &mode), cx);
                    })),
            );
        }
        header = header.child(head);
    }
    let taken: Vec<String> = collection.modes.iter().map(|m| m.1.clone()).collect();
    let new_mode = variables::fresh_name("Mode", &taken);
    let coll = cid.clone();
    header = header.child(
        Button::new("variables-add-mode")
            .ghost()
            .xsmall()
            .label("+ Mode")
            .on_click(cx.listener(move |view, _, _, cx| {
                view.run_variable_edit(variables::add_mode(&coll, &new_mode), cx);
            })),
    );
    let mut rows = Vec::new();
    for var in &collection.variables {
        let mut line = h_flex()
            .gap_1()
            .items_center()
            .child(div().w(px(NAME_WIDTH)).child(cell(
                Cell::Name(var.id.clone()),
                var.name.clone(),
                format!("variables-name-{}", var.id),
                cx,
            )));
        for ((mode, _), value) in collection.modes.iter().zip(&var.values) {
            line = line.child(cell(
                Cell::Value {
                    variable: var.id.clone(),
                    kind: var.kind,
                    mode: mode.clone(),
                },
                value.clone(),
                format!("variables-value-{}-{mode}", var.id),
                cx,
            ));
        }
        let id = var.id.clone();
        line = line.child(
            Button::new(SharedString::from(format!("variables-delete-{id}")))
                .ghost()
                .xsmall()
                .label("Delete")
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.run_variable_edit(variables::delete_variable(&id), cx);
                })),
        );
        rows.push(line.into_any_element());
    }
    let taken: Vec<String> = collection
        .variables
        .iter()
        .map(|v| v.name.clone())
        .collect();
    let adds = TYPES.iter().map(|(kind, label)| {
        let kind: &'static str = kind;
        let name = variables::fresh_name(kind, &taken);
        let coll = cid.clone();
        Button::new(SharedString::from(format!("variables-add-{kind}")))
            .ghost()
            .xsmall()
            .label(format!("+ {label}"))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.run_variable_edit(variables::create_variable(&coll, &name, kind), cx);
            }))
    });
    let coll = cid.clone();
    v_flex()
        .id("variables-grid")
        .flex_1()
        .min_w_0()
        .gap_1()
        .overflow_y_scroll()
        .child(header)
        .children(rows)
        .child(
            h_flex().gap_1().flex_wrap().children(adds).child(
                Button::new("variables-delete-collection")
                    .ghost()
                    .xsmall()
                    .label("Delete collection")
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.run_variable_edit(variables::delete_collection(&coll), cx);
                    })),
            ),
        )
}
