//! A private repository of the user's own, and committing files to it in one commit: where the
//! shell config backup goes.

use std::time::Duration;

use serde_json::{Value, json};

use super::http::api;
use crate::{CoreError, Result};

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

/// What [`commit_files`] did: the branch's head after it, and whether it made a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    pub commit: String,
    /// `false` when GitHub already had these exact files: nothing was committed.
    pub changed: bool,
}

/// Commits `files` (`path in the repository`, contents) on top of `branch`
/// in one commit. Files already there that aren't listed stay. When nothing
/// changed it commits nothing, and says so.
pub async fn commit_files(
    token: &str,
    owner: &str,
    name: &str,
    branch: &str,
    files: &[(String, Vec<u8>)],
    message: &str,
) -> Result<Committed> {
    commit_changes(token, owner, name, branch, files, &[], message).await
}

/// [`commit_files`], also deleting the files at `deletes` (paths in the repository) in the
/// same commit.
pub async fn commit_changes(
    token: &str,
    owner: &str,
    name: &str,
    branch: &str,
    files: &[(String, Vec<u8>)],
    deletes: &[String],
    message: &str,
) -> Result<Committed> {
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
            Some(super::sync::blob_body(bytes)),
        )
        .await?;
        let sha = blob["sha"].as_str().unwrap_or_default();
        entries.push(json!({ "path": path, "mode": "100644", "type": "blob", "sha": sha }));
    }
    // A tree entry with no blob deletes the file from the base tree.
    for path in deletes {
        entries.push(json!({ "path": path, "mode": "100644", "type": "blob", "sha": null }));
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
        return Ok(Committed {
            commit: head,
            changed: false,
        });
    }
    let new = api(
        "POST",
        &format!("{repo}/git/commits"),
        token,
        Some(json!({ "message": message, "tree": tree, "parents": [head] })),
    )
    .await?;
    let sha = new["sha"].as_str().unwrap_or_default().to_string();
    api(
        "PATCH",
        &format!("{repo}/git/refs/heads/{branch}"),
        token,
        Some(json!({ "sha": sha })),
    )
    .await?;
    Ok(Committed {
        commit: sha,
        changed: true,
    })
}

/// Standard base64 with padding.
pub(super) fn base64(bytes: &[u8]) -> String {
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

/// Standard base64 back to bytes; GitHub wraps it in lines. `None` when it isn't base64.
pub(super) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body = clean
        .strip_suffix(b"==")
        .or_else(|| clean.strip_suffix(b"="))
        .unwrap_or(&clean);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}
