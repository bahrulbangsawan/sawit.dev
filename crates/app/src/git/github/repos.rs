//! The GitHub tab's Your Repositories page: your repositories, searched, each cloned into a
//! folder of your choice and opened.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git;
use sawit_core::github::{self, Repo, ago, now};

use super::{GitHubView, ITEM_HEIGHT, Load, Page};
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
                    Ok(repos) => Load::Ready(repos),
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

    fn filtered_repos(&self, cx: &App) -> Vec<Repo> {
        let query = self.repo_search.read(cx).value().to_lowercase();
        match &self.repos {
            Load::Ready(repos) => repos
                .iter()
                .filter(|r| {
                    query.is_empty()
                        || r.full_name.to_lowercase().contains(&query)
                        || r.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&query))
                })
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn render_repo_row(&self, ix: usize, repo: Repo, cx: &mut Context<Self>) -> AnyElement {
        let cloning = self.cloning.as_deref() == Some(repo.full_name.as_str());
        let busy = self.cloning.is_some();
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(("gh-repo", ix))
            .w_full()
            .h(px(ITEM_HEIGHT))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Icon::new(if repo.private {
                    Lucide::Lock
                } else {
                    Lucide::BookMarked
                })
                .small()
                .text_color(muted),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(repo.full_name.clone()))
                    .child(div().text_xs().truncate().text_color(muted).child(
                        match &repo.description {
                            Some(d) => format!("{d} · pushed {}", ago(repo.pushed_at, now())),
                            None => format!("Pushed {}", ago(repo.pushed_at, now())),
                        },
                    )),
            )
            .child(
                Button::new(("gh-clone", ix))
                    .small()
                    .icon(Lucide::Download)
                    .label(if cloning { "Cloning…" } else { "Clone…" })
                    .loading(cloning)
                    .disabled(busy)
                    .tooltip("Clone into a Folder and Open It")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.clone_repo(repo.clone(), window, cx)
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_repos(&self, cx: &mut Context<Self>) -> AnyElement {
        let repos = self.filtered_repos(cx);
        let count = repos.len();
        let list: AnyElement = match &self.repos {
            Load::Failed(e) => Placeholder::error("Unable to load your repositories")
                .detail(e.clone())
                .action(
                    Button::new("gh-repos-retry")
                        .small()
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                )
                .into_any_element(),
            Load::Ready(all) if all.is_empty() => {
                Placeholder::new(Lucide::BookMarked, "No repositories")
                    .detail("Repositories you own or collaborate on show here.")
                    .into_any_element()
            }
            Load::Ready(_) if count == 0 => {
                Placeholder::new(IconName::Search, "No matching repositories")
                    .detail("Nothing matches your search.")
                    .into_any_element()
            }
            Load::Ready(_) => uniform_list(
                "gh-repos",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let repos = this.filtered_repos(cx);
                    range
                        .filter_map(|ix| Some(this.render_repo_row(ix, repos.get(ix)?.clone(), cx)))
                        .collect()
                }),
            )
            .track_scroll(&self.repo_scroll)
            .size_full()
            .into_any_element(),
            _ => Placeholder::loading("Loading your repositories…").into_any_element(),
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.repo_search).small().cleanable(true)),
                    )
                    .child(
                        Button::new("gh-repos-refresh")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.load_repos(cx))),
                    ),
            )
            .when_some(
                self.error.clone().filter(|_| self.page == Page::Repos),
                |el, e| el.child(Banner::error("gh-repos-error", e)),
            )
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}
