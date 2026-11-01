//! Staging and unstaging, throwing changes away, and committing.

use std::path::Path;

use super::{FileChange, LOCAL_TIMEOUT, git, git_ok};
use crate::Result;

pub async fn stage(repo: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn stage_all(repo: &Path) -> Result<()> {
    git_ok(repo, &["add", "--all"], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

pub async fn unstage(repo: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["restore", "--staged", "--"];
    args.extend(paths.iter().map(String::as_str));
    // Before the first commit there is nothing to restore from.
    let out = git(repo, &args, None, LOCAL_TIMEOUT).await?;
    if out.ok {
        return Ok(());
    }
    let mut args = vec!["rm", "--cached", "-r", "-q", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn unstage_all(repo: &Path) -> Result<()> {
    let out = git(repo, &["reset", "-q"], None, LOCAL_TIMEOUT).await?;
    if out.ok {
        return Ok(());
    }
    git_ok(
        repo,
        &["rm", "--cached", "-r", "-q", "."],
        None,
        LOCAL_TIMEOUT,
    )
    .await
    .map(drop)
}

/// Throws away unstaged changes to `file` (deletes it when untracked).
pub async fn discard(repo: &Path, file: &FileChange) -> Result<()> {
    if file.untracked() {
        git_ok(
            repo,
            &["clean", "-f", "-q", "--", &file.path],
            None,
            LOCAL_TIMEOUT,
        )
        .await
        .map(drop)
    } else {
        git_ok(
            repo,
            &["restore", "--worktree", "--", &file.path],
            None,
            LOCAL_TIMEOUT,
        )
        .await
        .map(drop)
    }
}

pub async fn commit(repo: &Path, message: &str, amend: bool) -> Result<String> {
    let mut args = vec!["commit", "-m", message];
    if amend {
        args.push("--amend");
    }
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await
}

/// The last commit's message, to edit when amending.
pub async fn last_message(repo: &Path) -> Result<String> {
    git_ok(repo, &["log", "-1", "--format=%B"], None, LOCAL_TIMEOUT)
        .await
        .map(|s| s.trim_end().to_string())
}
