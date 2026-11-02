//! The Git sidebar's branch bar: the branch menu (switch, create, merge into the current branch)
//! and fetch, pull and push.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git;

use super::GitPanel;

impl GitPanel {
    fn sync(&mut self, which: Sync, cx: &mut Context<Self>) {
        let has_upstream = self
            .store
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.upstream.is_some());
        self.store.update(cx, |s, cx| match which {
            Sync::Fetch => s.run("Fetching…", cx, |repo, token| async move {
                git::fetch(&repo, token.as_deref())
                    .await
                    .map(|_| String::new())
            }),
            Sync::Pull => s.run("Pulling…", cx, |repo, token| async move {
                git::pull(&repo, token.as_deref()).await.map(|out| {
                    if out.contains("Already up to date") {
                        "Already up to date.".into()
                    } else {
                        String::new()
                    }
                })
            }),
            Sync::Push => s.run("Pushing…", cx, move |repo, token| async move {
                git::push(&repo, has_upstream, token.as_deref())
                    .await
                    .map(|out| {
                        if out.contains("Everything up-to-date") {
                            "Everything is already pushed.".into()
                        } else {
                            String::new()
                        }
                    })
            }),
        });
    }

    pub(super) fn render_branch_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let status = store.status.clone().unwrap_or_default();
        let busy = store.busy.is_some();
        let branch_label = status
            .branch
            .clone()
            .or_else(|| status.head.clone().map(|h| format!("{h} (detached)")))
            .unwrap_or_else(|| "main".into());
        let branches = store.branches.clone();
        let current = status.branch.clone();
        let weak = cx.entity().downgrade();
        let menu_branches = branches.clone();
        let menu_current = current.clone();
        let branch_button = Button::new("git-branch")
            .ghost()
            .xsmall()
            .icon(Lucide::GitBranch)
            .label(branch_label)
            .tooltip("Switch, Create or Merge Branches")
            .disabled(busy)
            .dropdown_menu(move |menu, window, cx| {
                branch_menu(
                    menu,
                    &menu_branches,
                    menu_current.as_deref(),
                    weak.clone(),
                    window,
                    cx,
                )
            });
        let (ahead, behind) = (status.ahead, status.behind);
        let has_upstream = status.upstream.is_some();
        h_flex()
            .px_1()
            .py_1()
            .gap_0p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .justify_start()
                    .child(branch_button),
            )
            .child(
                Button::new("git-fetch")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Fetch from All Remotes")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.sync(Sync::Fetch, cx))),
            )
            .child(
                Button::new("git-pull")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ArrowDown)
                    .when(behind > 0, |b| b.label(behind.to_string()))
                    .tooltip(if behind > 0 {
                        format!("Pull {behind} Commit{}", if behind == 1 { "" } else { "s" })
                    } else {
                        "Pull".to_string()
                    })
                    .disabled(busy || !has_upstream)
                    .on_click(cx.listener(|this, _, _, cx| this.sync(Sync::Pull, cx))),
            )
            .child(
                Button::new("git-push")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ArrowUp)
                    .when(ahead > 0, |b| b.label(ahead.to_string()))
                    .tooltip(if !has_upstream {
                        "Publish Branch to origin".to_string()
                    } else if ahead > 0 {
                        format!("Push {ahead} Commit{}", if ahead == 1 { "" } else { "s" })
                    } else {
                        "Push".to_string()
                    })
                    .disabled(busy || status.head.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.sync(Sync::Push, cx))),
            )
    }
}

#[derive(Clone, Copy)]
enum Sync {
    Fetch,
    Pull,
    Push,
}

fn branch_menu(
    menu: PopupMenu,
    branches: &[git::Branch],
    current: Option<&str>,
    panel: WeakEntity<GitPanel>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let locals: Vec<_> = branches.iter().filter(|b| !b.remote).cloned().collect();
    // Remote branches that don't have a local branch yet.
    let remotes: Vec<_> = branches
        .iter()
        .filter(|b| {
            b.remote
                && !locals
                    .iter()
                    .any(|l| b.name.split_once('/').is_some_and(|(_, n)| n == l.name))
        })
        .cloned()
        .collect();
    let mergeable: Vec<_> = branches
        .iter()
        .filter(|b| Some(b.name.as_str()) != current)
        .map(|b| b.name.clone())
        .collect();
    let switch_item = |b: git::Branch| {
        let panel = panel.clone();
        let label = b.name.clone();
        PopupMenuItem::new(label)
            .checked(b.current)
            .on_click(move |_, _, cx| {
                let b = b.clone();
                let _ = panel.update(cx, |this, cx| {
                    this.store.update(cx, |s, cx| {
                        s.run("Switching branch…", cx, move |repo, _| async move {
                            git::switch(&repo, &b).await.map(|_| String::new())
                        })
                    })
                });
            })
    };
    let mut menu = menu.max_h(px(420.)).scrollable(true).label("Switch to");
    for b in locals {
        menu = menu.item(switch_item(b));
    }
    if !remotes.is_empty() {
        let remotes = remotes.clone();
        let panel_for_remote = panel.clone();
        menu = menu.submenu("Remote Branches", window, cx, move |mut menu, _, _| {
            for b in remotes.clone() {
                let panel = panel_for_remote.clone();
                menu = menu.item(
                    PopupMenuItem::new(b.name.clone()).on_click(move |_, _, cx| {
                        let b = b.clone();
                        let _ = panel.update(cx, |this, cx| {
                            this.store.update(cx, |s, cx| {
                                s.run("Switching branch…", cx, move |repo, _| async move {
                                    git::switch(&repo, &b).await.map(|_| String::new())
                                })
                            })
                        });
                    }),
                );
            }
            menu
        });
    }
    menu = menu.separator();
    let panel_new = panel.clone();
    menu = menu.item(
        PopupMenuItem::new("New Branch…")
            .icon(Lucide::Plus)
            .on_click(move |_, window, cx| {
                let _ = panel_new.update(cx, |this, cx| {
                    this.creating_branch = true;
                    this.new_branch.update(cx, |s, cx| s.focus(window, cx));
                    cx.notify();
                });
            }),
    );
    if let Some(current) = current
        && !mergeable.is_empty()
    {
        let panel_merge = panel.clone();
        menu = menu.submenu(
            format!("Merge into {current}"),
            window,
            cx,
            move |mut menu, _, _| {
                for name in mergeable.clone() {
                    let panel = panel_merge.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, cx| {
                            let name = name.clone();
                            let _ = panel.update(cx, |this, cx| this.merge(name, window, cx));
                        },
                    ));
                }
                menu
            },
        );
    }
    menu
}

impl GitPanel {
    fn merge(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .store
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.branch.clone())
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Merge {branch} into {current}?"),
            Some("Its commits are added to your current branch. If both changed the same lines, you'll resolve the conflicts before committing."),
            &["Merge", "Cancel"],
            cx,
        );
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            if answer.await != Ok(0) {
                return;
            }
            store.update(cx, |s, cx| {
                s.run("Merging…", cx, move |repo, _| async move {
                    let out = git::merge(&repo, &branch).await?;
                    if out.ok {
                        Ok(if out.stdout.contains("Already up to date") {
                            format!("{current} already has everything in {branch}.")
                        } else {
                            format!("Merged {branch} into {current}.")
                        })
                    } else if out.stdout.contains("CONFLICT") {
                        Err(sawit_core::CoreError::Git(format!(
                            "Merging {branch} has conflicts. Open each conflicted file, fix it, stage it, then commit. Or abort the merge."
                        )))
                    } else {
                        Err(sawit_core::CoreError::Git(out.message()))
                    }
                })
            });
        })
        .detach();
    }
}
