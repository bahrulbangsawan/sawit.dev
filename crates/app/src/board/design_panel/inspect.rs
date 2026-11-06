//! Dev Mode's Inspect panel, in the Design panel's place while Dev Mode is on (⇧D,
//! `view.devMode`; `sawit_board::design::devmode`): for the one selected layer, its ready-for-dev
//! mark, its properties section by section (each value with its colour and the style or variable
//! it comes from), its code in the language picked with a Copy button, and its annotations, which
//! can be added and removed. What it shows is worked out once for each document, layer and
//! language, not at every frame.

use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::devmode::{self, InspectSection};

use super::controls::{Choices, choice_menu};
use crate::board::BoardView;

/// What the panel showed last, and for what.
pub(in crate::board) struct InspectCache {
    doc: Arc<Document>,
    id: NodeId,
    language: &'static str,
    sections: Vec<InspectSection>,
    code: String,
}

/// Dev Mode's state on the canvas.
#[derive(Default)]
pub(in crate::board) struct DevMode {
    pub on: bool,
    pub language: Option<&'static str>,
    pub cache: Option<InspectCache>,
    pub note: Option<(Entity<InputState>, Subscription)>,
}

impl BoardView {
    /// Turns Dev Mode on or off: the Inspect panel takes the Design panel's place, which shows.
    pub(in crate::board) fn toggle_dev_mode(&mut self, cx: &mut Context<Self>) {
        self.dev.on = !self.dev.on;
        if self.dev.on {
            self.show_design = true;
        }
        cx.notify();
    }

    fn add_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.dev.note.as_ref().map(|(input, _)| input.clone()) else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        let id = self
            .board()
            .and_then(|b| b.session.selection().first().copied());
        if let (false, Some(id)) = (text.is_empty(), id) {
            let edit = devmode::add_annotation(id, &text);
            self.choose(edit.command, edit.params, cx);
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }
}

/// The Inspect panel's body.
pub(super) fn render_inspect(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let muted = theme.muted_foreground;
    let Some((doc, id)) = view.board().and_then(|b| {
        let ds = b.session.ds().ok()?;
        match ds.selection.as_slice() {
            [id] => Some((ds.doc.clone(), *id)),
            _ => None,
        }
    }) else {
        return div()
            .p_3()
            .text_sm()
            .text_color(muted)
            .child("Select one layer to inspect it.")
            .into_any_element();
    };
    let language = view.dev.language.unwrap_or("css");
    let fresh = view
        .dev
        .cache
        .as_ref()
        .is_some_and(|c| Arc::ptr_eq(&c.doc, &doc) && c.id == id && c.language == language);
    if !fresh {
        view.dev.cache = Some(InspectCache {
            sections: devmode::inspect(&doc, id),
            code: devmode::code(&doc, id, language),
            doc: doc.clone(),
            id,
            language,
        });
    }
    if view.dev.note.is_none() {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Add an annotation"));
        let events = cx.subscribe_in(&input, window, |view, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                view.add_note(window, cx);
            }
        });
        view.dev.note = Some((input, events));
    }
    let Some(cache) = view.dev.cache.as_ref() else {
        return div().into_any_element();
    };
    let this = cx.entity().downgrade();
    let mut body = v_flex().gap_3().p_3();
    if figcraft_engine::devinspect::has_status(&doc, id) {
        let ready = devmode::ready(&doc, id);
        let view_handle = this.clone();
        body = body.child(
            Switch::new("inspect-ready")
                .small()
                .checked(ready)
                .label("Ready for dev")
                .on_click(move |on, _, cx| {
                    let edit = devmode::set_ready(id, *on);
                    let _ = view_handle
                        .update(cx, |view, cx| view.choose(edit.command, edit.params, cx));
                }),
        );
    }
    for section in &cache.sections {
        let rows = section.rows.iter().map(|row| {
            let swatch = row.color.as_ref().map(|hex| {
                let colour = gpui_kit::Rgba::try_from(hex.as_str())
                    .map(Hsla::from)
                    .unwrap_or_default();
                div()
                    .size(px(12.))
                    .rounded(px(2.))
                    .border_1()
                    .border_color(theme.input)
                    .bg(colour)
            });
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(84.))
                        .flex_shrink_0()
                        .text_color(muted)
                        .truncate()
                        .child(row.label.clone()),
                )
                .children(swatch)
                .child(div().flex_1().min_w_0().truncate().child(row.value.clone()))
                .children(
                    row.token
                        .clone()
                        .map(|t| div().text_xs().text_color(muted).truncate().child(t)),
                )
        });
        body = body.child(
            v_flex()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(section.title.clone()),
                )
                .children(rows),
        );
    }
    let pick = {
        let view = this.clone();
        move |lang: &&'static str, cx: &mut App| {
            let lang = *lang;
            let _ = view.update(cx, |view, cx| {
                view.dev.language = Some(lang);
                cx.notify();
            });
        }
    };
    let label = devmode::languages()
        .into_iter()
        .find(|(l, _)| *l == language)
        .map_or("CSS", |(_, n)| n);
    let code = cache.code.clone();
    let copied = code.clone();
    body = body.child(
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Code"),
                    )
                    .child(div().w(px(110.)).child(choice_menu(
                        "inspect-language",
                        label,
                        "Language",
                        Choices::new(
                            || {
                                devmode::languages()
                                    .into_iter()
                                    .map(|(l, n)| (l, n.into()))
                                    .collect()
                            },
                            Some(language),
                            pick,
                        ),
                        false,
                    )))
                    .child(
                        Button::new("inspect-copy")
                            .ghost()
                            .small()
                            .label("Copy")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                                view.show_toast("Code copied", cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("inspect-code")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .p_2()
                    .rounded(px(4.))
                    .bg(theme.secondary)
                    .font_family("Menlo")
                    .text_xs()
                    .whitespace_normal()
                    .child(code),
            ),
    );
    let notes = devmode::annotations(&doc, id);
    let mut list = v_flex()
        .gap_1()
        .text_xs()
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Annotations"));
    for (index, note) in notes.into_iter().enumerate() {
        let edit = devmode::remove_annotation(id, index);
        list = list.child(
            h_flex()
                .gap_1()
                .items_center()
                .child(div().flex_1().min_w_0().child(note))
                .child(
                    Button::new(("inspect-note-remove", index))
                        .ghost()
                        .xsmall()
                        .label("Remove")
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.choose(edit.command, edit.params.clone(), cx);
                        })),
                ),
        );
    }
    if let Some((input, _)) = view.dev.note.as_ref() {
        list = list.child(
            h_flex()
                .gap_1()
                .child(div().flex_1().child(Input::new(input).small()))
                .child(
                    Button::new("inspect-note-add")
                        .small()
                        .label("Add")
                        .on_click(cx.listener(|view, _, window, cx| view.add_note(window, cx))),
                ),
        );
    }
    body.child(list).into_any_element()
}
