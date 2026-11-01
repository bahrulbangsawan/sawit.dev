//! The keymap editor drawn: its toolbar, the table's header and rows (a row's context with its
//! operators in the accent color), and the binding form over it while it is open.

use std::ops::Range;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{CONTEXT, CreateBinding, EditInJson, KeymapEditor, ToggleKeystrokeSearch};
use crate::keymap::{OpenKeymapFile, Source};
use crate::states::{Banner, Placeholder};

const ROW_HEIGHT: f32 = 32.;

impl KeymapEditor {
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search: AnyElement = if self.keystroke_search {
            div()
                .flex_1()
                .child(self.keys_filter.clone())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .child(Input::new(&self.filter).cleanable(true))
                .into_any_element()
        };
        let can_write = self.store.read(cx).can_write();
        h_flex()
            .gap_1()
            .p_2()
            .child(search)
            .child(
                Button::new("keystroke-search")
                    .ghost()
                    .small()
                    .icon(Lucide::Keyboard)
                    .selected(self.keystroke_search)
                    .accessibility_label("Search by Keystroke")
                    .toggled(self.keystroke_search)
                    .tooltip_with_action(
                        "Search by Keystroke",
                        &ToggleKeystrokeSearch,
                        Some(CONTEXT),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_keystroke_search(&ToggleKeystrokeSearch, window, cx)
                    })),
            )
            .child(
                Button::new("show-unbound")
                    .ghost()
                    .small()
                    .icon(Lucide::ListFilter)
                    .selected(self.show_unbound)
                    .accessibility_label("Show Actions Without a Binding")
                    .toggled(self.show_unbound)
                    .tooltip(if self.show_unbound {
                        "Hide Actions Without a Binding"
                    } else {
                        "Show Actions Without a Binding"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_unbound(cx))),
            )
            .child(
                Button::new("edit-json")
                    .ghost()
                    .small()
                    .label("Edit in JSON")
                    .tooltip_with_action("Open keymap.json", &EditInJson, Some(CONTEXT))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenKeymapFile), cx)),
            )
            .child(
                Button::new("create-binding")
                    .outline()
                    .small()
                    .icon(Lucide::Plus)
                    .label("Create Keybinding")
                    .disabled(!can_write)
                    .tooltip_with_action("Create Keybinding", &CreateBinding, Some(CONTEXT))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.create(&CreateBinding, window, cx)),
                    ),
            )
    }

    fn render_header(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        h_flex()
            .h(px(ROW_HEIGHT))
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .text_sm()
            .font_semibold()
            .text_color(theme.muted_foreground)
            .child(div().w(px(28.)))
            .child(cell_action().child("Action"))
            .child(cell_args().child("Arguments"))
            .child(cell_keys().child("Keystrokes"))
            .child(cell_context().child("Context"))
            .child(cell_source().child("Source"))
            .child(div().w(px(56.)))
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let selected = self.selected == Some(ix);
        let removed = matches!(row.source, Source::Removed { .. });
        let unbound = row.source == Source::Unbound;
        let conflict = self.row_conflicts(row);
        let muted = theme.muted_foreground;
        let source = match row.source {
            Source::Default(_) => "Default",
            Source::User(_) => "User",
            Source::Removed { .. } => "Removed",
            Source::Unbound => "",
        };
        let (remove_icon, remove_tip) = match row.source {
            Source::Removed { .. } => (Lucide::RotateCcw, "Restore the Default"),
            Source::Default(_) => (Lucide::Trash, "Remove the Default Binding"),
            _ => (Lucide::Trash, "Remove the Binding"),
        };
        let group = SharedString::from(format!("keymap-row-{ix}"));
        h_flex()
            .id(("keymap-row", ix))
            .role(Role::Row)
            .aria_label(row.label.clone())
            .aria_description(row.display.clone())
            .aria_selected(selected)
            .group(group.clone())
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .text_sm()
            .border_b_1()
            .border_color(theme.border.opacity(0.4))
            .when(ix % 2 == 1, |el| el.bg(theme.table_even))
            .when(selected, |el| el.bg(theme.table_active))
            .hover(|el| el.bg(theme.table_hover))
            .when(removed || unbound, |el| el.text_color(muted))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.selected = Some(ix);
                window.focus(&this.focus, cx);
                if event.click_count() >= 2 {
                    this.edit(ix, window, cx);
                }
                cx.notify();
            }))
            .child(
                div().w(px(28.)).child(
                    div()
                        .invisible()
                        .group_hover(group.clone(), |el| el.visible())
                        .when(selected, |el| el.visible())
                        .child(
                            Button::new(("edit-binding", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Pencil)
                                .accessibility_label(if unbound {
                                    "Add a Keybinding"
                                } else {
                                    "Edit"
                                })
                                .tooltip(if unbound { "Add a Keybinding" } else { "Edit" })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.edit(ix, window, cx)
                                })),
                        ),
                ),
            )
            .child(
                cell_action()
                    .child(
                        div()
                            .id(("action-name", ix))
                            .truncate()
                            .when(removed, |el| el.line_through())
                            .child(row.label.clone())
                            .tooltip({
                                let name = row.action.clone();
                                move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                            }),
                    ),
            )
            .child(
                cell_args().child(
                    div()
                        .truncate()
                        .font_family("monospace")
                        .text_xs()
                        .text_color(muted)
                        .children(row.args.clone()),
                ),
            )
            .child(
                cell_keys()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .font_family("monospace")
                            .when(removed, |el| el.line_through())
                            .child(row.display.clone()),
                    )
                    .when(conflict, |el| {
                        el.child(
                            div()
                                .id(("conflict", ix))
                                .child(
                                    Icon::new(Lucide::TriangleAlert)
                                        .xsmall()
                                        .text_color(theme.warning),
                                )
                                .tooltip(|window, cx| {
                                    Tooltip::new(
                                        "Another binding has these keys in this context: only one runs",
                                    )
                                    .build(window, cx)
                                }),
                        )
                    }),
            )
            .child(cell_context().child(render_context(row.context.as_ref(), unbound, cx)))
            .child(cell_source().text_color(muted).child(source))
            .child(
                h_flex()
                    .w(px(56.))
                    .justify_end()
                    .invisible()
                    .group_hover(group, |el| el.visible())
                    .when(selected, |el| el.visible())
                    .when(!unbound, |el| {
                        el.child(
                            Button::new(("remove-binding", ix))
                                .ghost()
                                .xsmall()
                                .icon(remove_icon)
                                .accessibility_label(remove_tip)
                                .tooltip(remove_tip)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove(ix, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for KeymapEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let errors: Vec<String> = store.errors().map(String::from).collect();
        let count = self.rows.len();
        let theme = cx.theme();
        v_flex()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(theme.background)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::edit_selected))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(Self::create))
            .on_action(cx.listener(Self::toggle_keystroke_search))
            .on_action(cx.listener(Self::focus_filter))
            .on_action(|_: &EditInJson, window, cx| {
                window.dispatch_action(Box::new(OpenKeymapFile), cx)
            })
            .child(self.render_toolbar(cx))
            .children(errors.into_iter().enumerate().map(|(ix, error)| {
                div()
                    .px_2()
                    .pb_1()
                    .child(Banner::error(("keymap-error", ix), error))
            }))
            .children(self.error.clone().map(|error| {
                div()
                    .px_2()
                    .pb_1()
                    .child(Banner::error("keymap-action-error", error))
            }))
            .child(self.render_header(cx))
            .child(
                div()
                    .id("keymap-table")
                    .role(Role::Table)
                    .aria_label("Keybindings")
                    .flex_1()
                    .min_h_0()
                    .when(count > 0, |el| {
                        el.child(
                            uniform_list(
                                "keymap-rows",
                                count,
                                cx.processor(move |this, range: Range<usize>, _, cx| {
                                    range.map(|ix| this.render_row(ix, cx)).collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .when(count == 0, |el| {
                        el.child(
                            Placeholder::new(Lucide::Keyboard, "No bindings match")
                                .detail(if self.keystroke_search {
                                    "Nothing is bound to these keys. Press other keys, or turn keystroke search off."
                                } else {
                                    "Try another name, or show the actions without a binding."
                                }),
                        )
                    }),
            )
            .children(self.form.clone().map(|form| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_start()
                    .justify_center()
                    .pt(px(72.))
                    .bg(gpui_kit::black().opacity(0.35))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                    )
                    .child(form)
            }))
    }
}

fn cell_action() -> Div {
    h_flex().flex_1().min_w_0().pr_3()
}

fn cell_args() -> Div {
    h_flex().w(px(180.)).flex_shrink_0().min_w_0().pr_3()
}

fn cell_keys() -> Div {
    h_flex().w(px(150.)).flex_shrink_0().min_w_0().pr_3()
}

fn cell_context() -> Div {
    h_flex().flex_1().min_w_0().pr_3()
}

fn cell_source() -> Div {
    h_flex().w(px(80.)).flex_shrink_0()
}

/// A context predicate with its operators in the accent color: `AcpThread > Editor && mode == full`.
fn render_context(context: Option<&SharedString>, muted: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let Some(context) = context else {
        return div()
            .text_color(theme.muted_foreground)
            .child("(anywhere)")
            .into_any_element();
    };
    let mut parts: Vec<AnyElement> = Vec::new();
    for token in tokenize_context(context) {
        let operator = matches!(token, "&&" | "||" | "!" | ">" | "==" | "!=" | "(" | ")");
        parts.push(
            div()
                .when(operator && !muted, |el| el.text_color(theme.link))
                .child(SharedString::from(token.to_string()))
                .into_any_element(),
        );
    }
    h_flex()
        .gap_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .children(parts)
        .into_any_element()
}

/// The words and operators of a context predicate, in order.
pub(super) fn tokenize_context(context: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = context.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        let c = bytes[ix];
        if c.is_ascii_whitespace() {
            ix += 1;
            continue;
        }
        let two = context.get(ix..ix + 2);
        let len = if matches!(two, Some("&&" | "||" | "==" | "!=")) {
            2
        } else if matches!(c, b'!' | b'>' | b'(' | b')') {
            1
        } else {
            let mut end = ix;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b'!' | b'>' | b'(' | b')' | b'&' | b'|' | b'=')
            {
                end += 1;
            }
            (end - ix).max(1)
        };
        tokens.push(&context[ix..ix + len]);
        ix += len;
    }
    tokens
}
