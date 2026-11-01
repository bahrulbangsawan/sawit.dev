//! Branches: listing, switching and creating them, and merging one into the current branch.

use std::path::Path;

use super::{GitOutput, LOCAL_TIMEOUT, git, git_ok};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main`, or `origin/main` for a remote branch.
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
}

/// Reads `for-each-ref --format=%(refname)%00%(upstream:short)%00%(HEAD)`.
pub fn parse_branches(raw: &str) -> Vec<Branch> {
    raw.lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let refname = f.next()?;
            let upstream = f.next().filter(|s| !s.is_empty()).map(String::from);
            let current = f.next() == Some("*");
            let (name, remote) = if let Some(n) = refname.strip_prefix("refs/heads/") {
                (n, false)
            } else {
                (refname.strip_prefix("refs/remotes/")?, true)
            };
            // `origin/HEAD` points at another remote branch.
            (!(remote && name.ends_with("/HEAD"))).then(|| Branch {
                name: name.to_string(),
                remote,
                current,
                upstream,
            })
        })
        .collect()
}

pub async fn branches(repo: &Path) -> Result<Vec<Branch>> {
    git_ok(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%00%(upstream:short)%00%(HEAD)",
            "refs/heads",
            "refs/remotes",
        ],
        None,
        LOCAL_TIMEOUT,
    )
    .await
    .map(|raw| parse_branches(&raw))
}

/// Switches to `name`; a remote branch (`origin/x`) gets a local tracking
/// branch `x`.
pub async fn switch(repo: &Path, branch: &Branch) -> Result<()> {
    let args: Vec<&str> = if branch.remote {
        let local = branch
            .name
            .split_once('/')
            .map_or(branch.name.as_str(), |(_, b)| b);
        vec!["switch", "--track", "-c", local, &branch.name]
    } else {
        vec!["switch", &branch.name]
    };
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn create_branch(repo: &Path, name: &str) -> Result<()> {
    git_ok(repo, &["switch", "-c", name], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

/// Merges `branch` into the current branch. Conflicts are not an error: the
/// returned output says what happened and [`Status::merging`](super::Status::merging) is set.
pub async fn merge(repo: &Path, branch: &str) -> Result<GitOutput> {
    git(repo, &["merge", "--no-edit", branch], None, LOCAL_TIMEOUT).await
}

pub async fn merge_abort(repo: &Path) -> Result<()> {
    git_ok(repo, &["merge", "--abort"], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}
