//! One pull request or issue in the GitHub tab: Summary, Changes (the whole
//! diff with line counts) and a Timeline of who did what (pushed commits,
//! reviews with their inline comments, comments, merges, labels…), with
//! a comment box underneath.

mod actions;
mod entry;
mod header;
mod markdown;
mod summary;
mod timeline;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::diff::{DiffFile, DiffState};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{self, Event, Item, PullState, TIMELINE_PAGE};

use crate::rt;
use crate::states::Banner;
use timeline::Filter;

pub enum ThreadEvent {
    /// Opened or closed on GitHub.
    StateChanged {
        number: u64,
        open: bool,
    },
    Commented(u64),
    Merged(u64),
}

impl EventEmitter<ThreadEvent> for ThreadView {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Summary,
    Changes,
    Timeline,
}

enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

pub struct ThreadView {
    pub item: Item,
    /// Whether the comment box was empty at the last Change.
    comment_empty: bool,
    owner: String,
    name: String,
    token: String,
    tab: Tab,
    filter: Filter,
    pull: Option<PullState>,
    events: Load<Vec<Event>>,
    /// Pages of the timeline loaded so far.
    pages: u32,
    more: bool,
    loading_more: bool,
    diff: Option<Load<Option<Entity<DiffState>>>>,
    comment: Entity<TextareaState>,
    working: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ThreadView {
    pub fn new(
        item: Item,
        (owner, name): (String, String),
        token: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let comment =
            cx.new(|cx| TextareaState::new(window, cx).placeholder(comment_placeholder(cx)));
        let subscriptions =
            vec![
                cx.subscribe_in(&comment, window, |this, input, e: &InputEvent, _, cx| {
                    // ⌘↩ sends through its binding (`super::SendComment`), which the keymap may change.
                    // Only the Send button depends on the text. Re-rendering the
                    // whole timeline per keystroke made typing lag on long threads.
                    if let InputEvent::Change = e {
                        let empty = input.read(cx).value().trim().is_empty();
                        if empty != this.comment_empty {
                            this.comment_empty = empty;
                            cx.notify();
                        }
                    }
                }),
            ];
        let mut this = Self {
            item,
            comment_empty: true,
            owner,
            name,
            token,
            tab: Tab::Timeline,
            filter: Filter::All,
            pull: None,
            events: Load::Loading,
            pages: 0,
            more: false,
            loading_more: false,
            diff: None,
            comment,
            working: false,
            error: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    pub fn number(&self) -> u64 {
        self.item.number
    }

    /// When `error` is GitHub refusing this thread's sign-in itself: the app signs out
    /// everywhere, and the GitHub page closes this thread for its sign-in card. True then.
    fn refused(&self, error: &sawit_core::CoreError, cx: &mut App) -> bool {
        super::account::note_refusal(error, &self.token, cx)
    }

    fn api(&self) -> (String, String, String, u64) {
        (
            self.token.clone(),
            self.owner.clone(),
            self.name.clone(),
            self.item.number,
        )
    }

    /// Reads the pull request state and the timeline's first page again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let (token, owner, name, number) = self.api();
        let pull = self.item.pull;
        self.events = Load::Loading;
        self.pages = 0;
        self.diff = None;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            let state = if pull {
                github::pull_state(&token, &owner, &name, number).await.ok()
            } else {
                None
            };
            let events = github::timeline(&token, &owner, &name, number, pull, 1).await;
            (state, events)
        });
        cx.spawn(async move |this, cx| {
            let Ok((state, events)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.pull = state;
                match events {
                    Ok(events) => {
                        this.more = events.len() >= TIMELINE_PAGE;
                        this.pages = 1;
                        this.events = Load::Ready(events);
                    }
                    Err(e) if this.refused(&e, cx) => {}
                    Err(e) => this.events = Load::Failed(e.to_string()),
                }
                if this.tab == Tab::Changes {
                    this.load_diff(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading_more {
            return;
        }
        let (token, owner, name, number) = self.api();
        let (pull, page) = (self.item.pull, self.pages + 1);
        self.loading_more = true;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::timeline(&token, &owner, &name, number, pull, page).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                match result {
                    Ok(more) => {
                        this.more = more.len() >= TIMELINE_PAGE;
                        this.pages = page;
                        if let Load::Ready(events) = &mut this.events {
                            events.extend(more);
                        }
                    }
                    Err(e) if this.refused(&e, cx) => {}
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_diff(&mut self, cx: &mut Context<Self>) {
        if self.diff.is_some() || !self.item.pull {
            return;
        }
        let (token, owner, name, number) = self.api();
        self.diff = Some(Load::Loading);
        cx.notify();
        // Parsing a big pull request's diff takes long enough to freeze the UI, so it happens
        // next to the download.
        let rx = rt::spawn(cx, async move {
            let patch = github::pull_diff(&token, &owner, &name, number).await?;
            let parsed = tokio::task::spawn_blocking(move || {
                DiffFile::parse(&patch).map_err(|e| e.message().to_string())
            })
            .await
            .unwrap_or_else(|_| Err("The diff parser stopped.".into()));
            Ok::<_, sawit_core::CoreError>(parsed)
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.diff = Some(match result {
                    Ok(Ok(files)) if files.is_empty() => Load::Ready(None),
                    Ok(Ok(files)) => Load::Ready(Some(cx.new(|cx| DiffState::new(files, cx)))),
                    Ok(Err(e)) => Load::Failed(format!("Unable to show this diff. {e}")),
                    Err(e) if this.refused(&e, cx) => Load::Loading,
                    Err(e) => Load::Failed(e.to_string()),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == Tab::Changes {
            self.load_diff(cx);
        }
        cx.notify();
    }
}

// ------------------------------------------------------------ render

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_send = !self.comment.read(cx).value().trim().is_empty() && !self.working;
        let body = match self.tab {
            Tab::Summary => self.render_summary(cx),
            Tab::Changes => self.render_changes(),
            Tab::Timeline => self.render_timeline(cx),
        };
        v_flex()
            .size_full()
            .child(
                div()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(self.render_header(cx)),
            )
            .child(div().flex_1().min_h_0().child(body))
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("gh-error", e))
            })
            .when(self.tab != Tab::Changes, |el| {
                el.child(
                    v_flex()
                        .p_3()
                        .gap_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .key_context("GitComment")
                                .on_action(cx.listener(
                                    |this, _: &super::SendComment, window, cx| {
                                        this.send_comment(window, cx)
                                    },
                                ))
                                .child(
                                    Textarea::new(&self.comment)
                                        .h(px(72.))
                                        .aria_label("Comment"),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .when(self.working, |el| {
                                            el.child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Spinner::new().xsmall())
                                                    .child("Working…"),
                                            )
                                        }),
                                )
                                .child(
                                    Button::new("gh-send")
                                        .primary()
                                        .small()
                                        .icon(Lucide::MessageSquare)
                                        .label("Comment")
                                        .disabled(!can_send)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.send_comment(window, cx)
                                        })),
                                ),
                        ),
                )
            })
    }
}

/// The comment box's placeholder, with the keys that send in it as the keymap binds them.
pub(super) fn comment_placeholder(cx: &App) -> String {
    match crate::keymap::keys_for_path(&super::SendComment, &["GitComment", "Input"], cx) {
        keys if keys.is_empty() => "Leave a comment (Markdown)".to_string(),
        keys => format!("Leave a comment (Markdown, {keys} to send)"),
    }
}
