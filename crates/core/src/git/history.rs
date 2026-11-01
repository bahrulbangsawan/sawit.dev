//! History: the newest commits (`git log`), and one commit's message and patch.

use std::path::Path;

use super::{LOCAL_TIMEOUT, git, git_ok};
use crate::{CoreError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
    /// Branch and tag names pointing here (`HEAD -> main`, `tag: v1`, …).
    pub refs: Vec<String>,
    pub subject: String,
}

impl Commit {
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

const LOG_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%D%x00%s%x1e";

/// Reads `git log` printed with [`LOG_FORMAT`].
pub fn parse_log(raw: &str) -> Vec<Commit> {
    raw.split('\x1e')
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                return None;
            }
            let mut f = record.splitn(7, '\0');
            let hash = f.next()?.to_string();
            let parents = f.next()?.split_whitespace().map(String::from).collect();
            let author = f.next()?.to_string();
            let email = f.next()?.to_string();
            let time = f.next()?.parse().unwrap_or(0);
            let refs = f
                .next()?
                .split(", ")
                .filter(|r| !r.is_empty())
                .map(String::from)
                .collect();
            let subject = f.next().unwrap_or_default().trim_end().to_string();
            Some(Commit {
                hash,
                parents,
                author,
                email,
                time,
                refs,
                subject,
            })
        })
        .collect()
}

/// The newest `limit` commits on every branch, newest first. `path` limits
/// it to the current branch's commits touching that file or folder, with
/// parents rewritten to those commits so the graph stays connected.
pub async fn log(repo: &Path, limit: usize, path: Option<&str>) -> Result<Vec<Commit>> {
    let limit = format!("-n{limit}");
    let mut args = vec!["log", "--date-order", LOG_FORMAT, &limit];
    match path {
        Some(path) => args.extend(["--parents", "--", path]),
        None => args.extend(["--all"]),
    }
    let out = git(repo, &args, None, LOCAL_TIMEOUT).await?;
    if out.ok {
        Ok(parse_log(&out.stdout))
    } else if out.stderr.contains("does not have any commits") {
        Ok(Vec::new())
    } else {
        Err(CoreError::Git(out.message()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CommitDetail {
    pub body: String,
    pub committer: String,
    /// The patch against the first parent.
    pub patch: String,
}

pub async fn show(repo: &Path, hash: &str) -> Result<CommitDetail> {
    let raw = git_ok(
        repo,
        &[
            "show",
            "--first-parent",
            "-m",
            "--format=%cn%x00%B%x00",
            "--patch",
            "--no-color",
            hash,
        ],
        None,
        LOCAL_TIMEOUT,
    )
    .await?;
    let mut parts = raw.splitn(3, '\0');
    Ok(CommitDetail {
        committer: parts.next().unwrap_or_default().to_string(),
        body: parts.next().unwrap_or_default().trim().to_string(),
        patch: parts.next().unwrap_or_default().trim_start().to_string(),
    })
}
