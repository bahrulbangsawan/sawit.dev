//! The panel as drawn: whether the page picks, the cards grouped by page under their headers (or
//! how to start, while there are none), and the footer's actions.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, page_key};

use super::{AnnotateEvent, AnnotatePanel, Picking, Row, group_order};
use crate::browser::panel;

impl AnnotatePanel {
    /// The line under the header: whether the page picks, and how to start again.
    fn status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (dot, text) = match self.picking {
            Picking::On => (theme.primary, "Click an element on the page to annotate it"),
            Picking::Waiting => (theme.muted_foreground, "Waiting for the page to load…"),
            Picking::Off => (theme.muted_foreground, "Picking is off"),
        };
        h_flex()
            .flex_none()
            .h(px(36.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(if self.picking == Picking::On {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(text),
            )
            .when(self.picking == Picking::Off, |el| {
                el.child(
                    Button::new("annotate-pick")
                        .outline()
                        .xsmall()
                        .icon(Lucide::MousePointer2)
                        .label("Pick element")
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(AnnotateEvent::Pick);
                        })),
                )
            })
            .when(self.picking == Picking::On, |el| {
                el.child(panel::key("Esc", cx)).child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("stops"),
                )
            })
    }

    /// How to start, and the keys, while there is nothing yet.
    fn empty_state(&self, window: &Window, cx: &App) -> impl IntoElement {
        let annotate = crate::keymap::keys_for(&crate::browser::ToggleAnnotate, "Browser", window);
        let muted = cx.theme().muted_foreground;
        let line = |keys: Vec<Div>, what: &'static str| {
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .children(keys)
                .child(div().pl_1().text_color(muted).child(what))
        };
        v_flex()
            .gap_4()
            .child(panel::empty(
                "No annotations yet",
                "Click an element on the page, say what should change, and press Return. \
                 Notes go to your agent with the element's selector, styles, source file and \
                 a screenshot.",
                None,
                cx,
            ))
            .child(panel::section(
                "Keys",
                None,
                v_flex()
                    .gap_1p5()
                    .child(line(
                        vec![panel::key("↑", cx), panel::key("↓", cx)],
                        "Walk to the parent or back to the child",
                    ))
                    .child(line(
                        vec![panel::key("⌥", cx), panel::key("Click", cx)],
                        "Pick the parent of what's under the pointer",
                    ))
                    .child(line(
                        vec![panel::key("↩", cx)],
                        "Save the note; ⇧↩ for a new line",
                    ))
                    .child(line(
                        vec![panel::key("Esc", cx)],
                        "Cancel the note; again to stop picking",
                    ))
                    .child(line(
                        vec![panel::key("⌫", cx)],
                        "Remove the note whose marker has focus",
                    ))
                    .when(!annotate.is_empty(), |el| {
                        el.child(line(
                            vec![panel::key(annotate.clone(), cx)],
                            "Annotate on or off",
                        ))
                    }),
                cx,
            ))
    }

    /// The group header: the page's title, its path under it when that says more than the
    /// title does (the home page's `/` doesn't), with the host in a tooltip; and with two notes
    /// or more, Collapse all, or Expand all when every one is folded.
    fn group_header(&self, key: &str, title: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let (path, host) = split_url(key);
        let muted = cx.theme().muted_foreground;
        let title = if title.trim().is_empty() {
            host.clone()
        } else {
            title.to_string()
        };
        let shown_path = route_line(&path);
        let rows: Vec<&Row> = self
            .rows
            .iter()
            .filter(|r| page_key(&r.annotation.url) == key)
            .collect();
        let fold = (rows.len() >= 2).then(|| rows.iter().all(|r| r.collapsed));
        let page = key.to_string();
        h_flex()
            .gap_2()
            .items_center()
            .child(
                v_flex()
                    .id(SharedString::from(format!("annotate-page-{key}")))
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .tooltip(move |window, cx| {
                        Tooltip::new(key_tooltip(&host, &path)).build(window, cx)
                    })
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .children(shown_path.map(|path| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(path)
                    })),
            )
            .children(fold.map(|all_folded| {
                Button::new(SharedString::from(format!("annotate-fold-{key}")))
                    .debug_selector(|| "annotate-fold".into())
                    .ghost()
                    .xsmall()
                    .icon(if all_folded {
                        Lucide::UnfoldVertical
                    } else {
                        Lucide::FoldVertical
                    })
                    .label(if all_folded {
                        "Expand all"
                    } else {
                        "Collapse all"
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.collapse_page(&page, !all_folded, cx);
                    }))
            }))
    }

    fn footer(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_rows = !self.rows.is_empty();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let errors_label = match self.errors {
            0 => "Include console errors".to_string(),
            n => format!("Include console errors ({n})"),
        };
        let undo = !self.removed.is_empty();
        let reason = self.send_blocked(has_rows);
        v_flex()
            .flex_none()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            // What goes with the notes, and the destructive action, apart from sending them.
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Checkbox::new("annotate-errors")
                                .label(errors_label)
                                .checked(self.include_errors)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.include_errors = *checked;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        Button::new("annotate-clear")
                            .debug_selector(|| "annotate-clear".into())
                            .xsmall()
                            .danger()
                            .outline()
                            .icon(Lucide::Trash)
                            .label("Clear all")
                            .disabled(!has_rows)
                            .accessibility_label("Clear all annotations")
                            .tooltip("Remove every annotation; you can undo it")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.request_clear(window, cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.send_control(has_rows, window, cx))
                    .child(
                        Button::new("annotate-copy")
                            .debug_selector(|| "annotate-copy".into())
                            .small()
                            .outline()
                            .icon(Lucide::Copy)
                            .label("Copy")
                            .disabled(!has_rows)
                            .tooltip("Copy the notes as Markdown")
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Copy);
                            })),
                    ),
            )
            .children(self.note.clone().or(reason).map(|note| {
                h_flex()
                    .min_h(px(22.))
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .debug_selector(|| "annotate-note".into())
                            .flex_1()
                            .text_xs()
                            .text_color(muted)
                            .child(note),
                    )
                    .when(undo, |el| {
                        el.child(
                            Button::new("annotate-undo")
                                .xsmall()
                                .outline()
                                .icon(Lucide::Undo2)
                                .label("Undo")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.undo(cx);
                                })),
                        )
                    })
            }))
    }
}

impl Render for AnnotatePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items: Vec<Annotation> = self.rows.iter().map(|r| r.annotation.clone()).collect();
        let groups = group_order(&self.current, &items);
        let list = if self.rows.is_empty() {
            self.empty_state(window, cx).into_any_element()
        } else {
            let mut sections: Vec<AnyElement> = Vec::new();
            for key in groups {
                let title = if key == page_key(&self.current) {
                    self.title.clone()
                } else {
                    items
                        .iter()
                        .find(|a| page_key(&a.url) == key)
                        .map(|a| a.title.clone())
                        .unwrap_or_default()
                };
                let mut rows: Vec<AnyElement> = Vec::new();
                for ix in 0..self.rows.len() {
                    if page_key(&self.rows[ix].annotation.url) == key {
                        rows.push(self.row(ix, cx).into_any_element());
                    }
                }
                sections.push(
                    v_flex()
                        .gap_2()
                        .child(self.group_header(&key, &title, cx))
                        .children(rows)
                        .into_any_element(),
                );
            }
            v_flex().gap_6().children(sections).into_any_element()
        };
        v_flex()
            .size_full()
            .child(self.status(cx))
            .child(panel::scroll("annotate-list").child(list))
            .child(self.footer(window, cx))
    }
}

/// The path a group header shows under the page's title, when it says more than the title
/// alone: not the home page's bare `/`.
pub(super) fn route_line(path: &str) -> Option<String> {
    (path != "/").then(|| path.to_string())
}

/// The group header's tooltip: where the page is.
fn key_tooltip(host: &str, path: &str) -> String {
    format!("{host}{path}")
}

/// A URL's path (with query) for a header, and its host for the tooltip.
pub(super) fn split_url(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    match rest.find('/') {
        Some(i) => (rest[i..].to_string(), rest[..i].to_string()),
        None => ("/".to_string(), rest.to_string()),
    }
}
