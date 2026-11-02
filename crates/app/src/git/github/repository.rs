//! The GitHub tab's This Repository page: the open repository's pull requests and issues, open
//! or closed, beside the one open in a [`ThreadView`].

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git;
use sawit_core::github::{self, Item, ago, now};

use super::{GitHubView, ITEM_HEIGHT, Load, Page};
use crate::git::thread::{ThreadEvent, ThreadView};
use crate::rt;
use crate::states::Placeholder;

impl GitHubView {
    pub(super) fn load_items(&mut self, cx: &mut Context<Self>) {
        let (Some(token), Some((owner, name))) = (self.token(cx), self.slug(cx)) else {
            return;
        };
        let open = self.show_open;
        self.items_for = Some((owner.clone(), name.clone(), open));
        self.items = Load::Loading;
        cx.notify();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move { github::items(&token, &owner, &name, open).await }
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.items = match result {
                    Ok(items) => Load::Ready(items),
                    Err(e) if Self::refused(&e, &token, cx) => Load::Idle,
                    Err(e) => Load::Failed(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn items(&self) -> &[Item] {
        match &self.items {
            Load::Ready(items) => items,
            _ => &[],
        }
    }

    pub(super) fn open_thread(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(token), Some(slug)) = (self.token(cx), self.slug(cx)) else {
            return;
        };
        let Some(item) = self.items().iter().find(|i| i.number == number).cloned() else {
            return;
        };
        self.composing = None;
        self.error = None;
        let thread = cx.new(|cx| ThreadView::new(item, slug, token, window, cx));
        cx.subscribe(&thread, |this, _, e: &ThreadEvent, cx| {
            this.on_thread_event(e, cx)
        })
        .detach();
        self.thread = Some(thread);
        cx.notify();
    }

    /// Keeps the list in step with what was done in the open thread.
    fn on_thread_event(&mut self, event: &ThreadEvent, cx: &mut Context<Self>) {
        let number = match event {
            ThreadEvent::StateChanged { number, .. }
            | ThreadEvent::Commented(number)
            | ThreadEvent::Merged(number) => *number,
        };
        if let Load::Ready(items) = &mut self.items
            && let Some(item) = items.iter_mut().find(|i| i.number == number)
        {
            match event {
                ThreadEvent::StateChanged { open, .. } => item.open = *open,
                ThreadEvent::Commented(_) => item.comments += 1,
                ThreadEvent::Merged(_) => item.open = false,
            }
        }
        if let ThreadEvent::Merged(number) = *event {
            // The merge commit is on the remote now.
            self.store.update(cx, |s, cx| {
                s.run("Fetching…", cx, move |repo, token| async move {
                    git::fetch(&repo, token.as_deref()).await?;
                    Ok(format!(
                        "Pull request #{number} was merged. Pull to bring it into your branch."
                    ))
                })
            });
        }
        cx.notify();
    }

    fn render_item_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.items().get(ix).cloned() else {
            return div().into_any_element();
        };
        let selected = self
            .thread
            .as_ref()
            .is_some_and(|t| t.read(cx).number() == item.number);
        let (icon, color) = match (item.pull, item.open) {
            (true, true) => (Lucide::GitPullRequest, cx.theme().success),
            (true, false) => (Lucide::GitMerge, cx.theme().muted_foreground),
            (false, true) => (Lucide::CircleDot, cx.theme().success),
            (false, false) => (Lucide::CircleCheck, cx.theme().muted_foreground),
        };
        let number = item.number;
        h_flex()
            .id(("gh-item", ix))
            .w_full()
            .h(px(ITEM_HEIGHT))
            .px_3()
            .gap_2()
            .cursor_pointer()
            .border_b_1()
            .border_color(cx.theme().border)
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_thread(number, window, cx)))
            .child(Icon::new(icon).small().text_color(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(item.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "#{} · {} · {}",
                                item.number,
                                item.author,
                                ago(item.updated_at, now())
                            )),
                    ),
            )
            .when(item.comments > 0, |el| {
                el.child(
                    h_flex()
                        .gap_0p5()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(Lucide::MessageSquare).xsmall())
                        .child(item.comments.to_string()),
                )
            })
            .into_any_element()
    }

    fn render_thread(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(pull) = self.composing {
            return self.render_compose(pull, cx);
        }
        let Some(thread) = &self.thread else {
            return Placeholder::new(Lucide::MessageSquare, "Select a pull request or issue")
                .detail("Its conversation shows here, and you can reply.")
                .into_any_element();
        };
        thread.clone().into_any_element()
    }

    pub(super) fn render_repository(&self, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        if store.slug.is_none() {
            let detail = if store.repo.is_none() {
                "Open a folder that's a GitHub repository, or clone one from Your Repositories."
            } else {
                "This repository's origin remote isn't on github.com."
            };
            return Placeholder::new(Lucide::Github, "No GitHub repository")
                .detail(detail)
                .action(
                    Button::new("gh-go-repos")
                        .small()
                        .label("Your Repositories")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::Repos;
                            this.ensure_loaded(cx);
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let show_open = self.show_open;
        let list: AnyElement = match &self.items {
            Load::Failed(e) => Placeholder::error("Unable to load from GitHub")
                .compact()
                .detail(e.clone())
                .action(
                    Button::new("gh-retry")
                        .small()
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.load_items(cx))),
                )
                .into_any_element(),
            Load::Ready(items) if items.is_empty() => Placeholder::new(
                Lucide::Check,
                if show_open {
                    "Nothing open"
                } else {
                    "Nothing closed"
                },
            )
            .compact()
            .detail(if show_open {
                "No open pull requests or issues."
            } else {
                "No closed pull requests or issues yet."
            })
            .into_any_element(),
            Load::Ready(items) => uniform_list(
                "gh-items",
                items.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| this.render_item_row(ix, cx)).collect()
                }),
            )
            .track_scroll(&self.item_scroll)
            .size_full()
            .into_any_element(),
            _ => Placeholder::loading("Loading from GitHub…")
                .compact()
                .into_any_element(),
        };
        let has_branch = store.status.as_ref().is_some_and(|s| s.branch.is_some());
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .w(px(340.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .gap_1()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("gh-show-open")
                                    .ghost()
                                    .xsmall()
                                    .label("Open")
                                    .selected(show_open)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_open = true;
                                        this.ensure_loaded(cx);
                                    })),
                            )
                            .child(
                                Button::new("gh-show-closed")
                                    .ghost()
                                    .xsmall()
                                    .label("Closed")
                                    .selected(!show_open)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_open = false;
                                        this.ensure_loaded(cx);
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("gh-new-issue")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::CircleDot)
                                    .tooltip("New Issue")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.compose(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("gh-new-pull")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::GitPullRequest)
                                    .tooltip("New Pull Request from This Branch")
                                    .disabled(!has_branch)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.compose(true, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("gh-refresh")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::RefreshCw)
                                    .tooltip("Refresh")
                                    .on_click(cx.listener(|this, _, _, cx| this.load_items(cx))),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(list)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_thread(cx)),
            )
            .into_any_element()
    }
}
