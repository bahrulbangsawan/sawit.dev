//! Git sidebar: branch and sync (fetch, pull, push), the commit box, and
//! staged / unstaged / conflicted files. Clicking a file opens its diff.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, FileChange};

use super::store::GitStore;
use crate::states::{Banner, Placeholder};

pub enum GitPanelEvent {
    Diff { file: FileChange, staged: bool },
    Graph,
    GitHub,
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

const ROW_HEIGHT: f32 = 26.;

#[derive(Clone)]
enum Row {
    Header(Section, usize),
    File(Section, FileChange),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Conflicts,
    Staged,
    Changes,
}

pub struct GitPanel {
    store: Entity<GitStore>,
    message: Entity<TextareaState>,
    new_branch: Entity<InputState>,
    creating_branch: bool,
    amend: bool,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl GitPanel {
    pub fn new(store: Entity<GitStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Commit message (⌘↩ to commit)")
        });
        let new_branch = cx.new(|cx| InputState::new(window, cx).placeholder("New branch name"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&message, window, |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { secondary: true, .. } => this.commit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe_in(&new_branch, window, |this, input, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    let name = input.read(cx).value().trim().to_string();
                    if !name.is_empty() {
                        this.store.update(cx, |s, cx| {
                            s.run("Creating branch…", cx, move |repo, _| async move {
                                git::create_branch(&repo, &name).await?;
                                Ok(format!("Switched to the new branch {name}."))
                            })
                        });
                    }
                    this.creating_branch = false;
                    input.update(cx, |s, cx| s.set_value("", window, cx));
                    cx.notify();
                }
            }),
        ];
        Self {
            store,
            message,
            new_branch,
            creating_branch: false,
            amend: false,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let Some(status) = &self.store.read(cx).status else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (section, files) in [
            (Section::Conflicts, status.conflicted().cloned().collect::<Vec<_>>()),
            (Section::Staged, status.staged().cloned().collect()),
            (Section::Changes, status.unstaged().cloned().collect()),
        ] {
            if files.is_empty() {
                continue;
            }
            rows.push(Row::Header(section, files.len()));
            rows.extend(files.into_iter().map(|f| Row::File(section, f)));
        }
        rows
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_string();
        let Some(status) = self.store.read(cx).status.clone() else {
            return;
        };
        if message.is_empty() {
            self.store.update(cx, |s, cx| {
                s.error = Some("Write a commit message first.".into());
                cx.notify();
            });
            return;
        }
        let amend = self.amend;
        // Like VS Code: with nothing staged, commit every change.
        let stage_all = status.staged().next().is_none() && !amend;
        if stage_all && status.unstaged().next().is_none() {
            self.store.update(cx, |s, cx| {
                s.error = Some("There's nothing to commit.".into());
                cx.notify();
            });
            return;
        }
        self.store.update(cx, |s, cx| {
            s.run("Committing…", cx, move |repo, _| async move {
                if stage_all {
                    git::stage_all(&repo).await?;
                }
                git::commit(&repo, &message, amend).await?;
                Ok(String::new())
            })
        });
        self.amend = false;
        self.message.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn toggle_amend(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = on;
        cx.notify();
        if !on || !self.message.read(cx).value().trim().is_empty() {
            return;
        }
        let Some(repo) = self.store.read(cx).repo.clone() else {
            return;
        };
        let rx = crate::rt::spawn(cx, async move { git::last_message(&repo).await });
        let message = self.message.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(text)) = rx.await {
                let _ = message.update_in(cx, |s, window, cx| {
                    if s.value().trim().is_empty() {
                        s.set_value(text, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn stage(&mut self, file: &FileChange, cx: &mut Context<Self>) {
        let path = file.path.clone();
        self.store.update(cx, |s, cx| {
            s.run("Staging…", cx, move |repo, _| async move {
                git::stage(&repo, &[path]).await.map(|_| String::new())
            })
        });
    }

    fn unstage(&mut self, file: &FileChange, cx: &mut Context<Self>) {
        let mut paths = vec![file.path.clone()];
        paths.extend(file.from.clone());
        self.store.update(cx, |s, cx| {
            s.run("Unstaging…", cx, move |repo, _| async move {
                git::unstage(&repo, &paths).await.map(|_| String::new())
            })
        });
    }

    fn discard(&mut self, file: FileChange, window: &mut Window, cx: &mut Context<Self>) {
        let (title, button) = if file.untracked() {
            (format!("Delete “{}”?", file.path), "Delete File")
        } else {
            (format!("Discard changes to “{}”?", file.path), "Discard Changes")
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some("This can't be undone."),
            &[button, "Cancel"],
            cx,
        );
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                store.update(cx, |s, cx| {
                    s.run("Discarding…", cx, move |repo, _| async move {
                        git::discard(&repo, &file).await.map(|_| String::new())
                    })
                });
            }
        })
        .detach();
    }

    fn sync(&mut self, which: Sync, cx: &mut Context<Self>) {
        let has_upstream = self
            .store
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.upstream.is_some());
        self.store.update(cx, |s, cx| match which {
            Sync::Fetch => s.run("Fetching…", cx, |repo, token| async move {
                git::fetch(&repo, token.as_deref()).await.map(|_| String::new())
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
                git::push(&repo, has_upstream, token.as_deref()).await.map(|out| {
                    if out.contains("Everything up-to-date") {
                        "Everything is already pushed.".into()
                    } else {
                        String::new()
                    }
                })
            }),
        });
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.store.read(cx).busy.is_some();
        match row {
            Row::Header(section, count) => {
                let (label, action): (&str, Option<(&str, &str, bool)>) = match section {
                    Section::Conflicts => ("MERGE CONFLICTS", None),
                    Section::Staged => ("STAGED", Some(("Unstage All", "unstage-all", false))),
                    Section::Changes => ("CHANGES", Some(("Stage All", "stage-all", true))),
                };
                h_flex()
                    .id(("git-header", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child(div().flex_1().child(format!("{label} · {count}")))
                    .when_some(action, |el, (tooltip, id, stage)| {
                        el.child(
                            Button::new(id)
                                .ghost()
                                .xsmall()
                                .disabled(busy)
                                .icon(if stage { Lucide::Plus } else { Lucide::Minus })
                                .tooltip(tooltip)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.store.update(cx, |s, cx| {
                                        s.run(if stage { "Staging…" } else { "Unstaging…" }, cx, move |repo, _| async move {
                                            if stage {
                                                git::stage_all(&repo).await?;
                                            } else {
                                                git::unstage_all(&repo).await?;
                                            }
                                            Ok(String::new())
                                        })
                                    })
                                })),
                        )
                    })
                    .into_any_element()
            }
            Row::File(section, file) => {
                let section = *section;
                let staged = section == Section::Staged;
                let letter = if file.conflicted {
                    '!'
                } else if staged {
                    file.index
                } else if file.untracked() {
                    'U'
                } else {
                    file.worktree
                };
                let color = match letter {
                    'A' | 'U' => cx.theme().success,
                    'D' | '!' => cx.theme().danger,
                    'R' | 'C' => cx.theme().info,
                    _ => cx.theme().warning,
                };
                let what = match letter {
                    'A' => "Added",
                    'U' => "Untracked",
                    'D' => "Deleted",
                    'R' => "Renamed",
                    'C' => "Copied",
                    '!' => "Conflicted",
                    _ => "Modified",
                };
                let (name, folder) = match file.path.rsplit_once('/') {
                    Some((dir, name)) => (name.to_string(), dir.to_string()),
                    None => (file.path.clone(), String::new()),
                };
                let (open, act, discard) = (file.clone(), file.clone(), file.clone());
                let group: SharedString = format!("git-row-{ix}").into();
                h_flex()
                    .id(("git-file", ix))
                    .group(group.clone())
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .pl_3()
                    .pr_1()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .tooltip({
                        let path = file.path.clone();
                        move |window, cx| Tooltip::new(format!("{path} · {what}")).build(window, cx)
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(GitPanelEvent::Diff {
                            file: open.clone(),
                            staged,
                        })
                    }))
                    .child(crate::file_icons::file_icon(std::path::Path::new(&file.path), cx))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().flex_none().max_w_full().truncate().child(name))
                            .child(div().min_w_0().truncate().text_xs().text_color(muted).child(folder)),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .when(section == Section::Changes, |el| {
                                el.child(
                                    Button::new(("git-discard", ix))
                                        .ghost()
                                        .xsmall()
                                        .disabled(busy)
                                        .icon(Lucide::Undo2)
                                        .tooltip(if discard.untracked() { "Delete File" } else { "Discard Changes" })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.discard(discard.clone(), window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new(("git-stage", ix))
                                    .ghost()
                                    .xsmall()
                                    .disabled(busy)
                                    .icon(if staged { Lucide::Minus } else { Lucide::Plus })
                                    .tooltip(if staged {
                                        "Unstage"
                                    } else if section == Section::Conflicts {
                                        "Mark as Resolved"
                                    } else {
                                        "Stage"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if staged {
                                            this.unstage(&act, cx);
                                        } else {
                                            this.stage(&act, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_xs()
                            .font_semibold()
                            .text_color(color)
                            .child(letter.to_string()),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_branch_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                branch_menu(menu, &menu_branches, menu_current.as_deref(), weak.clone(), window, cx)
            });
        let (ahead, behind) = (status.ahead, status.behind);
        let has_upstream = status.upstream.is_some();
        h_flex()
            .px_1()
            .py_1()
            .gap_0p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(h_flex().flex_1().min_w_0().overflow_hidden().justify_start().child(branch_button))
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

    fn render_commit_box(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let status = store.status.clone().unwrap_or_default();
        let busy = store.busy.is_some();
        let staged = status.staged().count();
        let changed = status.unstaged().count();
        let conflicts = status.conflicted().count();
        let label = if self.amend {
            "Amend Last Commit".to_string()
        } else if staged > 0 {
            format!("Commit {staged} File{}", if staged == 1 { "" } else { "s" })
        } else if changed > 0 {
            format!("Commit All {changed}")
        } else {
            "Commit".to_string()
        };
        let empty = self.message.read(cx).value().trim().is_empty();
        v_flex()
            .p_2()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Textarea::new(&self.message).h(px(64.)).aria_label("Commit message"))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("git-amend")
                            .label("Amend")
                            .checked(self.amend)
                            .tooltip("Replace the last commit instead of adding one")
                            .disabled(status.head.is_none())
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.toggle_amend(*checked, window, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("git-commit")
                            .primary()
                            .small()
                            .icon(Lucide::Check)
                            .label(label)
                            .disabled(busy || empty || conflicts > 0 || (staged + changed == 0 && !self.amend))
                            .tooltip(if conflicts > 0 {
                                "Resolve the conflicts first"
                            } else {
                                "Commit (⌘↩)"
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
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
                menu = menu.item(PopupMenuItem::new(b.name.clone()).on_click(move |_, _, cx| {
                    let b = b.clone();
                    let _ = panel.update(cx, |this, cx| {
                        this.store.update(cx, |s, cx| {
                            s.run("Switching branch…", cx, move |repo, _| async move {
                                git::switch(&repo, &b).await.map(|_| String::new())
                            })
                        })
                    });
                }));
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
        menu = menu.submenu(format!("Merge into {current}"), window, cx, move |mut menu, _, _| {
            for name in mergeable.clone() {
                let panel = panel_merge.clone();
                menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, window, cx| {
                    let name = name.clone();
                    let _ = panel.update(cx, |this, cx| this.merge(name, window, cx));
                }));
            }
            menu
        });
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

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let header = h_flex()
            .h(px(32.))
            .px_2()
            .gap_0p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(match &store.repo {
                        Some(repo) => format!(
                            "GIT · {}",
                            repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                        ),
                        None => "GIT".into(),
                    }),
            )
            .when(store.repo.is_some(), |el| {
                el.child(
                    Button::new("git-open-graph")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::GitGraph)
                        .tooltip("Commit Graph")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::Graph))),
                )
            })
            .child(
                Button::new("git-open-github")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Github)
                    .tooltip(match &store.account {
                        Some(a) => format!("GitHub · @{}", a.login),
                        None => "GitHub · Sign In".to_string(),
                    })
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::GitHub))),
            );

        let body: AnyElement = if store.repo.is_none() {
            let state = if !store.checked {
                Placeholder::loading("Looking for a repository…").compact()
            } else if store.dir().is_none() {
                Placeholder::new(Lucide::GitBranch, "No folder open")
                    .compact()
                    .detail("Open a project folder to see its changes, or clone one of your GitHub repositories.")
                    .action(
                        Button::new("git-open-folder")
                            .small()
                            .icon(Lucide::FolderOpen)
                            .label("Open Folder…")
                            .on_click(|_, window, cx| {
                                crate::cli::prompt_open_folder(Some(window.window_handle()), cx)
                            }),
                    )
                    .action(
                        Button::new("git-clone")
                            .small()
                            .icon(Lucide::Github)
                            .label("Clone from GitHub…")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::GitHub))),
                    )
            } else {
                Placeholder::new(Lucide::GitBranch, "Not a Git repository")
                    .compact()
                    .detail("Start tracking this folder's history with Git.")
                    .action(
                        Button::new("git-init")
                            .small()
                            .primary()
                            .icon(Lucide::Plus)
                            .label("Initialize Repository")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.update(cx, |s, cx| s.init_repo(cx))
                            })),
                    )
            };
            state.into_any_element()
        } else {
            // Built once per render and shared with the virtual list.
            let rows = std::rc::Rc::new(self.rows(cx));
            let count = rows.len();
            let list_rows = rows.clone();
            let merging = store.status.as_ref().is_some_and(|s| s.merging);
            let busy = store.busy;
            let message = store
                .error
                .clone()
                .map(|e| (e, true))
                .or_else(|| store.notice.clone().map(|n| (n, false)));
            v_flex()
                .flex_1()
                .min_h_0()
                .child(self.render_branch_bar(cx))
                .when(self.creating_branch, |el| {
                    el.child(
                        h_flex()
                            .p_2()
                            .gap_1()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().flex_1().child(Input::new(&self.new_branch).small()))
                            .child(
                                Button::new("git-cancel-branch")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .tooltip("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.creating_branch = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .when(merging, |el| {
                    el.child(
                        Banner::info("git-merging", "A merge is in progress. Resolve the conflicts, stage the files, then commit.")
                            .action(
                                Button::new("git-merge-abort")
                                    .xsmall()
                                    .label("Abort Merge")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.store.update(cx, |s, cx| {
                                            s.run("Aborting merge…", cx, |repo, _| async move {
                                                git::merge_abort(&repo).await?;
                                                Ok("The merge was aborted.".into())
                                            })
                                        })
                                    })),
                            ),
                    )
                })
                .when_some(busy, |el, label| {
                    el.child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(Spinner::new().xsmall())
                            .child(label),
                    )
                })
                .when_some(message, |el, (text, error)| {
                    let banner = if error {
                        Banner::error("git-message", text)
                    } else {
                        Banner::info("git-message", text)
                    };
                    el.child(banner.on_dismiss(cx.listener(|this, _, _, cx| {
                        this.store.update(cx, |s, cx| s.dismiss_message(cx))
                    })))
                })
                .child(self.render_commit_box(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .when(count > 0, |el| {
                            el.child(
                                uniform_list(
                                    "git-rows",
                                    count,
                                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                        let rows = list_rows.clone();
                                        range
                                            .filter_map(|ix| Some(this.render_row(ix, rows.get(ix)?, cx)))
                                            .collect()
                                    }),
                                )
                                .track_scroll(&self.scroll)
                                .size_full(),
                            )
                        })
                        .when(count == 0, |el| {
                            el.child(
                                Placeholder::new(Lucide::Check, "No changes")
                                    .compact()
                                    .detail("Edits to files in this repository show up here."),
                            )
                        }),
                )
                .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(header)
            .child(body)
    }
}
