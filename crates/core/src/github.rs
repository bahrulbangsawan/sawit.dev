//! GitHub: sign in with the device flow (the user types a code on
//! github.com, no password ever reaches Sawit), the token kept in the
//! Keychain, and the few REST calls the Git views need. Requests go through
//! `curl`, with the token passed on stdin so it never shows up in `ps`.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::{CoreError, Result};

/// The OAuth app Sawit signs in as (Device Flow enabled, no secret needed).
/// Set at build time with `SAWIT_GITHUB_CLIENT_ID`, or at run time with the
/// same variable.
const CLIENT_ID: Option<&str> = option_env!("SAWIT_GITHUB_CLIENT_ID");
/// Private repositories (push, issues, pull requests) and the profile.
const SCOPES: &str = "repo read:user";
const API: &str = "https://api.github.com";

const KEYCHAIN_SERVICE: &str = "sawit";
const KEYCHAIN_ACCOUNT: &str = "github";

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

// ---------------------------------------------------------------- token

pub fn load_token() -> Result<Option<String>> {
    crate::keychain::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
}

pub fn store_token(token: &str) -> Result<()> {
    crate::keychain::set(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, token)
}

pub fn delete_token() -> Result<()> {
    crate::keychain::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
}

// ---------------------------------------------------------------- http

/// One request with `curl`. `header` lines (the token among them) go in on
/// stdin; the HTTP status comes back with the body.
async fn curl(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<&str>,
) -> Result<(u16, String)> {
    let mut cmd = tokio::process::Command::new("/usr/bin/curl");
    cmd.args([
        "-sS",
        "--max-time",
        "30",
        "-X",
        method,
        "-H",
        "@-",
        "-w",
        "\n%{http_code}",
    ]);
    if let Some(body) = body {
        cmd.args(["--data-binary", body]);
    }
    cmd.arg(url)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(headers.join("\n").as_bytes()).await?;
    }
    let out = tokio::time::timeout(Duration::from_secs(40), child.wait_with_output())
        .await
        .map_err(|_| {
            CoreError::GitHub(
                "GitHub didn't answer in time. Check your connection and try again.".into(),
            )
        })??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(CoreError::GitHub(format!(
            "Unable to reach GitHub. Check your connection and try again. ({})",
            err.trim()
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    Ok((code.trim().parse().unwrap_or(0), body.to_string()))
}

/// A REST API call as the signed-in user.
async fn api(method: &str, path: &str, token: &str, body: Option<Value>) -> Result<Value> {
    let text = api_raw(method, path, token, body, "application/vnd.github+json").await?;
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Like [`api`], asking for the `accept` media type and returning the text
/// (a diff, say).
async fn api_raw(
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
    accept: &str,
) -> Result<String> {
    let headers = vec![
        format!("Authorization: Bearer {token}"),
        format!("Accept: {accept}"),
        "X-GitHub-Api-Version: 2022-11-28".to_string(),
        "Content-Type: application/json".to_string(),
    ];
    let body = body.map(|b| b.to_string());
    let (code, text) = curl(method, &format!("{API}{path}"), &headers, body.as_deref()).await?;
    match code {
        200..=299 => Ok(text),
        401 => Err(CoreError::GitHub(
            "GitHub no longer accepts this sign-in. Sign out, then sign in again.".into(),
        )),
        _ => {
            let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let message = value["message"].as_str().unwrap_or("Unknown error");
            let hint = match code {
                403 | 404 => {
                    " Your account may not have access, or the app wasn't granted access to this organization."
                }
                405 | 409 => " The pull request can't be merged as it is.",
                _ => "",
            };
            Err(CoreError::GitHub(format!(
                "GitHub refused ({code}): {message}.{hint}"
            )))
        }
    }
}

// ---------------------------------------------------------------- sign in

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    /// What the user types on the verification page.
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub fn parse_device_code(v: &Value) -> Option<DeviceCode> {
    Some(DeviceCode {
        device_code: v["device_code"].as_str()?.to_string(),
        user_code: v["user_code"].as_str()?.to_string(),
        verification_uri: v["verification_uri"].as_str()?.to_string(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Starts signing in: show `user_code` and open `verification_uri`, then
/// call [`wait_for_token`].
pub async fn start_sign_in() -> Result<DeviceCode> {
    let body = format!(
        "client_id={}&scope={}",
        client_id()?,
        SCOPES.replace(' ', "%20")
    );
    let (code, text) = curl(
        "POST",
        "https://github.com/login/device/code",
        &["Accept: application/json".into()],
        Some(&body),
    )
    .await?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    parse_device_code(&v).ok_or_else(|| {
        CoreError::GitHub(format!(
            "GitHub didn't start the sign-in ({code}): {}",
            v["error_description"]
                .as_str()
                .unwrap_or("unexpected answer")
        ))
    })
}

/// What one poll of the token endpoint said.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Token(String),
    Pending,
    SlowDown,
    Failed(String),
}

pub fn parse_poll(v: &Value) -> Poll {
    if let Some(token) = v["access_token"].as_str() {
        return Poll::Token(token.to_string());
    }
    match v["error"].as_str() {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("expired_token") => {
            Poll::Failed("The code expired before it was entered. Start signing in again.".into())
        }
        Some("access_denied") => Poll::Failed("Sign-in was canceled on GitHub.".into()),
        _ => Poll::Failed(format!(
            "GitHub didn't finish the sign-in: {}",
            v["error_description"]
                .as_str()
                .unwrap_or("unexpected answer")
        )),
    }
}

/// Polls until the user approves on github.com, then saves the token.
pub async fn wait_for_token(device: &DeviceCode) -> Result<String> {
    let client_id = client_id()?;
    let body = format!(
        "client_id={client_id}&device_code={}&grant_type=urn:ietf:params:oauth:grant-type:device_code",
        device.device_code
    );
    let mut interval = device.interval.max(1);
    let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let (_, text) = curl(
            "POST",
            "https://github.com/login/oauth/access_token",
            &["Accept: application/json".into()],
            Some(&body),
        )
        .await?;
        match parse_poll(&serde_json::from_str(&text).unwrap_or(Value::Null)) {
            Poll::Token(token) => {
                store_token(&token)?;
                return Ok(token);
            }
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Failed(message) => return Err(CoreError::GitHub(message)),
        }
    }
    Err(CoreError::GitHub(
        "The code expired before it was entered. Start signing in again.".into(),
    ))
}

// ---------------------------------------------------------------- data

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
}

pub async fn account(token: &str) -> Result<Account> {
    let v = api("GET", "/user", token, None).await?;
    Ok(Account {
        login: v["login"].as_str().unwrap_or_default().to_string(),
        name: v["name"].as_str().map(String::from),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub clone_url: String,
    /// Unix seconds of the last push.
    pub pushed_at: i64,
}

pub fn parse_repos(v: &Value) -> Vec<Repo> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Repo {
                full_name: r["full_name"].as_str()?.to_string(),
                description: r["description"]
                    .as_str()
                    .filter(|d| !d.is_empty())
                    .map(String::from),
                private: r["private"].as_bool().unwrap_or(false),
                clone_url: r["clone_url"].as_str()?.to_string(),
                pushed_at: r["pushed_at"].as_str().and_then(parse_time).unwrap_or(0),
            })
        })
        .collect()
}

/// The user's repositories (own, collaborator and organization), most
/// recently pushed first.
pub async fn repos(token: &str) -> Result<Vec<Repo>> {
    api("GET", "/user/repos?per_page=100&sort=pushed", token, None)
        .await
        .map(|v| parse_repos(&v))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub open: bool,
    pub pull: bool,
    pub comments: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub url: String,
}

pub fn parse_items(v: &Value) -> Vec<Item> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            Some(Item {
                number: i["number"].as_u64()?,
                title: i["title"].as_str().unwrap_or_default().to_string(),
                body: i["body"].as_str().unwrap_or_default().to_string(),
                author: i["user"]["login"].as_str().unwrap_or("ghost").to_string(),
                open: i["state"].as_str() == Some("open"),
                pull: i.get("pull_request").is_some_and(|p| !p.is_null()),
                comments: i["comments"].as_u64().unwrap_or(0),
                created_at: i["created_at"].as_str().and_then(parse_time).unwrap_or(0),
                updated_at: i["updated_at"].as_str().and_then(parse_time).unwrap_or(0),
                url: i["html_url"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// Issues and pull requests (GitHub lists both as issues), recently updated
/// first. `open` false lists closed ones.
pub async fn items(token: &str, owner: &str, repo: &str, open: bool) -> Result<Vec<Item>> {
    let state = if open { "open" } else { "closed" };
    api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues?state={state}&per_page=100&sort=updated"),
        token,
        None,
    )
    .await
    .map(|v| parse_items(&v))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

pub fn parse_comments(v: &Value) -> Vec<Comment> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|c| Comment {
            author: c["user"]["login"].as_str().unwrap_or("ghost").to_string(),
            body: c["body"].as_str().unwrap_or_default().to_string(),
            created_at: c["created_at"].as_str().and_then(parse_time).unwrap_or(0),
        })
        .collect()
}

pub async fn comments(token: &str, owner: &str, repo: &str, number: u64) -> Result<Vec<Comment>> {
    api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"),
        token,
        None,
    )
    .await
    .map(|v| parse_comments(&v))
}

pub async fn add_comment(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    body: &str,
) -> Result<Comment> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/issues/{number}/comments"),
        token,
        Some(json!({ "body": body })),
    )
    .await?;
    Ok(parse_comments(&Value::Array(vec![v])).remove(0))
}

/// Opens or closes an issue or pull request.
pub async fn set_open(token: &str, owner: &str, repo: &str, number: u64, open: bool) -> Result<()> {
    api(
        "PATCH",
        &format!("/repos/{owner}/{repo}/issues/{number}"),
        token,
        Some(json!({ "state": if open { "open" } else { "closed" } })),
    )
    .await
    .map(drop)
}

/// A pull request's merge state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullState {
    pub merged: bool,
    /// `None` while GitHub is still working it out.
    pub mergeable: Option<bool>,
    pub head: String,
    pub base: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
}

pub async fn pull_state(token: &str, owner: &str, repo: &str, number: u64) -> Result<PullState> {
    let v = api(
        "GET",
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        token,
        None,
    )
    .await?;
    Ok(PullState {
        merged: v["merged"].as_bool().unwrap_or(false),
        mergeable: v["mergeable"].as_bool(),
        head: v["head"]["ref"].as_str().unwrap_or_default().to_string(),
        base: v["base"]["ref"].as_str().unwrap_or_default().to_string(),
        additions: v["additions"].as_u64().unwrap_or(0),
        deletions: v["deletions"].as_u64().unwrap_or(0),
        changed_files: v["changed_files"].as_u64().unwrap_or(0),
    })
}

/// The pull request's whole change as a unified diff.
pub async fn pull_diff(token: &str, owner: &str, repo: &str, number: u64) -> Result<String> {
    api_raw(
        "GET",
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        token,
        None,
        "application/vnd.github.diff",
    )
    .await
}

// ---------------------------------------------------------------- timeline

/// One thing that happened on an issue or pull request, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A commit pushed to the pull request; [`group`] folds a run of them.
    Committed {
        author: String,
        time: i64,
        sha: String,
        message: String,
    },
    Commented {
        author: String,
        time: i64,
        body: String,
    },
    Reviewed {
        author: String,
        time: i64,
        state: ReviewState,
        body: String,
        /// GitHub's review id, to attach inline comments.
        id: u64,
        comments: Vec<ReviewComment>,
    },
    /// Merged, closed, renamed, labeled… one line, no body.
    Action {
        actor: String,
        time: i64,
        kind: ActionKind,
        /// What it was about: the new title, the label, the reviewer, a
        /// commit, a linked issue.
        detail: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Merged,
    Closed,
    Reopened,
    Renamed,
    Labeled,
    Unlabeled,
    ReviewRequested,
    Assigned,
    ForcePushed,
    BranchDeleted,
    ReadyForReview,
    MarkedDraft,
    Referenced,
}

/// A comment on a line of a file, part of a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewComment {
    pub path: String,
    /// `None` when the line has since changed (outdated).
    pub line: Option<u64>,
    pub body: String,
}

impl Event {
    pub fn time(&self) -> i64 {
        match self {
            Event::Committed { time, .. }
            | Event::Commented { time, .. }
            | Event::Reviewed { time, .. }
            | Event::Action { time, .. } => *time,
        }
    }
}

fn login(v: &Value) -> String {
    v["login"].as_str().unwrap_or("ghost").to_string()
}

fn time_of(v: &Value, key: &str) -> i64 {
    v[key].as_str().and_then(parse_time).unwrap_or(0)
}

/// Reads the issue timeline API; events Sawit doesn't show are skipped.
pub fn parse_timeline(v: &Value) -> Vec<Event> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let event = e["event"].as_str()?;
            let action = |kind: ActionKind, detail: String| {
                Some(Event::Action {
                    actor: login(&e["actor"]),
                    time: time_of(e, "created_at"),
                    kind,
                    detail,
                })
            };
            match event {
                "committed" => Some(Event::Committed {
                    author: e["author"]["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    time: e["author"]["date"]
                        .as_str()
                        .and_then(parse_time)
                        .unwrap_or(0),
                    sha: e["sha"].as_str()?.to_string(),
                    message: e["message"]
                        .as_str()
                        .unwrap_or_default()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                }),
                "commented" => Some(Event::Commented {
                    author: login(&e["user"]),
                    time: time_of(e, "created_at"),
                    body: e["body"].as_str().unwrap_or_default().to_string(),
                }),
                "reviewed" => Some(Event::Reviewed {
                    author: login(&e["user"]),
                    time: time_of(e, "submitted_at"),
                    state: match e["state"].as_str() {
                        Some("approved") => ReviewState::Approved,
                        Some("changes_requested") => ReviewState::ChangesRequested,
                        Some("dismissed") => ReviewState::Dismissed,
                        _ => ReviewState::Commented,
                    },
                    body: e["body"].as_str().unwrap_or_default().to_string(),
                    id: e["id"].as_u64().unwrap_or(0),
                    comments: Vec::new(),
                }),
                "merged" => action(
                    ActionKind::Merged,
                    e["commit_id"]
                        .as_str()
                        .map(|c| c[..c.len().min(7)].to_string())
                        .unwrap_or_default(),
                ),
                "closed" => action(ActionKind::Closed, String::new()),
                "reopened" => action(ActionKind::Reopened, String::new()),
                "renamed" => action(
                    ActionKind::Renamed,
                    e["rename"]["to"].as_str().unwrap_or_default().to_string(),
                ),
                "labeled" => action(
                    ActionKind::Labeled,
                    e["label"]["name"].as_str().unwrap_or_default().to_string(),
                ),
                "unlabeled" => action(
                    ActionKind::Unlabeled,
                    e["label"]["name"].as_str().unwrap_or_default().to_string(),
                ),
                "review_requested" => action(
                    ActionKind::ReviewRequested,
                    e["requested_reviewer"]["login"]
                        .as_str()
                        .or(e["requested_team"]["name"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
                "assigned" => action(ActionKind::Assigned, login(&e["assignee"])),
                "head_ref_force_pushed" => action(ActionKind::ForcePushed, String::new()),
                "head_ref_deleted" => action(ActionKind::BranchDeleted, String::new()),
                "ready_for_review" => action(ActionKind::ReadyForReview, String::new()),
                "convert_to_draft" => action(ActionKind::MarkedDraft, String::new()),
                "cross-referenced" => {
                    let issue = &e["source"]["issue"];
                    let number = issue["number"].as_u64()?;
                    action(
                        ActionKind::Referenced,
                        format!("#{number} {}", issue["title"].as_str().unwrap_or_default()),
                    )
                }
                _ => None,
            }
        })
        .collect()
}

/// Inline review comments, by the review they belong to.
pub fn parse_review_comments(v: &Value) -> Vec<(u64, ReviewComment)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["pull_request_review_id"].as_u64()?,
                ReviewComment {
                    path: c["path"].as_str().unwrap_or_default().to_string(),
                    line: c["line"].as_u64(),
                    body: c["body"].as_str().unwrap_or_default().to_string(),
                },
            ))
        })
        .collect()
}

/// Puts inline comments under their reviews.
pub fn attach_review_comments(events: &mut [Event], comments: Vec<(u64, ReviewComment)>) {
    for (review, comment) in comments {
        if let Some(Event::Reviewed { comments, .. }) = events
            .iter_mut()
            .find(|e| matches!(e, Event::Reviewed { id, .. } if *id == review))
        {
            comments.push(comment);
        }
    }
}

/// A page (100 events) of the timeline, newest pages later. For a pull
/// request, its reviews carry their inline comments.
pub async fn timeline(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    pull: bool,
    page: u32,
) -> Result<Vec<Event>> {
    let v = api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues/{number}/timeline?per_page=100&page={page}"),
        token,
        None,
    )
    .await?;
    let mut events = parse_timeline(&v);
    if pull && events.iter().any(|e| matches!(e, Event::Reviewed { .. })) {
        let v = api(
            "GET",
            &format!("/repos/{owner}/{repo}/pulls/{number}/comments?per_page=100"),
            token,
            None,
        )
        .await?;
        attach_review_comments(&mut events, parse_review_comments(&v));
    }
    Ok(events)
}

/// Events per timeline page; a full page means there may be more.
pub const TIMELINE_PAGE: usize = 100;

/// A timeline row: consecutive commits by one author become one push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Pushed {
        author: String,
        time: i64,
        /// Short hash and first line.
        commits: Vec<(String, String)>,
    },
    Event(Event),
}

pub fn group(events: &[Event]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for event in events {
        if let Event::Committed {
            author,
            time,
            sha,
            message,
        } = event
        {
            let commit = (sha[..sha.len().min(7)].to_string(), message.clone());
            if let Some(Entry::Pushed {
                author: a,
                time: t,
                commits,
            }) = entries.last_mut()
                && a == author
            {
                commits.push(commit);
                *t = (*t).max(*time);
                continue;
            }
            entries.push(Entry::Pushed {
                author: author.clone(),
                time: *time,
                commits: vec![commit],
            });
        } else {
            entries.push(Entry::Event(event.clone()));
        }
    }
    entries
}

pub async fn merge_pull(token: &str, owner: &str, repo: &str, number: u64) -> Result<()> {
    api(
        "PUT",
        &format!("/repos/{owner}/{repo}/pulls/{number}/merge"),
        token,
        Some(json!({ "merge_method": "merge" })),
    )
    .await
    .map(drop)
}

pub async fn create_pull(
    token: &str,
    owner: &str,
    repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<Item> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/pulls"),
        token,
        Some(json!({ "head": head, "base": base, "title": title, "body": body })),
    )
    .await?;
    let mut item = parse_items(&Value::Array(vec![v])).remove(0);
    item.pull = true;
    Ok(item)
}

pub async fn create_issue(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
) -> Result<Item> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/issues"),
        token,
        Some(json!({ "title": title, "body": body })),
    )
    .await?;
    Ok(parse_items(&Value::Array(vec![v])).remove(0))
}

/// The repository's default branch, for new pull requests.
pub async fn default_branch(token: &str, owner: &str, repo: &str) -> Result<String> {
    let v = api("GET", &format!("/repos/{owner}/{repo}"), token, None).await?;
    Ok(v["default_branch"].as_str().unwrap_or("main").to_string())
}

// ---------------------------------------------------------------- time

/// Unix seconds from GitHub's `2024-01-02T03:04:05Z`.
pub fn parse_time(s: &str) -> Option<i64> {
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    // `…T10:00:00+07:00`: apply the offset.
    let (time, offset) = match time.find(['+', '-']) {
        Some(at) => {
            let (t, o) = time.split_at(at);
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let (oh, om) = o[1..].split_once(':').unwrap_or((&o[1..], "0"));
            (
                t,
                sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60),
            )
        }
        None => (time, 0),
    };
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.splitn(3, ':').map(|p| p.parse::<i64>());
    let (h, min, sec) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + min * 60 + sec - offset)
}

/// "just now", "5 min ago", "3 days ago", … for a Unix time.
pub fn ago(then: i64, now: i64) -> String {
    let s = (now - then).max(0);
    let (n, unit) = match s {
        0..60 => return "just now".into(),
        60..3600 => (s / 60, "min"),
        3600..86400 => (s / 3600, "hour"),
        86400..2_592_000 => (s / 86400, "day"),
        2_592_000..31_536_000 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    let plural = if n == 1 || unit == "min" { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_code_and_poll_answers() {
        let d = parse_device_code(&json!({
            "device_code": "dc", "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device", "interval": 5, "expires_in": 899
        }))
        .unwrap();
        assert_eq!(d.user_code, "ABCD-1234");
        assert_eq!(d.expires_in, 899);
        assert_eq!(parse_device_code(&json!({"error": "x"})), None);
        assert_eq!(
            parse_poll(&json!({"access_token": "t"})),
            Poll::Token("t".into())
        );
        assert_eq!(
            parse_poll(&json!({"error": "authorization_pending"})),
            Poll::Pending
        );
        assert_eq!(parse_poll(&json!({"error": "slow_down"})), Poll::SlowDown);
        assert!(matches!(
            parse_poll(&json!({"error": "access_denied"})),
            Poll::Failed(_)
        ));
    }

    #[test]
    fn items_tell_pull_requests_from_issues() {
        let items = parse_items(&json!([
            {"number": 3, "title": "Fix", "user": {"login": "ann"}, "state": "open",
             "pull_request": {"url": "x"}, "comments": 2, "created_at": "2024-01-02T03:04:05Z",
             "updated_at": "2024-01-02T03:04:05Z", "html_url": "u", "body": null},
            {"number": 4, "title": "Bug", "user": {"login": "bo"}, "state": "closed"}
        ]));
        assert_eq!(items.len(), 2);
        assert!(items[0].pull && items[0].open && items[0].body.is_empty());
        assert!(!items[1].pull && !items[1].open);
    }

    #[test]
    fn repos_and_comments_parse() {
        let repos = parse_repos(
            &json!([{"full_name": "a/b", "description": "", "private": true,
            "clone_url": "https://github.com/a/b.git", "pushed_at": "1970-01-02T00:00:00Z"}]),
        );
        assert_eq!(repos[0].description, None);
        assert_eq!(repos[0].pushed_at, 86400);
        let c = parse_comments(
            &json!([{"user": {"login": "ann"}, "body": "hi", "created_at": "2000-03-01T00:00:00Z"}]),
        );
        assert_eq!(c[0].created_at, 951868800);
    }

    #[test]
    fn timeline_events_and_review_comments() {
        let mut events = parse_timeline(&json!([
            {"event": "committed", "sha": "a1f3c2e9", "message": "IG review\n\nbody", "author": {"name": "Tres", "date": "2024-01-01T00:00:00Z"}},
            {"event": "committed", "sha": "7be019d0", "message": "docs", "author": {"name": "Tres", "date": "2024-01-01T01:00:00Z"}},
            {"event": "reviewed", "id": 9, "user": {"login": "kevin"}, "state": "changes_requested", "body": "Add a down step", "submitted_at": "2024-01-02T00:00:00Z"},
            {"event": "commented", "user": {"login": "tres"}, "body": "Done", "created_at": "2024-01-03T00:00:00Z"},
            {"event": "committed", "sha": "5a38d8b0", "message": "Merge", "author": {"name": "Kev", "date": "2024-01-04T00:00:00Z"}},
            {"event": "labeled", "actor": {"login": "ann"}, "label": {"name": "bug"}, "created_at": "2024-01-04T00:00:00Z"},
            {"event": "subscribed", "actor": {"login": "ann"}},
            {"event": "merged", "actor": {"login": "ann"}, "commit_id": "abcdef1234", "created_at": "2024-01-05T00:00:00Z"}
        ]));
        assert_eq!(events.len(), 7);
        attach_review_comments(
            &mut events,
            parse_review_comments(&json!([
                {"pull_request_review_id": 9, "path": "db.sql", "line": 12, "body": "on conflict"},
                {"pull_request_review_id": 9, "path": "a.ts", "line": null, "body": "moved"}
            ])),
        );
        let Event::Reviewed {
            state, comments, ..
        } = &events[2]
        else {
            panic!()
        };
        assert_eq!(*state, ReviewState::ChangesRequested);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[1].line, None);
        let entries = group(&events);
        assert_eq!(entries.len(), 6);
        let Entry::Pushed {
            author,
            commits,
            time,
        } = &entries[0]
        else {
            panic!()
        };
        assert_eq!(
            (author.as_str(), commits.len(), *time),
            ("Tres", 2, 1704070800)
        );
        assert_eq!(commits[0], ("a1f3c2e".into(), "IG review".into()));
        assert!(matches!(&entries[3], Entry::Pushed { author, .. } if author == "Kev"));
        assert!(
            matches!(&entries[5], Entry::Event(Event::Action { kind: ActionKind::Merged, detail, .. }) if detail == "abcdef1")
        );
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2024-02-29T12:00:00Z"), Some(1709208000));
        assert_eq!(parse_time("nope"), None);
        assert_eq!(parse_time("1970-01-01T07:00:00+07:00"), Some(0));
        assert_eq!(parse_time("1970-01-01T00:00:00-01:30"), Some(5400));
        assert_eq!(ago(0, 30), "just now");
        assert_eq!(ago(0, 120), "2 min ago");
        assert_eq!(ago(0, 3600), "1 hour ago");
        assert_eq!(ago(0, 3 * 86400), "3 days ago");
    }
}
