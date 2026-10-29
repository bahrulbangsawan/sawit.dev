//! One pull request or issue in the GitHub tab: Summary, Changes (the whole
//! diff with line counts) and a Timeline of who did what (pushed commits,
//! reviews with their inline comments, comments, merges, labels…), with
//! a comment box underneath.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{
    self, ActionKind, Entry, Event, Item, PullState, ReviewComment, ReviewState, TIMELINE_PAGE,
    ago, now,
};

use crate::rt;
use crate::states::{Banner, Placeholder};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Comments,
    Reviews,
    Commits,
}

impl Filter {
    fn keeps(self, entry: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Comments => matches!(entry, Entry::Event(Event::Commented { .. })),
            Filter::Reviews => matches!(entry, Entry::Event(Event::Reviewed { .. })),
            Filter::Commits => matches!(entry, Entry::Pushed { .. }),
        }
    }
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
        let comment = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Leave a comment (Markdown, ⌘↩ to send)")
        });
        let subscriptions = vec![cx.subscribe_in(
            &comment,
            window,
            |this, input, e: &InputEvent, window, cx| {
                match e {
                    InputEvent::PressEnter {
                        secondary: true, ..
                    } => this.send_comment(window, cx),
                    // Only the Send button depends on the text. Re-rendering the
                    // whole timeline per keystroke made typing lag on long threads.
                    InputEvent::Change => {
                        let empty = input.read(cx).value().trim().is_empty();
                        if empty != this.comment_empty {
                            this.comment_empty = empty;
                            cx.notify();
                        }
                    }
                    _ => {}
                }
            },
        )];
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
        let rx = rt::spawn(cx, async move {
            github::pull_diff(&token, &owner, &name, number).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.diff = Some(match result {
                    Ok(patch) => match DiffFile::parse(&patch) {
                        Ok(files) if files.is_empty() => Load::Ready(None),
                        Ok(files) => Load::Ready(Some(cx.new(|cx| DiffState::new(files, cx)))),
                        Err(e) => {
                            Load::Failed(format!("Unable to show this diff. {}", e.message()))
                        }
                    },
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

    fn send_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.comment.read(cx).value().trim().to_string();
        if body.is_empty() || self.working {
            return;
        }
        let (token, owner, name, number) = self.api();
        self.working = true;
        self.error = None;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::add_comment(&token, &owner, &name, number, &body).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = false;
                match result {
                    Ok(c) => {
                        if let Load::Ready(events) = &mut this.events {
                            events.push(Event::Commented {
                                author: c.author,
                                time: c.created_at,
                                body: c.body,
                            });
                        }
                        this.item.comments += 1;
                        this.comment.update(cx, |s, cx| s.set_value("", window, cx));
                        this.tab = Tab::Timeline;
                        cx.emit(ThreadEvent::Commented(number));
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Closes or reopens it on GitHub.
    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        let (token, owner, name, number) = self.api();
        self.working = true;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::set_open(&token, &owner, &name, number, open).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                match result {
                    Ok(()) => {
                        this.item.open = open;
                        cx.emit(ThreadEvent::StateChanged { number, open });
                        this.reload(cx);
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (token, owner, name, number) = self.api();
        let (head, base) = self
            .pull
            .as_ref()
            .map(|p| (p.head.clone(), p.base.clone()))
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Merge pull request #{number}?"),
            Some(&format!(
                "{head} is merged into {base} on GitHub with a merge commit."
            )),
            &["Merge Pull Request", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(rx) = this.update(cx, |this, cx| {
                this.working = true;
                this.error = None;
                cx.notify();
                rt::spawn(cx, async move {
                    github::merge_pull(&token, &owner, &name, number).await
                })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                match result {
                    Ok(Ok(())) => {
                        this.item.open = false;
                        cx.emit(ThreadEvent::Merged(number));
                        this.reload(cx);
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The timeline rows: who opened it, then everything since.
    fn entries(&self) -> Vec<Entry> {
        let Load::Ready(events) = &self.events else {
            return Vec::new();
        };
        github::group(events)
    }

    // ------------------------------------------------------------ render

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let item = &self.item;
        let muted = cx.theme().muted_foreground;
        let merged = self.pull.as_ref().is_some_and(|p| p.merged);
        let (state_label, state_color) = match (item.open, merged) {
            (true, _) => ("Open", cx.theme().success),
            (false, true) => ("Merged", cx.theme().magenta),
            (false, false) => ("Closed", cx.theme().danger),
        };
        let can_merge = item.pull
            && item.open
            && self
                .pull
                .as_ref()
                .is_some_and(|p| p.mergeable != Some(false));
        let url = item.url.clone();
        let open = item.open;
        let pull = item.pull;
        v_flex()
            .px_4()
            .pt_2()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_semibold()
                            .child(item.title.clone()),
                    )
                    .child(
                        Button::new("gh-reload")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        Button::new("gh-open-web")
                            .ghost()
                            .small()
                            .icon(Lucide::ExternalLink)
                            .tooltip("Open on GitHub")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .when(!merged, |el| {
                        el.child(
                            Button::new("gh-toggle-open")
                                .small()
                                .label(match (open, pull) {
                                    (true, true) => "Close Pull Request",
                                    (true, false) => "Close Issue",
                                    _ => "Reopen",
                                })
                                .disabled(self.working)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.set_open(!open, cx)),
                                ),
                        )
                    })
                    .when(can_merge, |el| {
                        el.child(
                            Button::new("gh-merge")
                                .primary()
                                .small()
                                .icon(Lucide::GitMerge)
                                .label("Merge")
                                .disabled(self.working)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.merge(window, cx)),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        div()
                            .px_1p5()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(state_color)
                            .text_color(state_color)
                            .child(state_label),
                    )
                    .child(format!(
                        "#{} · {} opened {}",
                        item.number,
                        item.author,
                        ago(item.created_at, now())
                    ))
                    .when_some(self.pull.clone(), |el, p| {
                        el.child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(format!("{} → {}", p.head, p.base)),
                        )
                    }),
            )
            .child(self.render_tabs(cx))
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entries = match &self.events {
            Load::Ready(_) => Some(self.entries().len() + 1),
            _ => None,
        };
        let tab = |id: &'static str, which: Tab, current: Tab, cx: &App| {
            let active = which == current;
            h_flex()
                .id(id)
                .h(px(34.))
                .px_2()
                .gap_1p5()
                .text_sm()
                .cursor_pointer()
                .border_b_2()
                .border_color(if active {
                    cx.theme().primary
                } else {
                    transparent_black()
                })
                .text_color(if active {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                })
                .hover(|el| el.text_color(cx.theme().foreground))
        };
        let current = self.tab;
        let count = |n: String, cx: &App| div().text_color(cx.theme().muted_foreground).child(n);
        h_flex()
            .mt_1()
            .gap_2()
            .child(
                tab("gh-tab-summary", Tab::Summary, current, cx)
                    .child("Summary")
                    .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Summary, cx))),
            )
            .when(self.item.pull, |el| {
                let stats = self.pull.as_ref().map(|p| (p.additions, p.deletions));
                el.child(
                    tab("gh-tab-changes", Tab::Changes, current, cx)
                        .child("Changes")
                        .when_some(stats, |el, (add, del)| {
                            el.child(
                                div()
                                    .text_color(cx.theme().success)
                                    .child(format!("+{add}")),
                            )
                            .child(div().text_color(cx.theme().danger).child(format!("−{del}")))
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Changes, cx))),
                )
            })
            .child(
                tab("gh-tab-timeline", Tab::Timeline, current, cx)
                    .child("Timeline")
                    .when_some(entries, |el, n| el.child(count(n.to_string(), cx)))
                    .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Timeline, cx))),
            )
    }

    fn render_summary(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let facts: Vec<(&str, String)> = match &self.pull {
            Some(p) => vec![
                ("Branches", format!("{} → {}", p.head, p.base)),
                (
                    "Changes",
                    format!(
                        "{} file{}, +{} −{}",
                        p.changed_files,
                        if p.changed_files == 1 { "" } else { "s" },
                        p.additions,
                        p.deletions
                    ),
                ),
                (
                    "Mergeable",
                    match (p.merged, p.mergeable) {
                        (true, _) => "Already merged".into(),
                        (_, Some(true)) => "Yes, no conflicts".into(),
                        (_, Some(false)) => "No, it has conflicts with the base branch".into(),
                        (_, None) => "GitHub is still checking".into(),
                    },
                ),
            ],
            None => Vec::new(),
        };
        div()
            .id("gh-summary")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .when(!facts.is_empty(), |el| {
                        el.child(v_flex().gap_1().children(facts.into_iter().map(|(k, v)| {
                            h_flex()
                                .gap_3()
                                .text_sm()
                                .child(div().w(px(90.)).flex_none().text_color(muted).child(k))
                                .child(v)
                        })))
                    })
                    .child(card(
                        format!("gh-summary-body-{}", self.item.number).into(),
                        &self.item.body,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn render_changes(&self) -> AnyElement {
        match &self.diff {
            Some(Load::Ready(Some(diff))) => Diff::new(diff).size_full().into_any_element(),
            Some(Load::Ready(None)) => Placeholder::new(Lucide::File, "No file changes")
                .detail("This pull request doesn't change any files.")
                .into_any_element(),
            Some(Load::Failed(e)) => Placeholder::error("Unable to load the changes")
                .detail(e.clone())
                .into_any_element(),
            _ => Placeholder::loading("Loading the changes…").into_any_element(),
        }
    }

    fn render_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
        let events = match &self.events {
            Load::Loading => {
                return Placeholder::loading("Loading the timeline…").into_any_element();
            }
            Load::Failed(e) => {
                return Placeholder::error("Unable to load the timeline")
                    .detail(e.clone())
                    .action(
                        Button::new("gh-timeline-retry")
                            .small()
                            .label("Try Again")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .into_any_element();
            }
            Load::Ready(_) => self.entries(),
        };
        let count = |f: Filter| match f {
            // Commits, not pushes.
            Filter::Commits => events
                .iter()
                .map(|e| match e {
                    Entry::Pushed { commits, .. } => commits.len(),
                    _ => 0,
                })
                .sum(),
            _ => events.iter().filter(|e| f.keeps(e)).count() + usize::from(f == Filter::All),
        };
        let filter = self.filter;
        let chip = |id: &'static str,
                    label: &'static str,
                    which: Filter,
                    n: usize,
                    cx: &mut Context<Self>| {
            let active = filter == which;
            h_flex()
                .id(id)
                .px_2()
                .py_0p5()
                .gap_1p5()
                .rounded(cx.theme().radius)
                .text_sm()
                .cursor_pointer()
                .when(active, |el| {
                    el.bg(cx.theme().secondary)
                        .text_color(cx.theme().foreground)
                })
                .when(!active, |el| {
                    el.text_color(cx.theme().muted_foreground)
                        .hover(|el| el.text_color(cx.theme().foreground))
                })
                .child(label)
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(n.to_string()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = which;
                    cx.notify();
                }))
        };
        let mut chips = h_flex()
            .p_0p5()
            .gap_0p5()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(chip(
                "gh-filter-all",
                "All",
                Filter::All,
                count(Filter::All),
                cx,
            ))
            .child(chip(
                "gh-filter-comments",
                "Comments",
                Filter::Comments,
                count(Filter::Comments),
                cx,
            ));
        if self.item.pull {
            chips = chips
                .child(chip(
                    "gh-filter-reviews",
                    "Reviews",
                    Filter::Reviews,
                    count(Filter::Reviews),
                    cx,
                ))
                .child(chip(
                    "gh-filter-commits",
                    "Commits",
                    Filter::Commits,
                    count(Filter::Commits),
                    cx,
                ));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        if filter == Filter::All {
            rows.push(self.render_opened(cx));
        }
        for (ix, entry) in events.iter().enumerate().filter(|(_, e)| filter.keeps(e)) {
            rows.push(self.render_entry(ix, entry, cx));
        }
        let last = rows.len().saturating_sub(1);
        let rows: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| rail(row, i == last, cx))
            .collect();
        div()
            .id("gh-timeline")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(h_flex().child(chips))
                    .when(rows.is_empty(), |el| {
                        el.child(
                            div()
                                .py_6()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Nothing of this kind yet."),
                        )
                    })
                    .child(v_flex().children(rows))
                    .when(self.more, |el| {
                        el.child(
                            h_flex().child(
                                Button::new("gh-timeline-more")
                                    .small()
                                    .label("Load More")
                                    .loading(self.loading_more)
                                    .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_opened(&self, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.item;
        let (icon, what) = if item.pull {
            (Lucide::GitPullRequest, "opened this pull request")
        } else {
            (Lucide::CircleDot, "opened this issue")
        };
        let body = (!item.body.trim().is_empty()).then(|| {
            card(format!("gh-opened-{}", item.number).into(), &item.body, cx).into_any_element()
        });
        event_row(
            (icon, cx.theme().success),
            &item.author,
            what,
            item.created_at,
            body,
            cx,
        )
    }

    fn render_entry(&self, ix: usize, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        match entry {
            Entry::Pushed {
                author,
                time,
                commits,
            } => {
                let what = match commits.len() {
                    1 => "pushed 1 commit".to_string(),
                    n => format!("pushed {n} commits"),
                };
                let list = v_flex()
                    .gap_0p5()
                    .children(commits.iter().map(|(sha, message)| {
                        h_flex()
                            .gap_3()
                            .text_sm()
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(muted)
                                    .font_family(mono.clone())
                                    .child(sha.clone()),
                            )
                            .child(div().min_w_0().truncate().child(message.clone()))
                    }))
                    .into_any_element();
                event_row(
                    (Lucide::GitCommitHorizontal, muted),
                    author,
                    &what,
                    *time,
                    Some(list),
                    cx,
                )
            }
            Entry::Event(Event::Commented { author, time, body }) => {
                let body = card(
                    format!("gh-comment-{}-{ix}", self.item.number).into(),
                    body,
                    cx,
                )
                .into_any_element();
                event_row(
                    (Lucide::MessageSquare, muted),
                    author,
                    "commented",
                    *time,
                    Some(body),
                    cx,
                )
            }
            Entry::Event(Event::Reviewed {
                author,
                time,
                state,
                body,
                comments,
                ..
            }) => {
                let (icon, color, what) = match state {
                    ReviewState::Approved => (
                        Lucide::CircleCheck,
                        cx.theme().success,
                        "approved these changes",
                    ),
                    ReviewState::ChangesRequested => {
                        (Lucide::CircleAlert, cx.theme().danger, "requested changes")
                    }
                    ReviewState::Dismissed => {
                        (Lucide::CircleX, muted, "left a review that was dismissed")
                    }
                    ReviewState::Commented => (Lucide::Eye, muted, "reviewed"),
                };
                let content = v_flex()
                    .gap_2()
                    .when(!body.trim().is_empty(), |el| {
                        el.child(card(
                            format!("gh-review-{}-{ix}", self.item.number).into(),
                            body,
                            cx,
                        ))
                    })
                    .children(comments.iter().enumerate().map(|(c, comment)| {
                        inline_comment(
                            format!("gh-inline-{}-{ix}-{c}", self.item.number).into(),
                            comment,
                            cx,
                        )
                    }))
                    .into_any_element();
                let has_content = !body.trim().is_empty() || !comments.is_empty();
                event_row(
                    (icon, color),
                    author,
                    what,
                    *time,
                    has_content.then_some(content),
                    cx,
                )
            }
            Entry::Event(Event::Action {
                actor,
                time,
                kind,
                detail,
            }) => {
                let (icon, color, what) = match kind {
                    ActionKind::Merged => (
                        Lucide::GitMerge,
                        cx.theme().magenta,
                        format!("merged commit {detail}"),
                    ),
                    ActionKind::Closed => {
                        (Lucide::CircleX, cx.theme().danger, "closed this".into())
                    }
                    ActionKind::Reopened => (
                        Lucide::CircleDot,
                        cx.theme().success,
                        "reopened this".into(),
                    ),
                    ActionKind::Renamed => (
                        Lucide::Pencil,
                        muted,
                        format!("changed the title to “{detail}”"),
                    ),
                    ActionKind::Labeled => {
                        (Lucide::Tag, muted, format!("added the label {detail}"))
                    }
                    ActionKind::Unlabeled => {
                        (Lucide::Tag, muted, format!("removed the label {detail}"))
                    }
                    ActionKind::ReviewRequested => (
                        Lucide::Eye,
                        muted,
                        format!("requested a review from {detail}"),
                    ),
                    ActionKind::Assigned => (Lucide::UserPlus, muted, format!("assigned {detail}")),
                    ActionKind::ForcePushed => (
                        Lucide::GitCommitHorizontal,
                        cx.theme().warning,
                        "force-pushed the branch".into(),
                    ),
                    ActionKind::BranchDeleted => {
                        (Lucide::GitBranchMinus, muted, "deleted the branch".into())
                    }
                    ActionKind::ReadyForReview => {
                        (Lucide::Eye, muted, "marked this ready for review".into())
                    }
                    ActionKind::MarkedDraft => {
                        (Lucide::Pencil, muted, "marked this as a draft".into())
                    }
                    ActionKind::Referenced => {
                        (Lucide::Link, muted, format!("mentioned this in {detail}"))
                    }
                };
                event_row((icon, color), actor, &what, *time, None, cx)
            }
            // Folded into `Pushed` by `group`.
            Entry::Event(Event::Committed { .. }) => div().into_any_element(),
        }
    }
}

/// A timeline row: icon in a circle, avatar, who, what, when, and content.
fn event_row(
    (icon, color): (Lucide, Hsla),
    who: &str,
    what: &str,
    time: i64,
    content: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .flex_none()
                .size(px(28.))
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).small().text_color(color)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .child(
                    h_flex()
                        .h(px(28.))
                        .gap_2()
                        .text_sm()
                        .child(avatar(who, cx))
                        .child(div().font_semibold().child(who.to_string()))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(muted)
                                .child(what.to_string()),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(ago(time, now())),
                        ),
                )
                .children(content),
        )
        .into_any_element()
}

/// Draws the vertical line that joins the timeline's icons behind a row.
fn rail(row: AnyElement, last: bool, cx: &App) -> AnyElement {
    div()
        .relative()
        .pb_4()
        .when(!last, |el| {
            el.child(
                div()
                    .absolute()
                    .left(px(13.5))
                    .top(px(28.))
                    .bottom_0()
                    .w(px(1.))
                    .bg(cx.theme().border),
            )
        })
        .child(row)
        .into_any_element()
}

/// A round initial in a color picked from the name.
fn avatar(name: &str, cx: &App) -> impl IntoElement {
    let palette = [
        cx.theme().blue,
        cx.theme().green,
        cx.theme().yellow,
        cx.theme().magenta,
        cx.theme().cyan,
        cx.theme().red,
    ];
    let hash = name
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .flex_none()
        .size(px(18.))
        .rounded_full()
        .bg(palette[hash % palette.len()])
        .text_color(cx.theme().background)
        .text_xs()
        .font_semibold()
        .flex()
        .items_center()
        .justify_center()
        .child(initial)
}

/// Markdown in a bordered box.
fn card(id: SharedString, body: &str, cx: &App) -> impl IntoElement {
    div()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .text_sm()
        .child(if body.trim().is_empty() {
            div()
                .text_color(cx.theme().muted_foreground)
                .child("No description.")
                .into_any_element()
        } else {
            markdown(id, body)
        })
}

fn markdown(id: SharedString, body: &str) -> AnyElement {
    TextView::markdown(id, body.to_string())
        .selectable(true)
        .on_link_click(|url, _, _, cx| {
            if url.starts_with("https://") {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// A review comment on a line: the file and line, then the comment.
fn inline_comment(id: SharedString, comment: &ReviewComment, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .bg(cx.theme().secondary)
                .border_b_1()
                .border_color(cx.theme().border)
                .text_xs()
                .child(Icon::new(Lucide::FileCode).xsmall().text_color(muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(comment.path.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(muted)
                        .child(match comment.line {
                            Some(line) => format!("line {line}"),
                            None => "outdated".into(),
                        }),
                ),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .text_sm()
                .child(markdown(id, &comment.body)),
        )
        .into_any_element()
}

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
                            Textarea::new(&self.comment)
                                .h(px(72.))
                                .aria_label("Comment"),
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
