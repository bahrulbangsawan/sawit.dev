//! Two-way sync of a few small files with a repository of the user's: what changed where since
//! the last sync, judged by content hashes. Each file's hash is the one git gives a blob, so the
//! repository's tree says what GitHub holds without downloading it. The shell files and the
//! prompt library both sync this way.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::backup::{base64, base64_decode};
use super::http::api;
use crate::{CoreError, Result};

/// How one file stands, here against GitHub, since the last sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Same,
    ChangedHere,
    #[serde(rename = "changed_on_github")]
    ChangedOnGitHub,
    /// Both sides changed since the last sync (or there never was one from here and they
    /// differ): never overwritten without the user choosing.
    ChangedOnBoth,
    OnlyHere,
    #[serde(rename = "only_on_github")]
    OnlyOnGitHub,
    /// Synced before, now gone here and unchanged on GitHub: an upload deletes it there.
    DeletedHere,
    /// Synced before, now gone on GitHub and unchanged here: a download deletes it here.
    #[serde(rename = "deleted_on_github")]
    DeletedOnGitHub,
}

impl FileState {
    /// As the panel says it.
    pub fn label(self) -> &'static str {
        match self {
            FileState::Same => "Same",
            FileState::ChangedHere => "Changed Here",
            FileState::ChangedOnGitHub => "Changed on GitHub",
            FileState::ChangedOnBoth => "Changed on Both",
            FileState::OnlyHere => "Only Here",
            FileState::OnlyOnGitHub => "Only on GitHub",
            FileState::DeletedHere => "Deleted Here",
            FileState::DeletedOnGitHub => "Deleted on GitHub",
        }
    }

    /// An upload would send it without overwriting a change made on GitHub.
    pub fn uploads(self) -> bool {
        matches!(
            self,
            FileState::ChangedHere | FileState::OnlyHere | FileState::DeletedHere
        )
    }

    /// A download would bring it in without overwriting a change made here.
    pub fn downloads(self) -> bool {
        matches!(
            self,
            FileState::ChangedOnGitHub | FileState::OnlyOnGitHub | FileState::DeletedOnGitHub
        )
    }
}

/// What the last sync from this Mac left: the commit, when, and each file's hash then.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncBase {
    pub commit: Option<String>,
    /// Unix seconds.
    pub synced_at: Option<i64>,
    pub files: BTreeMap<String, String>,
}

impl SyncBase {
    /// The base kept at `path`; none yet (or unreadable) is an empty one: no sync from here.
    pub fn load(path: &Path) -> SyncBase {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// After a sync of `files` (path, hash; `None` for one that is gone on both sides) at
    /// `commit`: the others keep what they had.
    pub fn record(&mut self, commit: &str, now: i64, files: &[(String, Option<String>)]) {
        self.commit = Some(commit.to_string());
        self.synced_at = Some(now);
        for (path, hash) in files {
            match hash {
                Some(h) => self.files.insert(path.clone(), h.clone()),
                None => self.files.remove(path),
            };
        }
    }
}

/// The hash git gives a blob of `bytes`: `sha1("blob <len>\0" + bytes)`, as GitHub's tree says.
pub fn blob_sha(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    let mut hash = sha1::Sha1::new();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// How each file stands: `local` and `remote` map paths to hashes; `base` is the last sync.
/// Sorted by path.
pub fn plan_sync(
    local: &BTreeMap<String, String>,
    remote: &BTreeMap<String, String>,
    base: &SyncBase,
) -> Vec<(String, FileState)> {
    let paths: BTreeSet<&String> = local.keys().chain(remote.keys()).collect();
    paths
        .into_iter()
        .map(|path| {
            let then = base.files.get(path);
            let state = match (local.get(path), remote.get(path)) {
                (Some(l), Some(r)) if l == r => FileState::Same,
                (Some(l), Some(r)) => match then {
                    Some(b) if b == l => FileState::ChangedOnGitHub,
                    Some(b) if b == r => FileState::ChangedHere,
                    _ => FileState::ChangedOnBoth,
                },
                // Synced before and gone on one side: a delete there, carried over when the
                // other side hasn't changed since; changed there, it is a conflict.
                (Some(l), None) => match then {
                    Some(b) if b == l => FileState::DeletedOnGitHub,
                    Some(_) => FileState::ChangedOnBoth,
                    None => FileState::OnlyHere,
                },
                (None, Some(r)) => match then {
                    Some(b) if b == r => FileState::DeletedHere,
                    Some(_) => FileState::ChangedOnBoth,
                    None => FileState::OnlyOnGitHub,
                },
                (None, None) => FileState::OnlyOnGitHub,
            };
            (path.clone(), state)
        })
        .collect()
}

/// A branch of a repository as a sync reads it: its head commit and every file's hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteTree {
    pub commit: String,
    pub files: BTreeMap<String, String>,
}

/// The files GitHub's recursive tree lists: blobs only, by path.
pub fn parse_tree(v: &Value) -> BTreeMap<String, String> {
    v["tree"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| {
            Some((
                e["path"].as_str()?.to_string(),
                e["sha"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The head of `branch` in `owner/name` and the hash of every file in it.
pub async fn fetch_tree(token: &str, owner: &str, name: &str, branch: &str) -> Result<RemoteTree> {
    let repo = format!("/repos/{owner}/{name}");
    let head = api(
        "GET",
        &format!("{repo}/git/ref/heads/{branch}"),
        token,
        None,
    )
    .await?;
    let commit = head["object"]["sha"]
        .as_str()
        .ok_or_else(|| CoreError::GitHub("GitHub didn't say where the branch is.".into()))?
        .to_string();
    let tree = api(
        "GET",
        &format!("{repo}/git/trees/{commit}?recursive=1"),
        token,
        None,
    )
    .await?;
    Ok(RemoteTree {
        commit,
        files: parse_tree(&tree),
    })
}

/// One file's bytes, by its hash, from `owner/name`.
pub async fn fetch_blob(token: &str, owner: &str, name: &str, sha: &str) -> Result<Vec<u8>> {
    let v = api(
        "GET",
        &format!("/repos/{owner}/{name}/git/blobs/{sha}"),
        token,
        None,
    )
    .await?;
    let content = v["content"].as_str().unwrap_or_default();
    let bytes = base64_decode(content)
        .ok_or_else(|| CoreError::GitHub("GitHub sent a file Sawit can't read.".into()))?;
    if blob_sha(&bytes) != sha {
        return Err(CoreError::GitHub(
            "GitHub sent a file that doesn't match its hash.".into(),
        ));
    }
    Ok(bytes)
}

/// The blob GitHub keeps for `bytes`, for a tree entry (`json` used by `commit_files`).
pub(super) fn blob_body(bytes: &[u8]) -> Value {
    json!({ "content": base64(bytes), "encoding": "base64" })
}
