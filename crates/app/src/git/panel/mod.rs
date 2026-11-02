//! Git sidebar: branch and sync (fetch, pull, push), the commit box, and
//! staged / unstaged / conflicted files. Clicking a file opens its diff.

mod branches;
mod commit;
mod files;

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, FileChange};

use super::account::{self, GitHubAccount};
use super::store::GitStore;
use crate::states::{Banner, Placeholder};
pub(super) use commit::commit_placeholder;
use files::Row;

pub enum GitPanelEvent {
    Diff { file: FileChange, staged: bool },
    Graph,
    GitHub,
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

pub struct GitPanel {
    store: Entity<GitStore>,
    account: Entity<GitHubAccount>,
    message: Entity<TextareaState>,
    new_branch: Entity<InputState>,
    creating_branch: bool,
    amend: bool,
    scroll: UniformListScrollHandle,
    /// The file list, built from the store's status whenever the store changes rather than on
    /// every render: each keystroke in the commit box renders the panel again.
    rows: Rc<Vec<Row>>,
    _subscriptions: Vec<Subscription>,
}

impl GitPanel {
    pub fn new(store: Entity<GitStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message =
            cx.new(|cx| TextareaState::new(window, cx).placeholder(commit_placeholder(cx)));
        let new_branch = cx.new(|cx| InputState::new(window, cx).placeholder("New branch name"));
        let account = account::global(cx);
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| {
                this.rows = Rc::new(this.build_rows(cx));
                cx.notify()
            }),
            // Clone from GitHub turns on and off with the one sign-in.
            cx.observe(&account, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &message,
                window,
                // ⌘↩ commits through its binding (`super::Commit`), which the keymap may change.
                |_, _, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Change) {
                        cx.notify()
                    }
                },
            ),
            cx.subscribe_in(
                &new_branch,
                window,
                |this, input, e: &InputEvent, window, cx| {
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
                },
            ),
        ];
        let mut this = Self {
            store,
            account,
            message,
            new_branch,
            creating_branch: false,
            amend: false,
            scroll: UniformListScrollHandle::new(),
            rows: Rc::default(),
            _subscriptions: subscriptions,
        };
        this.rows = Rc::new(this.build_rows(cx));
        this
    }
}

impl GitPanel {
    /// Why "Clone from GitHub…" is off, or `None` when it's on: it lists your repositories,
    /// which needs the GitHub sign-in.
    pub(crate) fn clone_blocked(&self, cx: &App) -> Option<SharedString> {
        self.account.read(cx).blocked("clone your repositories")
    }
}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let account = self.account.read(cx);
        let login = account.login().map(String::from);
        let signed_in = account.is_signed_in();
        let clone_blocked = self.clone_blocked(cx).is_some();
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
                            repo.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
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
            // The page itself leads with the sign-in card while signed out: it stays on.
            .child(
                Button::new("git-open-github")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Github)
                    .accessibility_label("GitHub")
                    .tooltip(match (&login, signed_in) {
                        (Some(login), _) => format!("GitHub · @{login}"),
                        (None, true) => "GitHub".to_string(),
                        (None, false) => "GitHub · Sign In".to_string(),
                    })
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::GitHub))),
            );

        let body: AnyElement = if store.repo.is_none() {
            // No folder: nothing to look for (the store never looks), so it says so at once.
            let state = if store.dir().is_some() && !store.checked {
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
                    .action(account::gate(
                        Button::new("git-clone")
                            .small()
                            .icon(Lucide::Github)
                            .label("Clone from GitHub…")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::GitHub))),
                        account,
                        "clone your repositories",
                        cx,
                    ))
                    .when(clone_blocked, |state| {
                        state.action(account::sign_in_link("git-clone-sign-in"))
                    })
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
            // Built when the store changed, and shared with the virtual list.
            let list_rows = self.rows.clone();
            let count = list_rows.len();
            let merging = store.status.as_ref().is_some_and(|s| s.merging);
            let busy = store.busy;
            let message = store
                .error
                .clone()
                .map(|e| (e, true))
                .or_else(|| store.notice.clone().map(|n| (n, false)));
            // A remote that asked for credentials Sawit had none to lend: the way on is beside it.
            let offer_sign_in = store.sign_in_would_help && !signed_in;
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
                    let banner = if error && offer_sign_in {
                        banner.action(account::sign_in_link("git-message-sign-in"))
                    } else {
                        banner
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
