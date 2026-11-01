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

// ---------------------------------------------------------------- token

/// Where the sign-in is kept. The app keeps it in the Keychain ([`Keychain::user`]); its tests
/// keep theirs in memory ([`MemoryStore`]), so no test signs the user in or out.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<String>>;
    fn save(&self, token: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

/// A Keychain item holding the token: the one the app signs in with, or a test's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keychain {
    service: String,
    account: String,
}

impl Keychain {
    /// The user's sign-in: every GitHub control in the app works with it. A Sawit run with a
    /// data folder of its own (`SAWIT_HOME`: a scratch copy, the demo data the README's
    /// screenshots are taken with) keeps a sign-in of its own too, so it neither shows nor
    /// signs out the user's GitHub account.
    pub fn user() -> Self {
        Keychain {
            service: "sawit".into(),
            account: user_account(std::env::var_os("SAWIT_HOME").as_deref()),
        }
    }

    /// An item of a test's own, under another service, away from the user's.
    pub fn named(service: &str, account: &str) -> Self {
        Keychain {
            service: service.into(),
            account: account.into(),
        }
    }
}

/// The Keychain account of the GitHub sign-in: `github`, or with a data folder of its own
/// (`SAWIT_HOME`) `github:<that folder>`.
fn user_account(home: Option<&std::ffi::OsStr>) -> String {
    match home.filter(|h| !h.is_empty()) {
        Some(home) => format!("github:{}", home.to_string_lossy()),
        None => "github".into(),
    }
}

impl TokenStore for Keychain {
    fn load(&self) -> Result<Option<String>> {
        crate::keychain::get(&self.service, &self.account)
    }

    fn save(&self, token: &str) -> Result<()> {
        crate::keychain::set(&self.service, &self.account, token)
    }

    fn delete(&self) -> Result<()> {
        crate::keychain::delete(&self.service, &self.account)
    }
}

/// A token kept in memory only, for tests.
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

impl MemoryStore {
    pub fn with(token: &str) -> Self {
        MemoryStore(std::sync::Mutex::new(Some(token.to_string())))
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<String>> {
        Ok(self.0.lock().map(|t| t.clone()).unwrap_or_default())
    }

    fn save(&self, token: &str) -> Result<()> {
        if let Ok(mut t) = self.0.lock() {
            *t = Some(token.to_string());
        }
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        if let Ok(mut t) = self.0.lock() {
            *t = None;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- http

/// What one request answered: the HTTP status, the scopes GitHub says the token has (its
/// `X-OAuth-Scopes`, `None` when it didn't say), and the body.
struct Reply {
    code: u16,
    scopes: Option<Vec<String>>,
    body: String,
}

/// What `curl -w` prints after the body: the scopes header and the status, a line each.
const WRITE_OUT: &str = "\n%header{x-oauth-scopes}\n%{http_code}";

/// Splits what `curl` printed with [`WRITE_OUT`] into a [`Reply`]. A curl too old for
/// `%header{…}` prints it as is: no scopes then, rather than wrong ones.
fn parse_reply(text: &str) -> Reply {
    let (rest, code) = text.rsplit_once('\n').unwrap_or(("", text));
    let (body, scopes) = rest.rsplit_once('\n').unwrap_or(("", rest));
    let scopes = scopes.trim();
    Reply {
        code: code.trim().parse().unwrap_or(0),
        scopes: (!scopes.contains('%') && !scopes.contains('{')).then(|| {
            scopes
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        }),
        body: body.to_string(),
    }
}

/// One request with `curl`. `header` lines (the token among them) go in on
/// stdin and the body in a private temporary file, so neither shows up in
/// `ps`; the HTTP status comes back with the body.
async fn curl(method: &str, url: &str, headers: &[String], body: Option<&str>) -> Result<Reply> {
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
        WRITE_OUT,
    ]);
    let body_file = body.map(write_private).transpose()?;
    if let Some(file) = &body_file {
        cmd.arg("--data-binary")
            .arg(format!("@{}", file.0.display()));
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
    Ok(parse_reply(&String::from_utf8_lossy(&out.stdout)))
}

/// What a call says when GitHub no longer accepts the token it was made with (401: revoked or
/// expired). Said where the user is signed in, so Sign Out is at hand.
pub const REVOKED: &str = "GitHub no longer accepts this sign-in. Sign out, then sign in again.";

/// Whether `error` is GitHub refusing the token itself ([`REVOKED`]), not a call that couldn't
/// reach GitHub or that GitHub refused for another reason.
pub fn is_revoked(error: &CoreError) -> bool {
    matches!(error, CoreError::GitHub(message) if message == REVOKED)
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
    api_reply(method, path, token, body, accept)
        .await
        .map(|reply| reply.body)
}

/// Like [`api_raw`], with the scopes GitHub says the token has.
async fn api_reply(
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
    accept: &str,
) -> Result<Reply> {
    let headers = vec![
        format!("Authorization: Bearer {token}"),
        format!("Accept: {accept}"),
        "X-GitHub-Api-Version: 2022-11-28".to_string(),
        "Content-Type: application/json".to_string(),
    ];
    let body = body.map(|b| b.to_string());
    let reply = curl(method, &format!("{API}{path}"), &headers, body.as_deref()).await?;
    let (code, text) = (reply.code, &reply.body);
    match code {
        200..=299 => Ok(reply),
        401 => Err(CoreError::GitHub(REVOKED.into())),
        _ => {
            let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
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

/// A file only this user can read, removed when dropped.
struct PrivateFile(std::path::PathBuf);

impl Drop for PrivateFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_private(text: &str) -> Result<PrivateFile> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "sawit-github-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let guard = PrivateFile(path);
    file.write_all(text.as_bytes())?;
    Ok(guard)
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
        scope_list().replace(' ', "%20")
    );
    let Reply {
        code, body: text, ..
    } = curl(
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

/// Polls until the user approves on github.com, and returns the token for the caller to keep
/// (in its [`TokenStore`]).
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
        let reply = curl(
            "POST",
            "https://github.com/login/oauth/access_token",
            &["Accept: application/json".into()],
            Some(&body),
        )
        .await?;
        match parse_poll(&serde_json::from_str(&reply.body).unwrap_or(Value::Null)) {
            Poll::Token(token) => return Ok(token),
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    /// The picture GitHub shows for the account, on its avatars host.
    pub avatar_url: Option<String>,
    /// What the token may do, as GitHub says (`X-OAuth-Scopes`); `None` when it didn't say.
    pub scopes: Option<Vec<String>>,
}

pub fn parse_account(v: &Value, scopes: Option<Vec<String>>) -> Account {
    Account {
        login: v["login"].as_str().unwrap_or_default().to_string(),
        name: v["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(String::from),
        avatar_url: v["avatar_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .map(String::from),
        scopes,
    }
}

pub async fn account(token: &str) -> Result<Account> {
    let reply = api_reply("GET", "/user", token, None, "application/vnd.github+json").await?;
    let v: Value = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
    Ok(parse_account(&v, reply.scopes))
}

/// The account's picture, small: a public address, so it's fetched without the token. Bytes of
/// a PNG or JPEG, which the caller sniffs.
pub async fn avatar(url: &str) -> Result<Vec<u8>> {
    let sized = if url.contains('?') {
        format!("{url}&s=64")
    } else {
        format!("{url}?s=64")
    };
    let out = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new("/usr/bin/curl")
            .args(["-sSfL", "--max-time", "15", "--max-filesize", "1000000"])
            .arg(&sized)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| CoreError::GitHub("GitHub didn't send the avatar in time.".into()))??;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(CoreError::GitHub("GitHub didn't send the avatar.".into()));
    }
    Ok(out.stdout)
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

// ---------------------------------------------------------------- backup

/// A repository of the signed-in user, as far as a backup cares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnRepo {
    pub private: bool,
    pub default_branch: String,
    pub html_url: String,
}

/// `owner/name`, or `None` when it doesn't exist (or isn't visible).
pub async fn own_repo(token: &str, owner: &str, name: &str) -> Result<Option<OwnRepo>> {
    match api("GET", &format!("/repos/{owner}/{name}"), token, None).await {
        Ok(v) => Ok(Some(parse_own_repo(&v))),
        Err(CoreError::GitHub(m)) if m.starts_with("GitHub refused (404)") => Ok(None),
        Err(e) => Err(e),
    }
}

fn parse_own_repo(v: &Value) -> OwnRepo {
    OwnRepo {
        private: v["private"].as_bool().unwrap_or(false),
        default_branch: v["default_branch"].as_str().unwrap_or("main").to_string(),
        html_url: v["html_url"].as_str().unwrap_or_default().to_string(),
    }
}

/// Creates a private repository with a first commit, so files can be
/// committed to it right away.
pub async fn create_private_repo(token: &str, name: &str, description: &str) -> Result<OwnRepo> {
    let v = api(
        "POST",
        "/user/repos",
        token,
        Some(json!({
            "name": name,
            "description": description,
            "private": true,
            "auto_init": true,
        })),
    )
    .await?;
    Ok(parse_own_repo(&v))
}

/// Commits `files` (`path in the repository`, contents) on top of `branch`
/// in one commit. Files already there that aren't listed stay. Returns
/// `false` when nothing changed, and then commits nothing.
pub async fn commit_files(
    token: &str,
    owner: &str,
    name: &str,
    branch: &str,
    files: &[(String, Vec<u8>)],
    message: &str,
) -> Result<bool> {
    let repo = format!("/repos/{owner}/{name}");
    // A repository created a moment ago may not have its branch yet.
    let mut head = None;
    for attempt in 0..5 {
        match api(
            "GET",
            &format!("{repo}/git/ref/heads/{branch}"),
            token,
            None,
        )
        .await
        {
            Ok(v) => {
                head = v["object"]["sha"].as_str().map(String::from);
                break;
            }
            Err(e) if attempt == 4 => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    }
    let head =
        head.ok_or_else(|| CoreError::GitHub("GitHub didn't say where the branch is.".into()))?;
    let commit = api("GET", &format!("{repo}/git/commits/{head}"), token, None).await?;
    let base_tree = commit["tree"]["sha"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let mut entries = Vec::new();
    for (path, bytes) in files {
        let blob = api(
            "POST",
            &format!("{repo}/git/blobs"),
            token,
            Some(json!({ "content": base64(bytes), "encoding": "base64" })),
        )
        .await?;
        let sha = blob["sha"].as_str().unwrap_or_default();
        entries.push(json!({ "path": path, "mode": "100644", "type": "blob", "sha": sha }));
    }
    let tree = api(
        "POST",
        &format!("{repo}/git/trees"),
        token,
        Some(json!({ "base_tree": base_tree, "tree": entries })),
    )
    .await?;
    let tree = tree["sha"].as_str().unwrap_or_default().to_string();
    if tree == base_tree {
        return Ok(false);
    }
    let new = api(
        "POST",
        &format!("{repo}/git/commits"),
        token,
        Some(json!({ "message": message, "tree": tree, "parents": [head] })),
    )
    .await?;
    let sha = new["sha"].as_str().unwrap_or_default();
    api(
        "PATCH",
        &format!("{repo}/git/refs/heads/{branch}"),
        token,
        Some(json!({ "sha": sha })),
    )
    .await?;
    Ok(true)
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"export A=1\n"), "ZXhwb3J0IEE9MQo=");
    }

    /// The one sign-in asks for every scope a GitHub control needs: `repo` for pull requests,
    /// issues, private clones, HTTPS push and the shell files' private repository; `workflow`,
    /// without which GitHub refuses a push that changes `.github/workflows`; and the profile.
    #[test]
    fn the_sign_in_asks_for_what_every_control_needs() {
        assert_eq!(scope_list(), "repo workflow read:user");
        let granted = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(missing_scopes(&granted(&["repo", "workflow", "read:user"])).is_empty());
        assert!(missing_scopes(&granted(&["repo", "workflow", "user"])).is_empty());
        // A sign-in from before Sawit asked for `workflow`.
        assert_eq!(
            missing_scopes(&granted(&["read:user", "repo"])),
            ["workflow"]
        );
        assert_eq!(
            missing_scopes(&[]),
            ["repo", "workflow", "read:user"],
            "nothing granted lacks everything"
        );
    }

    /// The status and the scopes header come after the body, a line each; a curl too old for
    /// `%header{…}` leaves the scopes unknown instead of making some up.
    #[test]
    fn a_reply_carries_its_status_and_scopes() {
        let r = parse_reply("{\"login\":\"me\"}\nrepo, workflow,read:user\n200");
        assert_eq!(r.code, 200);
        assert_eq!(r.body, "{\"login\":\"me\"}");
        assert_eq!(
            r.scopes.as_deref(),
            Some(&["repo".to_string(), "workflow".into(), "read:user".into()][..])
        );
        let r = parse_reply("line one\nline two\n\n404");
        assert_eq!((r.code, r.body.as_str()), (404, "line one\nline two"));
        assert_eq!(r.scopes, Some(Vec::new()), "an empty header is no scopes");
        let r = parse_reply("{}\n%header{x-oauth-scopes}\n200");
        assert_eq!((r.code, r.scopes), (200, None));
    }

    #[test]
    fn accounts_parse() {
        let a = parse_account(
            &json!({"login": "me", "name": "", "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4"}),
            None,
        );
        assert_eq!(a.login, "me");
        assert_eq!(a.name, None, "an empty name is none");
        assert_eq!(
            a.avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/1?v=4")
        );
        let a = parse_account(
            &json!({"login": "me", "avatar_url": "file:///etc/passwd"}),
            None,
        );
        assert_eq!(a.avatar_url, None, "only an https picture is fetched");
    }

    /// The app's sign-in is the `sawit`/`github` Keychain item; a Sawit with a data folder of its
    /// own keeps its own, so a scratch or demo copy never shows (nor signs out) the user's.
    #[test]
    fn a_separate_data_folder_keeps_a_separate_sign_in() {
        assert_eq!(user_account(None), "github");
        assert_eq!(user_account(Some("".as_ref())), "github");
        assert_eq!(
            user_account(Some("/Users/Shared/sawit-demo".as_ref())),
            "github:/Users/Shared/sawit-demo"
        );
    }

    #[test]
    fn a_memory_store_keeps_one_token() {
        let store = MemoryStore::default();
        assert_eq!(store.load().unwrap(), None);
        store.save("t1").unwrap();
        store.save("t2").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("t2"));
        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    /// The Keychain store, on an item of the test's own: never the user's sign-in.
    #[test]
    #[ignore = "writes to the login Keychain"]
    fn a_keychain_store_round_trips_on_its_own_item() {
        let store = Keychain::named("sawit.github-test", "probe");
        assert_ne!(store, Keychain::user());
        store.save("gho_test").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("gho_test"));
        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

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
