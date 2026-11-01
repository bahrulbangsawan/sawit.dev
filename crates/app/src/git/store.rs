//! A project's Git state: the repository of the open folder, its status
//! and branches, and the action running. Shared by the Git sidebar, the
//! graph, diff and GitHub tabs. The GitHub sign-in is the app's one
//! ([`super::account`]), which remote actions lend git.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use sawit_core::git::{self, Branch, Status};
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
    /// The last error was the remote asking for a sign-in while there was none to lend it:
    /// the panel offers the GitHub sign-in beside it.
    pub sign_in_would_help: bool,
    pub notice: Option<String>,
    head: Option<String>,
    /// Bumped on every refresh so a stale answer is dropped.
    generation: u64,
    _poll: Task<()>,
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
        Self {
            dir: None,
            repo: None,
            checked: false,
            status: None,
            branches: Vec::new(),
            slug: None,
            busy: None,
            error: None,
            sign_in_would_help: false,
            notice: None,
            head: None,
            generation: 0,
            _poll: poll,
        }
    }

    /// Follows the folder open in the window. The same folder again (Try Again in the Files
    /// tab) keeps what is known of it, and says so to those who follow the store.
    pub fn set_dir(&mut self, dir: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.dir == dir {
            cx.notify();
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

    /// Runs one Git action. `f` gets the repository and the app's GitHub sign-in, which a
    /// remote action lends git for github.com over HTTPS when the user's own credentials don't
    /// answer; what it returns is shown as a notice (or an error), then everything is re-read.
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
        self.sign_in_would_help = false;
        self.notice = None;
        cx.notify();
        let token = super::account::global(cx)
            .read(cx)
            .token()
            .map(String::from);
        let signed_in = token.is_some();
        let rx = rt::spawn(cx, f(repo, token));
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err(CoreError::Git("Canceled.".into())));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(message) => this.notice = Some(message).filter(|m| !m.trim().is_empty()),
                    Err(e) => {
                        let message = e.to_string();
                        this.sign_in_would_help = !signed_in && asks_for_sign_in(&message);
                        this.error = Some(friendly(&message, signed_in));
                    }
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
        self.sign_in_would_help = false;
        self.notice = None;
        cx.notify();
    }
}

/// Whether git's `message` is the remote asking for credentials nobody gave it.
fn asks_for_sign_in(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("terminal prompts disabled")
}

/// Adds what to do to git's most common refusals.
fn friendly(message: &str, signed_in: bool) -> String {
    let lower = message.to_lowercase();
    let hint = if asks_for_sign_in(message) {
        if signed_in {
            "GitHub refused your sign-in for this remote. Check that your account has access to it."
        } else {
            "This remote needs a sign-in. Sign in to GitHub (one sign-in for all of Sawit), or set up a Git credential helper or an SSH key."
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A remote asking for credentials while signed out points at the one GitHub sign-in (the
    /// panel offers it beside the error), as well as the user's own credential helper; signed
    /// in, it says the account may lack access instead.
    #[test]
    fn a_remote_asking_for_credentials_points_at_the_sign_in() {
        let refused =
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled";
        assert!(asks_for_sign_in(refused));
        assert!(!asks_for_sign_in("! [rejected] main -> main (fetch first)"));
        let signed_out = friendly(refused, false);
        assert!(
            signed_out.starts_with("This remote needs a sign-in. Sign in to GitHub"),
            "{signed_out}"
        );
        assert!(signed_out.ends_with(refused), "git's own words are kept");
        assert!(friendly(refused, true).starts_with("GitHub refused your sign-in"));
    }
}
