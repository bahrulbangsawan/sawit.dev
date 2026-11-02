//! The binding form drawn: its fields, the action names a new binding's action matches, the
//! contexts to pick from, the notes on what the keys clash with, and Cancel and Save.

use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BindingForm, FormTarget, known_contexts};
use crate::keymap::names;
use crate::states::Banner;

impl Render for BindingForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let notes = self.clash_notes(cx);
        let label = |text: &'static str| {
            div()
                .text_xs()
                .font_semibold()
                .text_color(muted)
                .child(text)
        };
        let known_contexts = known_contexts(cx);
        let new = self.is_new();
        v_flex()
            .id("binding-form")
            .track_focus(&self.focus)
            .w(px(560.))
            .max_w_full()
            .gap_3()
            .p_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            // A click inside doesn't close the form.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    this.cancel(window, cx);
                }
            }))
            .child(div().text_base().font_semibold().child(self.title()))
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Action"))
                    .map(|el| {
                        if new {
                            el.child(Input::new(&self.action).small())
                                .children(self.suggestions.iter().copied().enumerate().map(
                                    |(ix, name)| {
                                        h_flex()
                                            .id(("suggestion", ix))
                                            .px_2()
                                            .py_1()
                                            .gap_2()
                                            .rounded(theme.radius)
                                            .cursor_pointer()
                                            .hover(|el| el.bg(theme.list_hover))
                                            .text_sm()
                                            .child(names::humanize_action(name))
                                            .child(div().text_xs().text_color(muted).child(name))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.pick(name, window, cx)
                                            }))
                                    },
                                ))
                        } else {
                            let FormTarget::Row(row) = &self.target else {
                                return el;
                            };
                            el.child(
                                h_flex()
                                    .gap_2()
                                    .text_sm()
                                    .child(row.label.clone())
                                    .child(div().text_xs().text_color(muted).child(row.action.clone())),
                            )
                        }
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Keystrokes"))
                    .child(self.keys.clone())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Every key is recorded while the field is focused, up to 3 for a chord (⌘K ⌘S). ↩ saves; ⎋ stops recording, and a second ⎋ cancels."),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Context"))
                    .child(Input::new(&self.context).small())
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .children(known_contexts.into_iter().take(10).enumerate().map(
                                |(ix, context)| {
                                    Button::new(("context", ix))
                                        .ghost()
                                        .xsmall()
                                        .label(context.clone())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.context.update(cx, |input, cx| {
                                                input.set_value(context.clone(), window, cx)
                                            });
                                            cx.notify();
                                        }))
                                },
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Arguments"))
                    .child(Input::new(&self.args).small()),
            )
            .children(
                notes
                    .into_iter()
                    .map(|(id, note)| Banner::info(id, note)),
            )
            .children(
                self.error
                    .clone()
                    .map(|error| Banner::error("binding-error", error)),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-binding")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .child(
                        Button::new("save-binding")
                            .primary()
                            .small()
                            .label("Save")
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
