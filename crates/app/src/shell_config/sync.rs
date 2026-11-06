//! The shell files' two-way sync with GitHub, once the repository is found: how each file
//! stands, Upload, Download (a diff per file, the files chosen, "Replace 3 local files?", a backup
//! beside each and Undo), a file changed on both sides shown with both versions instead of
//! overwritten, and the open terminals told to read the new files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::github::{self, FileState};
use sawit_core::shell_config::{self, Downloaded, FileSync, SyncStatus};
use sawit_core::text_diff::{DiffLine, diff_counts, line_diff};

use super::{ShellConfigEvent, ShellConfigPanel};
use crate::rt;

/// Where the sync stands with GitHub.
#[derive(Default)]
pub(super) enum Remote {
    /// Not asked yet (signed out, or not looked).
    #[default]
    Idle,
    Checking,
    Ready(SyncStatus),
    Failed(String),
}

/// The sync's state in the panel.
#[derive(Default)]
pub(super) struct SyncUi {
    pub remote: Remote,
    /// The sign-in the status was asked with: another one asks again.
    pub checked_with: Option<String>,
    /// The files ticked for Upload or Download.
    pub chosen: BTreeSet<String>,
    /// GitHub's copy of each file that differs, and the diff from here to there.
    pub github_copy: BTreeMap<String, Vec<u8>>,
    pub diffs: BTreeMap<String, Vec<DiffLine>>,
    /// The file whose diff is shown.
    pub open_diff: Option<String>,
    /// An upload or download under way: what the button says meanwhile.
    pub busy: Option<&'static str>,
    /// The last download, which Undo puts back.
    pub undo: Option<Downloaded>,
    /// What the last action did.
    pub notice: Option<String>,
    /// Files a download replaced that open shells read: Source in Open Terminals.
    pub to_source: Vec<PathBuf>,
}

/// The last sync, for the panel.
fn last_sync(at: Option<i64>) -> String {
    match at {
        Some(at) => format!("Last synced {}", github::ago(at, github::now())),
        None => "Never synced from this Mac".into(),
    }
}

impl ShellConfigPanel {
    fn sync_state_path() -> PathBuf {
        sawit_core::instances::AppPaths::default_macos()
            .root
            .join(shell_config::SYNC_STATE)
    }

    /// Asks GitHub how the files stand, once per sign-in, or again with `force`.
    pub(super) fn check_sync(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(token) = self.account.read(cx).token().map(String::from) else {
            self.sync = SyncUi::default();
            return;
        };
        if !force && self.sync.checked_with.as_deref() == Some(token.as_str()) {
            return;
        }
        // Tests never ask GitHub: they set the sync's state themselves.
        if cfg!(test) {
            return;
        }
        if matches!(self.sync.remote, Remote::Checking) && !force {
            return;
        }
        self.sync.checked_with = Some(token.clone());
        self.sync.remote = Remote::Checking;
        cx.notify();
        let state = Self::sync_state_path();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move {
                let status = shell_config::sync_status(&token, &state).await?;
                let differ: Vec<FileSync> = status
                    .files
                    .iter()
                    .filter(|f| f.state != FileState::Same && f.remote.is_some())
                    .cloned()
                    .collect();
                let copies = match &status.backup {
                    Some(b) => shell_config::fetch_remote(&token, b, &differ).await?,
                    None => Vec::new(),
                };
                Ok::<_, sawit_core::CoreError>((status, copies))
            }
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok((status, copies)) => this.sync_ready(status, copies),
                    Err(e) => {
                        let message = e.to_string();
                        crate::git::account::note_refusal(&e, &token, cx);
                        this.sync.remote = Remote::Failed(message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn sync_ready(&mut self, status: SyncStatus, copies: Vec<(String, Vec<u8>)>) {
        self.sync.github_copy = copies.into_iter().collect();
        self.sync.diffs = status
            .files
            .iter()
            .filter_map(|f| {
                let theirs = self.sync.github_copy.get(&f.rel)?;
                let mine = std::fs::read(&f.path).unwrap_or_default();
                Some((
                    f.rel.clone(),
                    line_diff(
                        &String::from_utf8_lossy(&mine),
                        &String::from_utf8_lossy(theirs),
                    ),
                ))
            })
            .collect();
        // The files a plain Upload or Download would move are ticked; a conflict never is.
        self.sync.chosen = status
            .files
            .iter()
            .filter(|f| f.state.uploads() || f.state.downloads())
            .map(|f| f.rel.clone())
            .collect();
        self.sync.remote = Remote::Ready(status);
    }

    pub(super) fn status(&self) -> Option<&SyncStatus> {
        match &self.sync.remote {
            Remote::Ready(s) => Some(s),
            _ => None,
        }
    }

    /// The ticked files that `fits` (uploads or downloads).
    fn picked(&self, fits: impl Fn(FileState) -> bool) -> Vec<FileSync> {
        self.status()
            .map(|s| {
                s.files
                    .iter()
                    .filter(|f| self.sync.chosen.contains(&f.rel) && fits(f.state))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn upload_count(&self) -> usize {
        self.picked(FileState::uploads).len()
    }

    pub(super) fn download_count(&self) -> usize {
        self.picked(FileState::downloads).len()
    }

    /// "~/.zshrc: 3 lines added, 1 removed", one line per file.
    fn summary(&self, files: &[FileSync], github_side: bool) -> String {
        files
            .iter()
            .map(|f| {
                let (added, removed) = self
                    .sync
                    .diffs
                    .get(&f.rel)
                    .map(|d| diff_counts(d))
                    .unwrap_or((0, 0));
                // The diff goes from here to GitHub: an upload is its reverse.
                let (added, removed) = if github_side {
                    (added, removed)
                } else {
                    (removed, added)
                };
                match f.state {
                    FileState::OnlyHere => format!("~/{}: new on GitHub", f.rel),
                    FileState::OnlyOnGitHub => format!("~/{}: new on this Mac", f.rel),
                    _ => format!("~/{}: {added} lines added, {removed} removed", f.rel),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Upload: the ticked files changed here go up, after the diff summary and, when lines look
    /// like secrets, the secrets question. `files`: those instead (Keep Local on a conflict).
    pub(super) fn sync_upload(
        &mut self,
        only: Option<Vec<FileSync>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(token) = self.account.read(cx).token().map(String::from) else {
            return;
        };
        let Some(backup) = self.status().and_then(|s| s.backup.clone()) else {
            return;
        };
        let files = only.unwrap_or_else(|| self.picked(FileState::uploads));
        if files.is_empty() || self.sync.busy.is_some() {
            return;
        }
        let mut secrets = Vec::new();
        for f in &files {
            let text = std::fs::read_to_string(&f.path).unwrap_or_default();
            for (line, name) in shell_config::find_secrets(&text) {
                secrets.push(format!("~/{}:{line} {name}", f.rel));
            }
        }
        let n = files.len();
        let title = if n == 1 {
            format!("Upload ~/{} to GitHub?", files[0].rel)
        } else {
            format!("Upload {n} files to GitHub?")
        };
        let mut detail = self.summary(&files, false);
        if files.iter().any(|f| f.state == FileState::ChangedOnBoth) {
            detail.push_str("\n\nGitHub's version of a file changed on both sides is replaced.");
        }
        if !secrets.is_empty() {
            let shown: Vec<_> = secrets.iter().take(6).cloned().collect();
            detail.push_str(&format!(
                "\n\nThese lines look like secrets and go to the private repository too:\n{}",
                shown.join("\n")
            ));
        }
        let verb = if n == 1 {
            "Upload".to_string()
        } else {
            format!("Upload {n} Files")
        };
        let answer = crate::confirm::Confirm::new(title, verb)
            .detail(detail)
            .ask(window, cx);
        let rels: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
        cx.spawn_in(window, async move |this, cx| {
            if !answer.await {
                return;
            }
            let Ok(rx) = this.update(cx, |this, cx| {
                this.sync.busy = Some("Uploading…");
                this.sync.notice = None;
                cx.notify();
                let state = Self::sync_state_path();
                let host = shell_config::computer_name();
                let token = token.clone();
                rt::spawn(cx, async move {
                    shell_config::upload(&token, &backup, &rels, &state, &host).await
                })
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.sync.busy = None;
                match outcome {
                    Ok(Ok(done)) => {
                        this.sync.notice = Some(if done.changed {
                            format!(
                                "Uploaded {n} {} to GitHub.",
                                if n == 1 { "file" } else { "files" }
                            )
                        } else {
                            "GitHub already had these files; nothing to upload.".into()
                        });
                        this.check_sync(true, cx);
                    }
                    Ok(Err(e)) => this.github_failed(e, &token, cx),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Download: the ticked files changed on GitHub replace the ones here, after "Replace 3 local
    /// files?", each backed up beside itself first. `only`: those instead (Keep GitHub's).
    pub(super) fn sync_download(
        &mut self,
        only: Option<Vec<FileSync>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(status) = self.status() else { return };
        let Some(backup) = status.backup.clone() else {
            return;
        };
        let files = only.unwrap_or_else(|| self.picked(FileState::downloads));
        if files.is_empty() || self.sync.busy.is_some() {
            return;
        }
        let contents: Vec<(String, Vec<u8>)> = files
            .iter()
            .filter_map(|f| Some((f.rel.clone(), self.sync.github_copy.get(&f.rel)?.clone())))
            .collect();
        if contents.len() != files.len() {
            self.error =
                Some("GitHub's copy of a file isn't here yet. Check Again, then download.".into());
            cx.notify();
            return;
        }
        let n = files.len();
        let title = if n == 1 {
            format!("Replace ~/{} with GitHub's version?", files[0].rel)
        } else {
            format!("Replace {n} local files?")
        };
        let detail = format!(
            "{}\n\nEach file is backed up beside itself first (~/.zshrc.sawit-backup-<time>), and Undo puts it back.",
            self.summary(&files, true)
        );
        let verb = if n == 1 {
            "Replace".to_string()
        } else {
            format!("Replace {n} Files")
        };
        let answer = crate::confirm::Confirm::new(title, verb)
            .detail(detail)
            .ask(window, cx);
        let commit = backup.tree.commit.clone();
        cx.spawn_in(window, async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                let known = shell_config::known_files_here();
                let state = Self::sync_state_path();
                match shell_config::apply_download(
                    &known,
                    &contents,
                    &state,
                    &commit,
                    github::now(),
                ) {
                    Ok(done) => {
                        this.sync.to_source = done
                            .replaced
                            .iter()
                            .map(|r| r.path.clone())
                            .filter(|p| {
                                this.configs
                                    .iter()
                                    .any(|c| &c.path == p && this.reads(c.shell))
                            })
                            .collect();
                        this.sync.notice = Some(format!(
                            "Replaced {n} {} with GitHub's. Each was backed up beside itself.",
                            if n == 1 { "file" } else { "files" }
                        ));
                        this.sync.undo = Some(done);
                        this.refresh(cx);
                        this.check_sync(true, cx);
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts back what the last download replaced.
    pub(super) fn sync_undo(&mut self, cx: &mut Context<Self>) {
        let Some(done) = self.sync.undo.take() else {
            return;
        };
        match shell_config::undo_download(&done, &Self::sync_state_path()) {
            Ok(()) => {
                self.sync.notice = Some("Put the files back as they were.".into());
                self.sync.to_source.clear();
                self.refresh(cx);
                self.check_sync(true, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Both versions of a file changed on both sides, open beside each other to merge by hand:
    /// the one here, and GitHub's in a copy in Sawit's data folder.
    pub(super) fn open_both(&mut self, file: &FileSync, cx: &mut Context<Self>) {
        let Some(theirs) = self.sync.github_copy.get(&file.rel) else {
            return;
        };
        let dir = sawit_core::instances::AppPaths::default_macos()
            .root
            .join("shell-sync-github");
        let copy = dir.join(format!("{}.from-github", file.rel.replace('/', "_")));
        let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&copy, theirs));
        match written {
            Ok(()) => {
                cx.emit(ShellConfigEvent::OpenFile(file.path.clone()));
                cx.emit(ShellConfigEvent::OpenFile(copy));
            }
            Err(e) => {
                self.error = Some(format!("GitHub's copy couldn't be saved: {e}"));
                cx.notify();
            }
        }
    }

    pub(super) fn source_in_terminals(&mut self, cx: &mut Context<Self>) {
        let files = std::mem::take(&mut self.sync.to_source);
        if !files.is_empty() {
            cx.emit(ShellConfigEvent::SourceInTerminals(files));
        }
        cx.notify();
    }

    /// What Source in Open Terminals did: how many shells read the files, and which were busy.
    pub(crate) fn sourced(&mut self, told: usize, busy: Vec<String>, cx: &mut Context<Self>) {
        let shells = if told == 1 {
            "1 shell".to_string()
        } else {
            format!("{told} shells")
        };
        let mut text = format!("{shells} read the new files.");
        if !busy.is_empty() {
            text.push_str(&format!(
                " Left alone while they run something: {}. Open a new shell there.",
                busy.join(", ")
            ));
        }
        self.sync.notice = Some(text);
        cx.notify();
    }

    pub(super) fn toggle_chosen(&mut self, rel: &str, on: bool, cx: &mut Context<Self>) {
        if on {
            self.sync.chosen.insert(rel.to_string());
        } else {
            self.sync.chosen.remove(rel);
        }
        cx.notify();
    }

    pub(super) fn toggle_diff(&mut self, rel: &str, cx: &mut Context<Self>) {
        self.sync.open_diff = match self.sync.open_diff.as_deref() {
            Some(open) if open == rel => None,
            _ => Some(rel.to_string()),
        };
        cx.notify();
    }

    pub(super) fn last_sync_text(&self) -> String {
        last_sync(self.status().and_then(|s| s.synced_at))
    }
}
