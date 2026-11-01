//! Requests to GitHub through `curl`: headers (the token among them) on stdin, a body in a
//! private temporary file, and what GitHub's answer means.

use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::API;
use crate::{CoreError, Result};

/// What one request answered: the HTTP status, the scopes GitHub says the token has (its
/// `X-OAuth-Scopes`, `None` when it didn't say), and the body.
pub(super) struct Reply {
    pub(super) code: u16,
    pub(super) scopes: Option<Vec<String>>,
    pub(super) body: String,
}

/// What `curl -w` prints after the body: the scopes header and the status, a line each.
const WRITE_OUT: &str = "\n%header{x-oauth-scopes}\n%{http_code}";

/// Splits what `curl` printed with [`WRITE_OUT`] into a [`Reply`]. A curl too old for
/// `%header{…}` prints it as is: no scopes then, rather than wrong ones.
pub(super) fn parse_reply(text: &str) -> Reply {
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
pub(super) async fn curl(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<&str>,
) -> Result<Reply> {
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
pub(super) async fn api(
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> Result<Value> {
    let text = api_raw(method, path, token, body, "application/vnd.github+json").await?;
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Like [`api`], asking for the `accept` media type and returning the text
/// (a diff, say).
pub(super) async fn api_raw(
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
pub(super) async fn api_reply(
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
