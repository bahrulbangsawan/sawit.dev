//! Two-way sync of the user's shell files with their private `sawit-shell-config` repository:
//! how each file stands ([`sync_status`]), sending some up ([`upload`]), and bringing some down
//! ([`apply_download`]), each local file backed up beside itself first so it can be put back
//! ([`undo_download`]). Only the user's own startup files under the home folder are ever written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::backup::{BACKUP_REPO, backup_files};
use super::{USER, USER_CONFIG};
use crate::github::{self, FileState, OwnRepo, RemoteTree, SyncBase};
use crate::{CoreError, Result};

/// The file in Sawit's data folder that keeps the last sync from this Mac.
pub const SYNC_STATE: &str = "shell-files-sync.json";

/// The repository, found: what a sync works against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub login: String,
    pub repo: OwnRepo,
    pub tree: RemoteTree,
}

/// One file as the sync sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSync {
    /// Its path in the repository: its path under the home folder (`.zshrc`).
    pub rel: String,
    /// Where it is (or would be) on this Mac.
    pub path: PathBuf,
    pub state: FileState,
    /// Its hash on GitHub, when it is there.
    pub remote: Option<String>,
}

/// How the shell files stand against GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatus {
    pub login: String,
    /// `None`: no repository yet; Save Shell Files to GitHub makes it.
    pub backup: Option<Backup>,
    pub files: Vec<FileSync>,
    /// Unix seconds of the last sync from this Mac.
    pub synced_at: Option<i64>,
}

/// Every user file a shell reads that the sync may write: the startup files under the home
/// folder, whether they exist or not, by their path in the repository. Nothing else is written.
pub fn known_files(home: &Path, config: &Path) -> Vec<(String, PathBuf)> {
    USER.iter()
        .map(|(_, rel, _)| home.join(rel))
        .chain(USER_CONFIG.iter().map(|(_, rel, _)| config.join(rel)))
        .filter_map(|path| {
            let rel = path.strip_prefix(home).ok()?.to_string_lossy().to_string();
            (!rel.is_empty() && !rel.starts_with("..")).then_some((rel, path))
        })
        .collect()
}

/// [`known_files`] for this user.
pub fn known_files_here() -> Vec<(String, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    known_files(&home, &config)
}

/// The hashes of the files here (`local`: path in the repository, file) that exist.
pub fn local_hashes(local: &[(String, PathBuf)]) -> BTreeMap<String, String> {
    local
        .iter()
        .filter_map(|(rel, path)| Some((rel.clone(), github::blob_sha(&std::fs::read(path).ok()?))))
        .collect()
}

/// How each file stands: the files here, the repository's tree (only the files the sync knows
/// count), and the last sync. Sorted by path.
pub fn plan_files(
    known: &[(String, PathBuf)],
    local: &BTreeMap<String, String>,
    tree: &RemoteTree,
    base: &SyncBase,
) -> Vec<FileSync> {
    let remote: BTreeMap<String, String> = tree
        .files
        .iter()
        .filter(|(rel, _)| known.iter().any(|(k, _)| k == *rel))
        .map(|(rel, sha)| (rel.clone(), sha.clone()))
        .collect();
    github::plan_sync(local, &remote, base)
        .into_iter()
        .filter_map(|(rel, state)| {
            let path = known.iter().find(|(k, _)| *k == rel)?.1.clone();
            Some(FileSync {
                remote: remote.get(&rel).cloned(),
                rel,
                path,
                state,
            })
        })
        .collect()
}

/// Finds the repository (refusing a public one) and its files on GitHub.
pub async fn fetch_backup(token: &str) -> Result<(String, Option<Backup>)> {
    let login = github::account(token).await?.login;
    let Some(repo) = github::own_repo(token, &login, BACKUP_REPO).await? else {
        return Ok((login, None));
    };
    if !repo.private {
        return Err(CoreError::GitHub(format!(
            "github.com/{login}/{BACKUP_REPO} is public, so Sawit won't sync your shell files with it. Make it private on GitHub, then try again."
        )));
    }
    let tree = github::fetch_tree(token, &login, BACKUP_REPO, &repo.default_branch).await?;
    let backup = Backup {
        login: login.clone(),
        repo,
        tree,
    };
    Ok((login, Some(backup)))
}

/// How the shell files stand against GitHub, with the last sync kept at `state`.
pub async fn sync_status(token: &str, state: &Path) -> Result<SyncStatus> {
    let (login, backup) = fetch_backup(token).await?;
    let base = SyncBase::load(state);
    let known = known_files_here();
    let local_files: Vec<(String, PathBuf)> = backup_files()
        .into_iter()
        .filter(|(rel, _)| known.iter().any(|(k, _)| k == rel))
        .collect();
    let local = local_hashes(&local_files);
    let files = match &backup {
        Some(b) => plan_files(&known, &local, &b.tree, &base),
        None => Vec::new(),
    };
    Ok(SyncStatus {
        login,
        backup,
        files,
        synced_at: base.synced_at,
    })
}

/// GitHub's copy of each of `files` (by its hash), for a diff or a download.
pub async fn fetch_remote(
    token: &str,
    backup: &Backup,
    files: &[FileSync],
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for f in files {
        if let Some(sha) = &f.remote {
            let bytes = github::fetch_blob(token, &backup.login, BACKUP_REPO, sha).await?;
            out.push((f.rel.clone(), bytes));
        }
    }
    Ok(out)
}

/// Commits the files here named `rels` (they must be known shell files that exist), then keeps
/// the sync's state. Returns the commit and whether anything changed.
pub async fn upload(
    token: &str,
    backup: &Backup,
    rels: &[String],
    state: &Path,
    host: &str,
) -> Result<github::Committed> {
    let known = known_files_here();
    let mut base = SyncBase::load(state);
    let mut contents = Vec::new();
    let mut deletes = Vec::new();
    for rel in rels {
        let (_, path) = known
            .iter()
            .find(|(k, _)| k == rel)
            .ok_or_else(|| CoreError::Config(format!("{rel} isn't a shell file Sawit syncs.")))?;
        match std::fs::read(path) {
            Ok(bytes) => contents.push((rel.clone(), bytes)),
            // Synced before and deleted here since: deleted on GitHub too.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && base.files.contains_key(rel) => {
                deletes.push(rel.clone())
            }
            Err(e) => return Err(e.into()),
        }
    }
    if contents.is_empty() && deletes.is_empty() {
        return Err(CoreError::Config(
            "Choose at least one file to upload.".into(),
        ));
    }
    let message = format!("Save shell config from {host}\n\n{}", rels.join("\n"));
    let committed = github::commit_changes(
        token,
        &backup.login,
        BACKUP_REPO,
        &backup.repo.default_branch,
        &contents,
        &deletes,
        &message,
    )
    .await?;
    let synced: Vec<(String, Option<String>)> = contents
        .iter()
        .map(|(rel, bytes)| (rel.clone(), Some(github::blob_sha(bytes))))
        .chain(deletes.iter().map(|rel| (rel.clone(), None)))
        .collect();
    base.record(&committed.commit, github::now(), &synced);
    base.save(state)?;
    Ok(committed)
}

/// What a download did, so Undo can put it back: each file, and the sync's state before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub replaced: Vec<Replaced>,
    pub previous: SyncBase,
}

/// What a download did to one file, so it can be put back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replaced {
    pub path: PathBuf,
    /// The copy of the file as it was, beside it; `None` when there was no file.
    pub backup: Option<PathBuf>,
}

/// `~/.zshrc.sawit-backup-20261106-103502` for `~/.zshrc` at Unix time `now`.
pub fn backup_path(path: &Path, now: i64) -> PathBuf {
    let stamp = github::stamp(now)
        .replace(['-', ':'], "")
        .replace('T', "-")
        .trim_end_matches('Z')
        .to_string();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".sawit-backup-{stamp}"));
    path.with_file_name(name)
}

/// Writes GitHub's `files` (path in the repository, bytes) over the ones here, after backing each
/// up beside itself, and keeps the sync's state at `commit`. Only `known` files are written,
/// keeping their permissions (a new one gets `600`). Refuses all before writing any when one
/// isn't known.
pub fn apply_download(
    known: &[(String, PathBuf)],
    files: &[(String, Vec<u8>)],
    state: &Path,
    commit: &str,
    now: i64,
) -> Result<Downloaded> {
    apply_download_with(known, files, &[], state, commit, now)
}

/// [`apply_download`], also carrying over the files deleted on GitHub since the last sync
/// (`deletes`, by path in the repository): each is moved to its backup beside it, so Undo puts it
/// back, and the sync forgets it.
pub fn apply_download_with(
    known: &[(String, PathBuf)],
    files: &[(String, Vec<u8>)],
    deletes: &[String],
    state: &Path,
    commit: &str,
    now: i64,
) -> Result<Downloaded> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut removing = Vec::new();
    for rel in deletes {
        let (_, path) = known.iter().find(|(k, _)| k == rel).ok_or_else(|| {
            CoreError::Config(format!(
                "{rel} isn't a shell file Sawit syncs: not deleted."
            ))
        })?;
        removing.push((rel, path));
    }
    let mut targets = Vec::new();
    for (rel, bytes) in files {
        let (_, path) = known.iter().find(|(k, _)| k == rel).ok_or_else(|| {
            CoreError::Config(format!(
                "{rel} isn't a shell file Sawit syncs: not written."
            ))
        })?;
        targets.push((rel, path, bytes));
    }
    let mut done = Vec::new();
    for (_, path, bytes) in &targets {
        let mode = std::fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o7777);
        let backup = match mode {
            Some(_) => {
                let to = backup_path(path, now);
                std::fs::copy(path, &to)?;
                Some(to)
            }
            None => None,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_file_name(format!(
            ".{}.sawit-download",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::write(&tmp, bytes)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode.unwrap_or(0o600)))?;
        std::fs::rename(&tmp, path)?;
        done.push(Replaced {
            path: path.to_path_buf(),
            backup,
        });
    }
    for (_, path) in &removing {
        if path.exists() {
            let to = backup_path(path, now);
            std::fs::rename(path, &to)?;
            done.push(Replaced {
                path: path.to_path_buf(),
                backup: Some(to),
            });
        }
    }
    let previous = SyncBase::load(state);
    let mut base = previous.clone();
    let synced: Vec<(String, Option<String>)> = targets
        .iter()
        .map(|(rel, _, bytes)| ((*rel).clone(), Some(github::blob_sha(bytes))))
        .chain(removing.iter().map(|(rel, _)| ((*rel).clone(), None)))
        .collect();
    base.record(commit, now, &synced);
    base.save(state)?;
    Ok(Downloaded {
        replaced: done,
        previous,
    })
}

/// Puts back what [`apply_download`] did: each backup over its file (the backup moved back),
/// a file the download created removed, and the sync's state at `state` as it was.
pub fn undo_download(done: &Downloaded, state: &Path) -> Result<()> {
    for r in &done.replaced {
        match &r.backup {
            Some(backup) => std::fs::rename(backup, &r.path)?,
            None => std::fs::remove_file(&r.path)?,
        }
    }
    done.previous.save(state)
}
