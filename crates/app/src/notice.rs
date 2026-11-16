//! A notice or a plain question (OK, Save / Don't Save, Install / Cancel) whose detail may be long:
//! what a run changed with every path, a JSON before and after, a server's error.
//!
//! [`prompt`] takes what `window.prompt` takes and answers the same way. A short detail still goes
//! to the system alert. A long one would grow the alert past the screen with no way to scroll, so
//! it opens a dialog in the window instead: the detail scrolls inside a box of its own, Copy takes
//! all of it, the first answer is the default (Return), and Escape or a click beside it answers
//! Cancel when there is one, else the last answer.
//!
//! Destructive questions are [`crate::confirm`]'s, not this.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The dialog's width.
const WIDTH: Pixels = px(480.);
/// How tall the detail grows before it scrolls.
const DETAIL_MAX_H: Pixels = px(320.);
/// A detail longer than this, in lines or in characters, opens the dialog instead of the alert.
const ALERT_LINES: usize = 10;
const ALERT_CHARS: usize = 600;

/// Whether `detail` is too long for the system alert.
pub fn too_long(detail: Option<&str>) -> bool {
    detail.is_some_and(|d| d.lines().count() > ALERT_LINES || d.chars().count() > ALERT_CHARS)
}

/// Asks `title` with `detail` and `answers` (the first one the default), as `window.prompt` does,
/// and answers with the index of the button chosen.
pub fn prompt(
    window: &mut Window,
    level: PromptLevel,
    title: &str,
    detail: Option<&str>,
    answers: &[&str],
    cx: &mut App,
) -> oneshot::Receiver<usize> {
    if !too_long(detail) {
        return window.prompt(level, title, detail, answers, cx);
    }
    let (tx, rx) = oneshot::channel();
    let view = cx.new(|cx| NoticeView {
        level,
        title: title.to_string().into(),
        detail: detail.unwrap_or_default().to_string().into(),
        answers: answers
            .iter()
            .map(|a| SharedString::from(a.to_string()))
            .collect(),
        answer: Rc::new(RefCell::new(Some(tx))),
        default_focus: cx.focus_handle(),
    });
    let first = std::cell::Cell::new(true);
    let (ok_view, close_view) = (view.clone(), view.clone());
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            view.read(cx).default_focus.clone().focus(window, cx);
        }
        let (ok_view, close_view) = (ok_view.clone(), close_view.clone());
        dialog
            .w(WIDTH)
            .keyboard(true)
            .overlay_closable(true)
            .close_button(false)
            .child(view.clone())
            .on_ok(move |_, _, cx| {
                ok_view.update(cx, |v, _| v.send(0));
                true
            })
            .on_close(move |_, _, cx| close_view.update(cx, |v, _| v.send(v.cancel_index())))
    });
    rx
}

/// The dialog's body, and the answer, sent once.
pub struct NoticeView {
    level: PromptLevel,
    title: SharedString,
    detail: SharedString,
    answers: Vec<SharedString>,
    answer: Rc<RefCell<Option<oneshot::Sender<usize>>>>,
    default_focus: FocusHandle,
}

impl NoticeView {
    /// What Escape or a click beside the dialog answers: Cancel when it is an answer, else the
    /// last one (the only one for a notice).
    fn cancel_index(&self) -> usize {
        self.answers
            .iter()
            .position(|a| a.as_ref() == "Cancel")
            .unwrap_or(self.answers.len().saturating_sub(1))
    }

    fn send(&mut self, ix: usize) {
        if let Some(tx) = self.answer.borrow_mut().take() {
            let _ = tx.send(ix);
        }
    }

    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.send(ix);
        window.close_dialog(cx);
    }
}

impl Render for NoticeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (icon, tint) = match self.level {
            PromptLevel::Critical => (IconName::CircleX, theme.danger),
            PromptLevel::Warning => (IconName::TriangleAlert, theme.warning),
            _ => (IconName::Info, theme.muted_foreground),
        };
        let detail = self.detail.clone();
        // NSAlert's order: the default (first) answer on the right.
        let buttons = self.answers.iter().enumerate().rev().map(|(ix, label)| {
            Button::new(("notice-answer", ix))
                .label(label.clone())
                .when(ix == 0, |b| b.primary().track_focus(&self.default_focus))
                .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)))
        });
        v_flex()
            .id("notice")
            .debug_selector(|| "notice".into())
            .gap_3()
            .w_full()
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .pt(px(2.))
                            .flex_none()
                            .child(Icon::new(icon).small().text_color(tint)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title.clone()),
                    ),
            )
            .child(
                div()
                    .id("notice-detail")
                    .max_h(DETAIL_MAX_H)
                    .overflow_y_scroll()
                    .p_2()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.muted.opacity(0.4))
                    .text_sm()
                    .whitespace_normal()
                    .text_color(theme.foreground)
                    .child(self.detail.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("notice-copy")
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .label("Copy")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(detail.to_string()))
                            }),
                    )
                    .child(div().flex_1())
                    .children(buttons),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{ALERT_CHARS, ALERT_LINES, too_long};

    #[test]
    fn only_a_long_detail_leaves_the_alert() {
        assert!(!too_long(None));
        assert!(!too_long(Some(
            "Your changes will be lost if you don't save them."
        )));
        assert!(too_long(Some(&"a line\n".repeat(ALERT_LINES + 1))));
        assert!(too_long(Some(&"x".repeat(ALERT_CHARS + 1))));
    }
}
