//! A note's card: folded to one line (its number, kind, element and the note's first line), or
//! open with its kind, its comment and its actions.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::annotations::{Annotation, Intent};

use super::{AnnotateEvent, AnnotatePanel};
use crate::browser::panel;

impl AnnotatePanel {
    /// A note's card: one line while folded (its number, kind, element and the first line of the
    /// comment), else the whole card. Its head folds and opens it, by click, ↩ or Space.
    pub(super) fn row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let theme = cx.theme();
        let collapsed = row.collapsed;
        v_flex()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(self.card_head(ix, cx))
            .when(!collapsed, |el| el.child(self.card_body(ix, cx)))
    }

    /// The part of a card that folds it: a button that says whether the card is open.
    fn card_head(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let collapsed = row.collapsed;
        let comment = row.comment.read(cx).value().to_string();
        let first = first_line(&comment);
        let kind = intent_name(a.intent);
        let inner = theme.radius - px(1.);
        let label = if first.is_empty() {
            format!("Annotation {}, {kind}, no note yet", a.n)
        } else {
            format!("Annotation {}, {kind}: {first}", a.n)
        };
        let head = gpui_kit::base::Button::new(("annotate-card", row.key))
            .track_focus(&row.head)
            .debug_selector(move || format!("annotate-card-{ix}"))
            .accessibility_label(label)
            .aria_expanded(!collapsed)
            .w_full()
            .flex()
            .gap_2()
            .px_3()
            .border_1()
            .border_color(transparent_black())
            .map(|el| {
                if collapsed {
                    el.py_1p5().items_center().rounded(inner)
                } else {
                    el.pt_3().pb_2().items_start().rounded_t(inner)
                }
            })
            .line_height(relative(1.4))
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .focus_visible(|s| s.border_color(theme.ring))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle(ix, cx)))
            .child(number_badge(a.n, cx));
        let chevron = Icon::new(if collapsed {
            Lucide::ChevronRight
        } else {
            Lucide::ChevronDown
        })
        .xsmall()
        .flex_none()
        .text_color(muted);
        if collapsed {
            let full = comment.trim().to_string();
            head.when(!full.is_empty(), |el| {
                el.tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            })
            .child(panel::tag(kind, cx))
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w(px(24.))
                    .max_w(px(112.))
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .font_family(mono)
                    .child(a.element.label.clone()),
            )
            .child(div().flex_1().min_w_0().truncate().text_xs().map(|el| {
                if first.is_empty() {
                    el.text_color(muted).child("No note yet")
                } else {
                    el.child(first)
                }
            }))
            .child(chevron)
            .into_any_element()
        } else {
            let summary = element_summary(a);
            let source = source_line(a);
            head.child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .font_family(mono.clone())
                            .child(a.element.label.clone()),
                    )
                    .child(div().truncate().text_xs().text_color(muted).child(summary))
                    .children(source.map(|s| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(mono)
                            .child(s)
                    })),
            )
            .child(div().pt_0p5().child(chevron))
            .into_any_element()
        }
    }

    /// An open card under its head: the kind, the comment and the card's actions.
    fn card_body(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let row = &self.rows[ix];
        let a = &row.annotation;
        let selector = a.element.selector.clone();
        let shot = a.include_screenshot;
        let has_shot = a.screenshot.is_some();
        let intent_ix = Intent::ALL.iter().position(|i| *i == a.intent).unwrap_or(0);
        v_flex()
            .gap_2()
            .px_3()
            .pb_3()
            .child(
                TabBar::new(("annotate-intent", ix))
                    .segmented()
                    .xsmall()
                    .w_full()
                    .selected_index(intent_ix)
                    .children(Intent::ALL.iter().map(|i| {
                        Tab::new()
                            .label(intent_name(*i))
                            .aria_label(format!("Kind: {}", intent_name(*i)))
                            .flex_1()
                    }))
                    .on_click(cx.listener(move |this, at: &usize, _, cx| {
                        if let Some(i) = Intent::ALL.get(*at) {
                            this.set_intent(ix, *i, cx);
                        }
                    })),
            )
            .child(
                Textarea::new(&row.comment)
                    .h(px(64.))
                    .aria_label(format!("Comment for annotation {}", a.n)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new(("annotate-show", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Eye)
                            .label("Show")
                            .tooltip("Scroll the page to the element and flash it")
                            .accessibility_label(format!("Show annotation {} on the page", a.n))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(AnnotateEvent::Show(selector.clone()));
                            })),
                    )
                    .child(
                        Button::new(("annotate-shot", ix))
                            .ghost()
                            .xsmall()
                            .icon(if shot { Lucide::Check } else { Lucide::Camera })
                            .label("Screenshot")
                            .selected(shot && has_shot)
                            .disabled(!has_shot)
                            .tooltip(if !has_shot {
                                "No screenshot was taken of this element"
                            } else if shot {
                                "The screenshot goes with the note; click to leave it out"
                            } else {
                                "Click to send the screenshot with the note"
                            })
                            .accessibility_label(if shot {
                                "Leave the screenshot out"
                            } else {
                                "Include the screenshot"
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Some(r) = this.rows.get_mut(ix) {
                                    r.annotation.include_screenshot =
                                        !r.annotation.include_screenshot;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(("annotate-delete", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Trash)
                            .label("Delete")
                            .accessibility_label(format!("Delete annotation {}", a.n))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(ix, cx);
                            })),
                    ),
            )
    }
}

/// The number on a note's marker.
fn number_badge(n: u32, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex_none()
        .min_w(px(20.))
        .h(px(20.))
        .px_1()
        .rounded_full()
        .bg(theme.primary)
        .text_color(theme.primary_foreground)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(n.to_string())
}

/// What the element is, under its label: `button “Shop the harvest” · 180×40`.
pub(super) fn element_summary(a: &Annotation) -> String {
    let el = &a.element;
    let size = format!("{}×{}", el.rect.width.round(), el.rect.height.round());
    match el.role.as_deref().filter(|r| !r.is_empty()) {
        Some(role) => match el.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => format!("{role} “{}” · {size}", cut(name, 40)),
            None => format!("{role} · {size}"),
        },
        None => match el.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(text) => format!("“{}” · {size}", cut(text, 40)),
            None => size,
        },
    }
}

/// The source line under a card's label: `src/Hero.tsx:24 · <HeroActions>`.
pub(super) fn source_line(a: &Annotation) -> Option<String> {
    let src = a.element.source.as_ref()?;
    let mut parts = Vec::new();
    if !src.file.is_empty() {
        parts.push(match src.line {
            Some(line) => format!("{}:{line}", src.file),
            None => src.file.clone(),
        });
    }
    if let Some(c) = src.component.as_deref().filter(|c| real_component(c)) {
        parts.push(format!("<{c}>"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Whether a component's name, as the page's framework reports it, names something the user
/// wrote: `HeroActions`, `nav-bar`, `Card.Header`. A production build minifies names (`$u`,
/// `Xe`, `t`), and a placeholder like `Anonymous` names nothing; a card shows neither.
pub(super) fn real_component(name: &str) -> bool {
    let name = name.trim();
    name.chars().count() >= 3
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '(' | ')'))
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "anonymous" | "unknown" | "component"
        )
}

/// A note's first line that has words, for its folded card.
pub(super) fn first_line(comment: &str) -> String {
    comment
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// The intent as its button shows it: "Fix", "Change", "Question", "Copy".
fn intent_name(i: Intent) -> &'static str {
    match i {
        Intent::Fix => "Fix",
        Intent::Change => "Change",
        Intent::Question => "Question",
        Intent::Copy => "Copy",
    }
}
