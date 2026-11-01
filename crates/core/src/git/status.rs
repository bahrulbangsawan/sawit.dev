//! The working tree's state: what changed, staged or not (`git status`), and the patch of each
//! change.

use std::path::Path;

use super::{LOCAL_TIMEOUT, git, git_ok};
use crate::Result;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Current branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Short hash of HEAD; `None` before the first commit.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
    /// A merge is in progress (`MERGE_HEAD` exists).
    pub merging: bool,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.is_staged())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.is_unstaged())
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.conflicted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// The old path of a rename or copy.
    pub from: Option<String>,
    /// Index (staged) state, porcelain letter: `M`, `A`, `D`, `R`, `C` or `.`.
    pub index: char,
    /// Work-tree (unstaged) state: `M`, `D`, `?` for untracked, or `.`.
    pub worktree: char,
    pub conflicted: bool,
}

impl FileChange {
    pub fn is_staged(&self) -> bool {
        !self.conflicted && self.index != '.' && self.index != '?'
    }

    pub fn is_unstaged(&self) -> bool {
        !self.conflicted && self.worktree != '.'
    }

    pub fn untracked(&self) -> bool {
        self.worktree == '?'
    }
}

/// Reads `git status --porcelain=v2 --branch -z`.
pub fn parse_status(raw: &str) -> Status {
    let mut status = Status::default();
    let mut fields = raw.split('\0');
    while let Some(entry) = fields.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => {
                    status.head = Some(value.chars().take(7).collect())
                }
                "branch.head" if value != "(detached)" => status.branch = Some(value.to_string()),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut chars = entry.chars();
        let kind = chars.next();
        let rest = chars.as_str().trim_start();
        match kind {
            // 1 XY sub mH mI mW hH hI path
            Some('1') => {
                if let Some((xy, path)) = split_entry(rest, 6) {
                    status.files.push(change(xy, path, None, false));
                }
            }
            // 2 XY sub mH mI mW hH hI Xscore path, then the old path
            Some('2') => {
                if let Some((xy, path)) = split_entry(rest, 7) {
                    let from = fields.next().map(String::from);
                    status.files.push(change(xy, path, from, false));
                }
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            Some('u') => {
                if let Some((xy, path)) = split_entry(rest, 8) {
                    status.files.push(change(xy, path, None, true));
                }
            }
            Some('?') => status.files.push(FileChange {
                path: rest.to_string(),
                from: None,
                index: '.',
                worktree: '?',
                conflicted: false,
            }),
            _ => {}
        }
    }
    status
}

/// The `XY` field and the path after the `skip` fields that follow it.
fn split_entry(rest: &str, skip: usize) -> Option<(&str, &str)> {
    let mut parts = rest.splitn(skip + 2, ' ');
    let xy = parts.next()?;
    let path = parts.nth(skip)?;
    Some((xy, path))
}

fn change(xy: &str, path: &str, from: Option<String>, conflicted: bool) -> FileChange {
    let mut c = xy.chars();
    FileChange {
        path: path.to_string(),
        from,
        index: c.next().unwrap_or('.'),
        worktree: c.next().unwrap_or('.'),
        conflicted,
    }
}

pub async fn status(repo: &Path) -> Result<Status> {
    // Neither takes a lock (`GIT_OPTIONAL_LOCKS=0`), so both run at once.
    let (raw, merge_head) = tokio::join!(
        git_ok(
            repo,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
            ],
            None,
            LOCAL_TIMEOUT,
        ),
        git(
            repo,
            &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
            None,
            LOCAL_TIMEOUT,
        ),
    );
    let mut status = parse_status(&raw?);
    status.merging = merge_head.is_ok_and(|o| o.ok);
    Ok(status)
}

/// The patch for one changed file: staged (index vs HEAD) or not (work tree
/// vs index; an untracked file shows as all new).
pub async fn diff(repo: &Path, file: &FileChange, staged: bool) -> Result<String> {
    if !staged && file.untracked() {
        let out = git(
            repo,
            &[
                "diff",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
                &file.path,
            ],
            None,
            LOCAL_TIMEOUT,
        )
        .await?;
        // Exit 1 means "there are differences".
        return Ok(out.stdout);
    }
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    if let Some(from) = file.from.as_deref().filter(|_| staged) {
        args.push(from);
    }
    args.push(&file.path);
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await
}
