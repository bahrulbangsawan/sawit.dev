//! GitHub: sign in with the device flow (the user types a code on
//! github.com, no password ever reaches Sawit), the token kept in the
//! Keychain, and the few REST calls the Git views need. Requests go through
//! `curl`, with the token passed on stdin so it never shows up in `ps`.

mod account;
mod backup;
mod http;
mod issues;
mod sign_in;
mod time;
mod timeline;
mod token;

pub use account::{Account, Repo, account, avatar, parse_account, parse_repos, repos};
pub use backup::{OwnRepo, commit_files, create_private_repo, own_repo};
pub use http::{REVOKED, is_revoked};
pub use issues::{
    Comment, Item, PullState, add_comment, comments, create_issue, create_pull, default_branch,
    items, merge_pull, parse_comments, parse_items, pull_diff, pull_state, set_open,
};
pub use sign_in::{DeviceCode, Poll, parse_device_code, parse_poll, start_sign_in, wait_for_token};
pub use time::{ago, now, parse_time};
pub use timeline::{
    ActionKind, Entry, Event, ReviewComment, ReviewState, TIMELINE_PAGE, attach_review_comments,
    group, parse_review_comments, parse_timeline, timeline,
};
pub use token::{Keychain, MemoryStore, TokenStore};

use crate::{CoreError, Result};

/// The OAuth app Sawit signs in as (Device Flow enabled, no secret needed).
/// Set at build time with `SAWIT_GITHUB_CLIENT_ID`, or at run time with the
/// same variable.
const CLIENT_ID: Option<&str> = option_env!("SAWIT_GITHUB_CLIENT_ID");
/// What the one sign-in asks GitHub for, and what in Sawit needs each: every GitHub control
/// works on this one sign-in, so it asks once for all of them. The sign-in card lists them.
pub const SCOPES: &[(&str, &str)] = &[
    (
        "repo",
        "pull requests and issues, comments, merges, cloning private repositories, push and pull over HTTPS, saving shell files to a private repository",
    ),
    (
        "workflow",
        "pushing commits that change GitHub Actions workflows (.github/workflows): GitHub refuses those pushes without it",
    ),
    ("read:user", "your name and avatar in the title bar"),
];
const API: &str = "https://api.github.com";

/// The scopes as the device flow takes them, space-separated.
fn scope_list() -> String {
    SCOPES
        .iter()
        .map(|(scope, _)| *scope)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The scopes of [`SCOPES`] a token granted `granted` lacks: one signed in before Sawit asked
/// for them. A wider scope GitHub lists counts for the narrower one (`user` for `read:user`).
pub fn missing_scopes(granted: &[String]) -> Vec<&'static str> {
    let has = |scope: &str| granted.iter().any(|g| g == scope);
    SCOPES
        .iter()
        .map(|(scope, _)| *scope)
        .filter(|scope| match *scope {
            "read:user" => !has("read:user") && !has("user"),
            scope => !has(scope),
        })
        .collect()
}

fn client_id() -> Result<String> {
    std::env::var("SAWIT_GITHUB_CLIENT_ID")
        .ok()
        .or(CLIENT_ID.map(String::from))
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            CoreError::GitHub(
                "GitHub sign-in isn't set up in this build. Set SAWIT_GITHUB_CLIENT_ID to an OAuth app's client ID with Device Flow enabled.".into(),
            )
        })
}

#[cfg(test)]
mod tests;
