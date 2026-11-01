//! Center pane for the selected instance: controls, port-conflict warning,
//! live log, and the "new instance" form.

mod form;
mod instance;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::*;

use super::store::ServerStore;

pub struct ServerDetail {
    store: Entity<ServerStore>,
    name_input: Entity<InputState>,
    port_input: Entity<InputState>,
    /// Index into `choices()`.
    choice: usize,
    form_error: Option<String>,
    form_busy: bool,
    log_scroll: UniformListScrollHandle,
    log_len: usize,
    was_creating: bool,
}

impl ServerDetail {
    pub fn new(store: Entity<ServerStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_in(&store, window, |this, store, window, cx| {
            let creating = store.read(cx).creating;
            if creating && !this.was_creating {
                this.reset_form(window, cx);
            }
            this.was_creating = creating;
            let len = store.read(cx).log.len();
            if len != this.log_len {
                this.log_len = len;
                if len > 0 {
                    this.log_scroll
                        .scroll_to_item(len - 1, ScrollStrategy::Bottom);
                }
            }
            cx.notify();
        })
        .detach();
        let mut this = Self {
            store,
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("project-name")),
            port_input: cx.new(|cx| InputState::new(window, cx).placeholder("5432")),
            choice: 0,
            form_error: None,
            form_busy: false,
            log_scroll: UniformListScrollHandle::new(),
            log_len: 0,
            was_creating: false,
        };
        if this.store.read(cx).registry.instances.is_empty() {
            this.reset_form(window, cx);
        }
        this
    }
}

fn field(label: &'static str, child: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(div().text_xs().font_semibold().child(label))
        .child(child)
}

fn error_box(message: String, cx: &App) -> impl IntoElement {
    div()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().danger)
        .text_sm()
        .text_color(cx.theme().danger)
        .whitespace_normal()
        .child(message)
}

impl Render for ServerDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let content = if store.creating || store.registry.instances.is_empty() {
            // Scrolls, so a short window still reaches Create Instance.
            div()
                .id("server-form")
                .size_full()
                .overflow_y_scroll()
                .child(self.render_form(cx))
                .into_any_element()
        } else if let Some(inst) = store.selected_instance().cloned() {
            self.render_instance(&inst, cx).into_any_element()
        } else {
            div()
                .p_6()
                .text_color(cx.theme().muted_foreground)
                .child("Select an instance in the sidebar.")
                .into_any_element()
        };
        div().size_full().bg(cx.theme().background).child(content)
    }
}
