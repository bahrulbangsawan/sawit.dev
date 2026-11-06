//! Comments on the canvas (`sawit_board::design::comments`): the Comment tool (C) asks for a new
//! thread where it is clicked, or opens the thread whose pin is there; View ▸ Comments… lists the
//! page's threads. The window shows them, open ones first, each with its messages, and the one
//! picked with a field to reply, Resolve (or Reopen) and Delete. A new thread's window has one
//! field, and Enter posts it.

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::design::comments::{self, Thread};

use super::BoardView;

const WIDTH: f32 = 420.;
const TOP: f32 = 64.;
/// How far from a pin (screen px) a click with the Comment tool opens its thread.
const PIN_REACH: f64 = 14.0;

/// The window, while it is open.
pub(super) struct CommentsUi {
    /// A new thread at this page point, or the threads with this one open.
    new_at: Option<(f64, f64)>,
    open: Option<u64>,
    input: Entity<InputState>,
    focus_pending: bool,
    _events: Subscription,
}

impl BoardView {
    /// The Comment tool's click at page point (`x`, `y`): the thread whose pin is there, or a new
    /// one.
    pub(super) fn comment_at(
        &mut self,
        x: f64,
        y: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((doc, page, zoom)) = self
            .board()
            .and_then(|b| b.session.ds().ok())
            .map(|ds| (ds.doc.clone(), ds.page, ds.view.zoom))
        else {
            return;
        };
        let reach = PIN_REACH / zoom.max(0.01);
        match comments::pin_at(&doc, page, x, y, reach) {
            Some(id) => self.open_comments(None, Some(id), window, cx),
            None => self.open_comments(Some((x, y)), None, window, cx),
        }
    }

    /// Opens the window: a new thread at `new_at`, or the page's threads with `open` open.
    pub(super) fn open_comments(
        &mut self,
        new_at: Option<(f64, f64)>,
        open: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menus(cx);
        let hint = if new_at.is_some() {
            "Add a comment"
        } else {
            "Reply"
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        let events = cx.subscribe_in(&input, window, |view, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                view.post_comment(window, cx);
            }
        });
        self.comments_ui = Some(CommentsUi {
            new_at,
            open,
            input,
            focus_pending: true,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn close_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.comments_ui.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Posts what was typed: a new thread, or a reply to the open one.
    fn post_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, new_at, open)) = self
            .comments_ui
            .as_ref()
            .map(|ui| (ui.input.clone(), ui.new_at, ui.open))
        else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let edit = match (new_at, open) {
            (Some((x, y)), _) => comments::add(x, y, &text),
            (None, Some(id)) => comments::reply(id, &text),
            (None, None) => return,
        };
        self.choose(edit.command, edit.params, cx);
        input.update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(at) = new_at {
            // The new thread is the newest at that point: show it open.
            let page_threads = self
                .board()
                .and_then(|b| b.session.ds().ok())
                .map(|ds| comments::threads(&ds.doc, ds.page))
                .unwrap_or_default();
            let opened = page_threads.iter().rev().find(|t| t.at == at).map(|t| t.id);
            if let Some(ui) = self.comments_ui.as_mut() {
                ui.new_at = None;
                ui.open = opened;
            }
        }
        cx.notify();
    }
}

/// The window, while it is open.
pub(super) fn render_comments(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let (doc, page) = view
        .board()
        .and_then(|b| b.session.ds().ok())
        .map(|ds| (ds.doc.clone(), ds.page))?;
    let ui = view.comments_ui.as_mut()?;
    if std::mem::take(&mut ui.focus_pending) {
        ui.input.update(cx, |input, cx| input.focus(window, cx));
    }
    let theme = cx.theme().clone();
    let muted = theme.muted_foreground;
    let mut threads = comments::threads(&doc, page);
    threads.sort_by_key(|t| t.resolved);
    let input = ui.input.clone();
    let (new_at, open) = (ui.new_at, ui.open);
    let title = if new_at.is_some() {
        "New comment"
    } else {
        "Comments"
    };
    let mut body = v_flex()
        .gap_2()
        .max_h(px(420.))
        .id("comments-list")
        .overflow_y_scroll();
    if new_at.is_none() {
        if threads.is_empty() {
            body =
                body.child(div().text_sm().text_color(muted).child(
                    "No comments on this page. Click with the Comment tool (C) to add one.",
                ));
        }
        for thread in threads {
            body = body.child(thread_card(&thread, open == Some(thread.id), &input, cx));
        }
    }
    let field = h_flex()
        .gap_1()
        .child(div().flex_1().child(Input::new(&input).small()))
        .child(
            Button::new("comments-post")
                .primary()
                .small()
                .label(if new_at.is_some() { "Post" } else { "Reply" })
                .disabled(new_at.is_none() && open.is_none())
                .on_click(cx.listener(|view, _, window, cx| view.post_comment(window, cx))),
        );
    Some(
        v_flex()
            .id("comments-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .items_center()
            .pt(px(TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.close_comments(window, cx)),
            )
            .child(
                v_flex()
                    .id("comments-window")
                    .debug_selector(|| "comments-window".into())
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            view.close_comments(window, cx);
                            cx.stop_propagation();
                        }
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
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                Button::new("comments-close")
                                    .ghost()
                                    .small()
                                    .label("Close")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_comments(window, cx)
                                    })),
                            ),
                    )
                    .child(body)
                    .child(field),
            )
            .into_any_element(),
    )
}

/// One thread: its first message, and while open its replies and its actions.
fn thread_card(
    thread: &Thread,
    open: bool,
    _input: &Entity<InputState>,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let id = thread.id;
    let message = |m: &sawit_board::design::comments::MessageRow| {
        let author = if m.agent {
            format!("{} (agent)", m.author)
        } else if m.author.is_empty() {
            "You".to_string()
        } else {
            m.author.clone()
        };
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(author),
            )
            .child(div().text_sm().child(m.text.clone()))
    };
    let shown: Vec<_> = if open {
        thread.messages.iter().map(message).collect()
    } else {
        thread.messages.first().map(message).into_iter().collect()
    };
    let mut card = v_flex()
        .id(SharedString::from(format!("comment-{id}")))
        .gap_1()
        .p_2()
        .rounded(px(6.))
        .border_1()
        .border_color(if open { theme.ring } else { theme.border })
        .when(thread.resolved, |c| c.opacity(0.6))
        .children(shown)
        .on_click(cx.listener(move |view, _, _, cx| {
            if let Some(ui) = view.comments_ui.as_mut() {
                ui.open = Some(id);
                cx.notify();
            }
        }));
    if !open && thread.messages.len() > 1 {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("{} replies", thread.messages.len() - 1)),
        );
    }
    if open {
        let resolved = thread.resolved;
        card = card.child(
            h_flex()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!("comment-resolve-{id}")))
                        .ghost()
                        .xsmall()
                        .label(if resolved { "Reopen" } else { "Resolve" })
                        .on_click(cx.listener(move |view, _, _, cx| {
                            let e = comments::resolve(id, !resolved);
                            view.choose(e.command, e.params, cx);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("comment-delete-{id}")))
                        .ghost()
                        .xsmall()
                        .label("Delete")
                        .on_click(cx.listener(move |view, _, _, cx| {
                            let e = comments::delete(id);
                            view.choose(e.command, e.params, cx);
                        })),
                ),
        );
    }
    card.into_any_element()
}
