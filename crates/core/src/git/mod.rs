//! Git through the `git` command line: status, staging, commits, branches,
//! merge, sync with the remote, history with a lane layout for the commit
//! graph, and diffs.
//!
//! Parsing is kept separate from running so it can be tested on fixed
//! output.

mod branches;
mod graph;
mod history;
mod remote;
mod repo;
mod staging;
mod status;

pub use branches::{Branch, branches, create_branch, merge, merge_abort, parse_branches, switch};
pub use graph::{Edge, GraphRow, graph, lane_count};
pub use history::{Commit, CommitDetail, log, parse_log, show};
pub use remote::{clone, fetch, github_slug, pull, push, remote_url};
pub use repo::{discover, git_path, ignore, init};
pub use staging::{commit, discard, last_message, stage, stage_all, unstage, unstage_all};
pub use status::{FileChange, Status, diff, parse_status, status};

use std::path::Path;
use std::time::Duration;

use crate::{CoreError, Result};

/// Local commands (status, log, diff, commit).
const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Commands that talk to a remote (fetch, pull, push, clone).
const REMOTE_TIMEOUT: Duration = Duration::from_secs(300);

/// What one `git` run printed.
pub struct GitOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    /// The useful part of what git said, for showing after an action.
    pub fn message(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.trim().to_string()
    }
}

/// The variable the GitHub sign-in reaches git's credential helper in: the environment of that
/// one `git` run, never its command line, where `ps` would show it.
pub const TOKEN_ENV: &str = "SAWIT_GITHUB_TOKEN";

/// The `-c` setting that lends git the GitHub sign-in: a credential helper for
/// `https://github.com` only (no other host, nor SSH remotes, ever sees the token), which
/// answers `get` with the token from [`TOKEN_ENV`] and ignores `store` and `erase`, so git never
/// writes it anywhere. Git asks the helpers the user set up first (their own `osxkeychain`, `gh
/// auth git-credential`) and only falls back to this one when none answers.
pub fn github_credential_helper() -> [&'static str; 2] {
    [
        "-c",
        "credential.https://github.com.helper=!f() { test \"$1\" = get && echo username=x-access-token && echo \"password=$SAWIT_GITHUB_TOKEN\"; }; f",
    ]
}

/// The `git -C dir args…` command, with `token` lent through [`github_credential_helper`].
fn command(dir: &Path, args: &[&str], token: Option<&str>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(dir);
    if token.is_some() {
        cmd.args(github_credential_helper());
    }
    cmd.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(token) = token {
        cmd.env(TOKEN_ENV, token);
    }
    cmd
}

/// Runs `git -C dir args…`. Never prompts in a terminal; with `token`, HTTPS
/// remotes on github.com fall back to it when no credential helper answers.
async fn git(
    dir: &Path,
    args: &[&str],
    token: Option<&str>,
    timeout: Duration,
) -> Result<GitOutput> {
    let mut cmd = command(dir, args, token);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| CoreError::Command {
            program: format!("git {}", args.first().unwrap_or(&"")),
            code: None,
            stderr: format!("timed out after {timeout:?}"),
        })?
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::Config(
                "Git isn't installed. Install the Xcode Command Line Tools with `xcode-select --install`, then try again.".into(),
            ),
            _ => CoreError::Io(e),
        })?;
    Ok(GitOutput {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Like [`git`], but a failure becomes an error carrying git's message.
async fn git_ok(
    dir: &Path,
    args: &[&str],
    token: Option<&str>,
    timeout: Duration,
) -> Result<String> {
    let out = git(dir, args, token, timeout).await?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(CoreError::Git(out.message()))
    }
}

#[cfg(test)]
mod tests;
