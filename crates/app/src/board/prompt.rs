//! The dialogs of the commands that need a value (`sawit_board::prompts`): a link's URL, an
//! offset, a page, the new names, a colour profile and the nudge amounts. Running such a command
//! from the menus, the Actions palette or its key opens its dialog over the canvas, filled in
//! with what the document holds; Enter or its button runs the command with what was typed, on the
//! layers selected when it opened, and Escape, Cancel or a press outside it closes it.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::prompts::{self, FieldKind, Prompt};

use super::BoardView;

const WIDTH: f32 = 420.;
const TOP: f32 = 72.;

/// A dialog while it is open.
pub(super) struct PromptUi {
    prompt: Prompt,
    /// Each field's text input, for a text or number field.
    inputs: Vec<Option<Entity<InputState>>>,
    /// Each choice's and toggle's value.
    picked: Vec<String>,
    /// The layers selected when it opened.
    ids: Vec<NodeId>,
    /// Why what was typed won't do, under the fields.
    error: Option<String>,
    focus: FocusHandle,
    _events: Vec<Subscription>,
}

impl BoardView {
    /// Opens command `id`'s dialog, or says why it has none (no other page to move to).
    pub(super) fn open_prompt(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(board) = self.board() else {
            return;
        };
        let ids = board.session.selection().to_vec();
        let Some(prompt) = prompts::prompt(&board.session, id) else {
            if id == "object.moveToPage" {
                self.show_toast("Add another page to move layers to", cx);
            }
            return;
        };
        let mut inputs = Vec::new();
        let mut picked = Vec::new();
        let mut events = Vec::new();
        let mut first = None;
        for field in &prompt.fields {
            match field.kind {
                FieldKind::Text { .. } | FieldKind::Number { .. } => {
                    let (value, hint) = (field.value.clone(), field.placeholder);
                    let input = cx.new(|cx| {
                        let mut input = InputState::new(window, cx).placeholder(hint);
                        input.set_value(value, window, cx);
                        input
                    });
                    events.push(cx.subscribe_in(
                        &input,
                        window,
                        |view, _, event: &InputEvent, window, cx| {
                            if let InputEvent::PressEnter { .. } = event {
                                view.submit_prompt(window, cx);
                            }
                        },
                    ));
                    first.get_or_insert_with(|| input.clone());
                    inputs.push(Some(input));
                    picked.push(String::new());
                }
                FieldKind::Choice(_) | FieldKind::Toggle => {
                    inputs.push(None);
                    picked.push(field.value.clone());
                }
            }
        }
        let focus = cx.focus_handle();
        match &first {
            Some(input) => input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            }),
            None => window.focus(&focus, cx),
        }
        self.close_menus(cx);
        self.prompt_ui = Some(PromptUi {
            prompt,
            inputs,
            picked,
            ids,
            error: None,
            focus,
            _events: events,
        });
        cx.notify();
    }

    /// Closes the dialog, and gives the canvas the keyboard back.
    pub(super) fn close_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_ui.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Runs the dialog's command with what was typed, or says why not under its fields.
    fn submit_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.prompt_ui.as_mut() else {
            return;
        };
        let values: Vec<String> = (ui.inputs.iter().zip(&ui.picked))
            .map(|(input, picked)| match input {
                Some(input) => input.read(cx).value().to_string(),
                None => picked.clone(),
            })
            .collect();
        match prompts::params(&ui.prompt, &values, &ui.ids) {
            Ok(params) => {
                let command = ui.prompt.command;
                self.close_prompt(window, cx);
                self.choose(command, params, cx);
                // A version changes the file's history, not the design: written now, since the
                // autosave follows the design.
                if command == "file.saveVersion" {
                    self.saved = None;
                    match self.save_now() {
                        Ok(_) => self.show_toast("Version saved", cx),
                        Err(why) => self.show_toast(why, cx),
                    }
                }
            }
            Err(why) => {
                ui.error = Some(why);
                cx.notify();
            }
        }
    }
}

/// The dialog, while one is open.
pub(super) fn render_prompt(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let ui = view.prompt_ui.as_ref()?;
    let theme = cx.theme().clone();
    let fields = (ui.prompt.fields.iter().enumerate()).map(|(index, field)| {
        let label = div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(field.label);
        let control = match (&field.kind, &ui.inputs[index]) {
            (FieldKind::Text { .. } | FieldKind::Number { .. }, Some(input)) => {
                Input::new(input).small().into_any_element()
            }
            (FieldKind::Choice(options), _) => {
                let chosen = ui.picked[index].clone();
                let buttons = options.iter().enumerate().map(|(n, (value, name))| {
                    let value = value.clone();
                    Button::new(("prompt-choice", index * 1000 + n))
                        .small()
                        .label(name.clone())
                        .selected(value == chosen)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            if let Some(ui) = view.prompt_ui.as_mut() {
                                ui.picked[index] = value.clone();
                                cx.notify();
                            }
                        }))
                });
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(buttons)
                    .into_any_element()
            }
            (FieldKind::Toggle, _) => {
                return Checkbox::new(("prompt-toggle", index))
                    .label(field.label)
                    .checked(ui.picked[index] == "true")
                    .on_click(cx.listener(move |view, ticked: &bool, _, cx| {
                        if let Some(ui) = view.prompt_ui.as_mut() {
                            ui.picked[index] = ticked.to_string();
                            cx.notify();
                        }
                    }))
                    .into_any_element();
            }
            _ => div().into_any_element(),
        };
        v_flex()
            .gap_1()
            .child(label)
            .child(control)
            .into_any_element()
    });
    let error = ui
        .error
        .clone()
        .map(|why| div().text_sm().text_color(theme.danger).child(why));
    let footer = h_flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("prompt-cancel")
                .ghost()
                .small()
                .label("Cancel")
                .on_click(cx.listener(|view, _, window, cx| view.close_prompt(window, cx))),
        )
        .child(
            Button::new("prompt-submit")
                .primary()
                .small()
                .label(ui.prompt.submit)
                .on_click(cx.listener(|view, _, window, cx| view.submit_prompt(window, cx))),
        );
    Some(
        v_flex()
            .id("prompt-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .items_center()
            .pt(px(TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.close_prompt(window, cx)),
            )
            .child(
                v_flex()
                    .id("prompt-dialog")
                    .debug_selector(|| "prompt-dialog".into())
                    .track_focus(&ui.focus)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "escape" => view.close_prompt(window, cx),
                            "enter" => view.submit_prompt(window, cx),
                            _ => return,
                        }
                        cx.stop_propagation();
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(WIDTH))
                    .max_w_full()
                    .p_4()
                    .gap_3()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(ui.prompt.title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(ui.prompt.note),
                    )
                    .children(fields)
                    .children(error)
                    .child(footer),
            )
            .into_any_element(),
    )
}
