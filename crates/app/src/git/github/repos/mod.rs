//! The GitHub tab's Your Repositories page, laid out as the catalog pages are: your repositories
//! searched, narrowed by a filter tab and by owner, sorted and shown a page at a time, each
//! cloned into a folder of your choice and opened.

mod filter;
mod row;
mod toolbar;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git;
use sawit_core::github::{self, Repo};

pub(super) use filter::{Filter, Sort};

use super::{GitHubView, Load, Page};
use crate::catalog;
use crate::rt;
use crate::states::{Banner, Placeholder};

impl GitHubView {
    pub(super) fn load_repos(&mut self, cx: &mut Context<Self>) {
        let Some(token) = self.token(cx) else {
            return;
        };
        self.repos = Load::Loading;
        cx.notify();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move { github::repos(&token).await }
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.repos = match result {
                    Ok(repos) => {
                        // An owner picked whose repositories are all gone would hide every row.
                        if let Some(owner) = &this.repo_owner
                            && !repos.iter().any(|r| &r.owner == owner)
                        {
                            this.repo_owner = None;
                        }
                        Load::Ready(repos)
                    }
                    Err(e) if Self::refused(&e, &token, cx) => Load::Idle,
                    Err(e) => Load::Failed(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn clone_repo(&mut self, repo: Repo, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Clone Here".into()),
        });
        let handle = window.window_handle();
        let token = self.token(cx);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            let name = repo
                .full_name
                .rsplit('/')
                .next()
                .unwrap_or(&repo.full_name)
                .to_string();
            let target = parent.join(&name);
            let Ok(rx) = this.update(cx, |this, cx| {
                this.cloning = Some(repo.full_name.clone());
                this.error = None;
                cx.notify();
                let (url, target) = (repo.clone_url.clone(), target.clone());
                rt::spawn(cx, async move {
                    if target.exists() {
                        return Err(sawit_core::CoreError::Git(format!(
                            "{} already exists. Pick another folder.",
                            target.display()
                        )));
                    }
                    git::clone(&url, &target, token.as_deref()).await
                })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.cloning = None;
                match result {
                    Ok(Ok(())) => {
                        cx.defer(move |cx| crate::cli::open_folder_in(target, Some(handle), cx))
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The search as typed, without the spaces around it.
    fn repo_query(&self, cx: &App) -> String {
        self.repo_search.read(cx).value().trim().to_string()
    }

    /// Clear Filters: no search, the All tab and every owner, from the first page.
    fn clear_repo_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.repo_filter = Filter::All;
        self.repo_owner = None;
        self.repo_paging.page = 0;
        self.repo_search
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    /// The page: a clone's failure on top, then the toolbar and the list card, scrolling as a
    /// whole. Until there are repositories, what stands in for the card fills the rest.
    pub(super) fn render_repos(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.repo_query(cx);
        let fill = |state: Placeholder| {
            v_flex()
                .flex_1()
                .justify_center()
                .child(state)
                .into_any_element()
        };
        let body = match &self.repos {
            Load::Ready(repos) if !repos.is_empty() => self.render_repo_list(repos, &query, cx),
            Load::Ready(_) => fill(
                Placeholder::new(Lucide::BookMarked, "No repositories")
                    .detail("Repositories you own or collaborate on show here."),
            ),
            Load::Failed(e) => fill(
                Placeholder::error("Unable to load your repositories")
                    .detail(e.clone())
                    .action(
                        Button::new("gh-repos-retry")
                            .small()
                            .label("Try Again")
                            .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                    ),
            ),
            Load::Idle | Load::Loading => fill(Placeholder::loading("Loading your repositories…")),
        };
        v_flex()
            .size_full()
            .when_some(
                self.error.clone().filter(|_| self.page == Page::Repos),
                |el, e| el.child(Banner::error("gh-repos-error", e)),
            )
            .child(
                div()
                    .id("gh-repos-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .min_h_full()
                            .p_6()
                            .gap_4()
                            .child(self.render_repo_toolbar(&query, cx))
                            .child(body),
                    ),
            )
            .into_any_element()
    }

    /// The list card: column titles, a page of `repos` as the search, tab, owner and sort leave
    /// them, and the pager; or, when they leave none, the empty state.
    fn render_repo_list(&self, repos: &[Repo], query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let scope = filter::in_scope(repos, query, self.repo_owner.as_deref());
        let rows = filter::shown(scope, self.repo_filter, self.repo_sort);
        let total = rows.len();
        let card = v_flex()
            .debug_selector(|| "gh-repos-list".into())
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = if query.is_empty() {
                "No repositories here".to_string()
            } else {
                format!("No repositories match “{query}”")
            };
            return card
                .child(catalog::empty(
                    title,
                    Button::new("gh-repos-clear-filters")
                        .outline()
                        .small()
                        .icon(Lucide::FunnelX)
                        .label("Clear Filters")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.clear_repo_filters(window, cx)),
                        ),
                    cx,
                ))
                .into_any_element();
        }
        let rows: Vec<_> = rows.into_iter().enumerate().collect();
        let (rows, paging) = catalog::page_of(rows, self.repo_paging);
        card.child(catalog::header(None, "Repository", cx))
            .children(
                rows.into_iter()
                    .map(|(ix, repo)| self.render_repo_row(ix, repo, cx)),
            )
            .child(catalog::pager(
                "gh-repos",
                paging,
                total,
                if total == 1 {
                    "repository"
                } else {
                    "repositories"
                },
                None,
                cx.listener(|this, paging: &catalog::Paging, _, cx| {
                    this.repo_paging = *paging;
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
