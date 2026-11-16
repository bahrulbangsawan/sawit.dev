//! Keeping agents from overwriting each other's work. Agents that share a folder take turns in it
//! ([`FolderLocks`]): one turn at a time per folder, whichever agent runs it. An isolated agent
//! ([`Isolation::Worktree`](super::Isolation)) works in a git worktree of its folder made for each
//! thread, on its own branch (`sawit/<thread>`), which the asking agent or a person merges.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::process::Command;

use super::ids::ThreadId;
use super::{OrchestraError, Result};

/// The folders a turn is running in now.
#[derive(Clone, Default)]
pub struct FolderLocks {
    held: Arc<Mutex<HashSet<PathBuf>>>,
}

/// A held folder, given back when dropped.
pub struct FolderGuard {
    held: Arc<Mutex<HashSet<PathBuf>>>,
    path: PathBuf,
}

impl Drop for FolderGuard {
    fn drop(&mut self) {
        self.held
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.path);
    }
}

impl FolderLocks {
    /// Takes `folder` for a turn, or `None` while another turn holds it.
    pub fn try_take(&self, folder: &Path) -> Option<FolderGuard> {
        let path = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        held.insert(path.clone()).then(|| FolderGuard {
            held: self.held.clone(),
            path,
        })
    }
}

/// The branch an isolated thread works on.
pub fn branch(thread: &ThreadId) -> String {
    format!("sawit/{thread}")
}

async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| OrchestraError::Provider(format!("git: {e}")))?;
    if !out.status.success() {
        return Err(OrchestraError::Provider(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The worktree for `thread` of the repository at `repo`, under `root`, made (on a new branch
/// from the repository's HEAD) the first time.
pub async fn worktree(repo: &Path, root: &Path, thread: &ThreadId) -> Result<PathBuf> {
    let dir = root.join(thread.as_str());
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    let top = git(repo, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(|_| {
            OrchestraError::Invalid(format!(
                "{} isn't in a git repository, so it can't be isolated in a worktree",
                repo.display()
            ))
        })?;
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|e| OrchestraError::Provider(format!("{}: {e}", root.display())))?;
    let dir_text = dir.to_string_lossy().into_owned();
    git(
        Path::new(&top),
        &["worktree", "add", "-b", &branch(thread), &dir_text, "HEAD"],
    )
    .await?;
    Ok(dir)
}
