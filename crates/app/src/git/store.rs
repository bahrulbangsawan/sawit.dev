//! The window's Git state: the repository of the open folder, its status
//! and branches, the action running, and the GitHub sign-in. Shared by the
//! Git sidebar, the graph, diff and GitHub tabs.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use sawit_core::git::{self, Branch, Status};
use sawit_core::github::{self, Account, DeviceCode};
use sawit_core::{CoreError, Result};

use crate::rt;

/// How often the status is re-read while a repository is open, so edits
/// made elsewhere (the editor, the terminal) show up.
const POLL: Duration = Duration::from_secs(4);

pub enum GitEvent {
    /// History may have changed (commit, merge, pull, switch…).
    HistoryChanged,
}

impl EventEmitter<GitEvent> for GitStore {}

#[derive(Clone, Default)]
pub enum SignIn {
    #[default]
    Idle,
    /// Asking GitHub for a code.
    Starting,
    /// Waiting for the user to enter the code on github.com.
    Waiting(DeviceCode),
    Failed(String),
}

pub struct GitStore {
    /// The open folder.
    dir: Option<PathBuf>,
    /// Top folder of its repository; `None` when it isn't in one.
    pub repo: Option<PathBuf>,
    /// The folder has been checked (so "not a repository" can be shown).
    pub checked: bool,
    pub status: Option<Status>,
    pub branches: Vec<Branch>,
    /// `owner`, `name` when `origin` is on github.com.
    pub slug: Option<(String, String)>,
    /// The action running ("Pushing…"); others wait for it.
    pub busy: Option<&'static str>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub token: Option<String>,
    pub account: Option<Account>,
    pub sign_in: SignIn,
    head: Option<String>,
    /// Bumped on every refresh so a stale answer is dropped.
    generation: u64,
    _poll: Task<()>,
    _sign_in: Task<()>,
}

impl GitStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    if this.repo.is_some() && this.busy.is_none() {
                        this.refresh(cx);
                    }
                }) else {
                    return;
                };
            }
        });
        let mut this = Self {
            dir: None,
            repo: None,
            checked: false,
            status: None,
            branches: Vec::new(),
            slug: None,
            busy: None,
            error: None,
            notice: None,
            token: None,
            account: None,
            sign_in: SignIn::Idle,
            head: None,
            generation: 0,
            _poll: poll,
            _sign_in: Task::ready(()),
        };
        this.load_account(cx);
        this
    }

    /// Follows the folder open in the window.
    pub fn set_dir(&mut self, dir: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.dir == dir {
            return;
        }
        self.dir = dir;
        self.repo = None;
        self.checked = false;
        self.status = None;
        self.branches.clear();
        self.slug = None;
        self.error = None;
        self.notice = None;
        self.head = None;
        self.rediscover(cx);
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn rediscover(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let Some(dir) = self.dir.clone() else {
            self.checked = true;
            cx.notify();
            return;
        };
        let rx = rt::spawn(cx, async move { git::discover(&dir).await });
        cx.spawn(async move |this, cx| {
            let repo = rx.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.repo = repo;
                this.checked = true;
                if this.repo.is_some() {
                    this.refresh(cx);
                }
                cx.emit(GitEvent::HistoryChanged);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Re-reads status, branches and the remote.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let rx = rt::spawn(cx, async move {
            let status = git::status(&repo).await;
            let branches = git::branches(&repo).await.unwrap_or_default();
            let slug = git::remote_url(&repo)
                .await
                .and_then(|url| git::github_slug(&url));
            (status, branches, slug)
        });
        cx.spawn(async move |this, cx| {
            let Ok((status, branches, slug)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match status {
                    Ok(status) => {
                        let head = status
                            .head
                            .clone()
                            .zip(status.branch.clone())
                            .map(|(h, b)| format!("{h} {b}"));
                        if head != this.head {
                            this.head = head;
                            cx.emit(GitEvent::HistoryChanged);
                        }
                        if this.status.as_ref() != Some(&status) {
                            this.status = Some(status);
                            cx.notify();
                        }
                    }
                    // The folder stopped being a repository (deleted `.git`).
                    Err(_) => {
                        this.rediscover(cx);
                        return;
                    }
                }
                if this.branches != branches || this.slug != slug {
                    this.branches = branches;
                    this.slug = slug;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs one Git action. `f` gets the repository and the GitHub token;
    /// what it returns is shown as a notice (or an error), then everything
    /// is re-read.
    pub fn run<F, Fut>(&mut self, label: &'static str, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(PathBuf, Option<String>) -> Fut,
        Fut: Future<Output = Result<String>> + Send + 'static,
    {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(label);
        self.error = None;
        self.notice = None;
        cx.notify();
        let rx = rt::spawn(cx, f(repo, self.token.clone()));
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err(CoreError::Git("Canceled.".into())));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(message) => this.notice = Some(message).filter(|m| !m.trim().is_empty()),
                    Err(e) => this.error = Some(friendly(&e.to_string(), this.token.is_some())),
                }
                this.refresh(cx);
                cx.emit(GitEvent::HistoryChanged);
                cx.notify();
            });
        })
        .detach();
    }

    /// Makes the open folder a repository.
    pub fn init_repo(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let rx = rt::spawn(cx, async move { git::init(&dir).await });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(e)) = result {
                    this.error = Some(e.to_string());
                }
                this.rediscover(cx);
            });
        })
        .detach();
    }

    pub fn dismiss_message(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    // ------------------------------------------------------------ GitHub

    fn load_account(&mut self, cx: &mut Context<Self>) {
        let rx = rt::spawn(cx, async move {
            let token = github::load_token().ok().flatten()?;
            let account = github::account(&token).await;
            Some((token, account))
        });
        cx.spawn(async move |this, cx| {
            let Ok(Some((token, account))) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match account {
                    Ok(account) => this.account = Some(account),
                    // Keep the token through a network failure; a revoked
                    // token is reported where it's used.
                    Err(e) => this.sign_in = SignIn::Failed(e.to_string()),
                }
                this.token = Some(token);
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts the device flow; the GitHub tab shows the code to enter.
    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        self.sign_in = SignIn::Starting;
        cx.notify();
        let rx = rt::spawn(cx, async move { github::start_sign_in().await });
        self._sign_in = cx.spawn(async move |this, cx| {
            let device = match rx.await {
                Ok(Ok(device)) => device,
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.sign_in = SignIn::Failed(e.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(_) => return,
            };
            let Ok(rx) = this.update(cx, |this, cx| {
                this.sign_in = SignIn::Waiting(device.clone());
                cx.notify();
                rt::spawn(cx, async move {
                    let token = github::wait_for_token(&device).await?;
                    let account = github::account(&token).await?;
                    Ok::<_, CoreError>((token, account))
                })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok((token, account))) => {
                        this.token = Some(token);
                        this.account = Some(account);
                        this.sign_in = SignIn::Idle;
                    }
                    Ok(Err(e)) => this.sign_in = SignIn::Failed(e.to_string()),
                    Err(_) => this.sign_in = SignIn::Idle,
                }
                cx.notify();
            });
        });
    }

    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        self._sign_in = Task::ready(());
        self.sign_in = SignIn::Idle;
        cx.notify();
    }

    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = github::delete_token() {
            self.sign_in = SignIn::Failed(e.to_string());
        } else {
            self.sign_in = SignIn::Idle;
        }
        self.token = None;
        self.account = None;
        cx.notify();
    }
}

/// Adds what to do to git's most common refusals.
fn friendly(message: &str, signed_in: bool) -> String {
    let lower = message.to_lowercase();
    let hint = if lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("terminal prompts disabled")
    {
        if signed_in {
            "GitHub refused your sign-in for this remote. Check that your account has access to it."
        } else {
            "This remote needs a sign-in. Sign in to GitHub in the GitHub tab, or set up a Git credential helper."
        }
    } else if lower.contains("not possible to fast-forward") || lower.contains("diverging branches")
    {
        "Your branch and its remote have both changed. Merge the remote branch from the branch menu, then push."
    } else if lower.contains("non-fast-forward") || lower.contains("fetch first") {
        "The remote has commits you don't have. Pull first, then push."
    } else if lower.contains("please tell me who you are") {
        "Git doesn't know your name yet. Run `git config --global user.name \"Your Name\"` and `git config --global user.email you@example.com` in the terminal."
    } else if lower.contains("would be overwritten") {
        "You have changes that this would overwrite. Commit them first, or discard them."
    } else if lower.contains("nothing to commit") {
        "There's nothing to commit."
    } else {
        ""
    };
    if hint.is_empty() {
        message.to_string()
    } else {
        format!("{hint}\n\n{message}")
    }
}
