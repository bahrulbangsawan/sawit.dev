//! The remote: fetching, pulling, pushing, cloning, and the GitHub repository it is.

use std::path::Path;

use super::{LOCAL_TIMEOUT, REMOTE_TIMEOUT, git, git_ok};
use crate::{CoreError, Result};

pub async fn fetch(repo: &Path, token: Option<&str>) -> Result<String> {
    git_ok(repo, &["fetch", "--all", "--prune"], token, REMOTE_TIMEOUT).await
}

/// Fast-forwards only: a diverged branch is left for an explicit merge.
pub async fn pull(repo: &Path, token: Option<&str>) -> Result<String> {
    git_ok(repo, &["pull", "--ff-only"], token, REMOTE_TIMEOUT).await
}

/// Pushes the current branch, setting its upstream on `origin` the first time.
pub async fn push(repo: &Path, has_upstream: bool, token: Option<&str>) -> Result<String> {
    let args: &[&str] = if has_upstream {
        &["push"]
    } else {
        &["push", "-u", "origin", "HEAD"]
    };
    let out = git(repo, args, token, REMOTE_TIMEOUT).await?;
    if out.ok {
        Ok(out.message())
    } else {
        Err(CoreError::Git(out.message()))
    }
}

pub async fn clone(url: &str, into: &Path, token: Option<&str>) -> Result<()> {
    let parent = into.parent().unwrap_or(Path::new("/"));
    let target = into.to_string_lossy();
    git_ok(
        parent,
        &["clone", "--", url, &target],
        token,
        REMOTE_TIMEOUT,
    )
    .await
    .map(drop)
}

pub async fn remote_url(repo: &Path) -> Option<String> {
    let out = git(repo, &["remote", "get-url", "origin"], None, LOCAL_TIMEOUT)
        .await
        .ok()?;
    out.ok.then(|| out.stdout.trim().to_string())
}

/// `owner/name` of a github.com remote, from its HTTPS or SSH URL.
pub fn github_slug(url: &str) -> Option<(String, String)> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| {
            // https://user@github.com/…
            let rest = url.strip_prefix("https://")?;
            rest.split_once("@github.com/").map(|(_, p)| p)
        })?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| (owner.to_string(), name.to_string()))
}
