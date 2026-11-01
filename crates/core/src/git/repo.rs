//! The repository itself: finding it, making one, and adding to its ignore files.

use std::path::{Path, PathBuf};

use super::{LOCAL_TIMEOUT, git, git_ok};
use crate::{CoreError, Result};

/// The top folder of the repository containing `dir`, if it is in one.
pub async fn discover(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"], None, LOCAL_TIMEOUT)
        .await
        .ok()?;
    out.ok
        .then(|| PathBuf::from(out.stdout.trim()))
        .filter(|p| !p.as_os_str().is_empty())
}

/// Adds `path`, a file or folder in the repository whose top folder is `repo`, to its
/// `.gitignore`, or with `exclude` to its private `info/exclude`: one line, anchored at the top
/// folder ([`crate::files::ignore_pattern`]). False when the line was there already.
///
/// A link is ignored as itself, not as what it leads to: git names the repository by its real
/// path, so the links on the way to `path` are resolved (the tree may name the project through
/// one, `/tmp` for `/private/tmp`), but not `path` itself. A name with a line break is refused,
/// as it would make a second line, and spaces that end a name are kept.
pub async fn ignore(repo: &Path, path: &Path, exclude: bool) -> Result<bool> {
    let outside = || CoreError::Config("This isn't inside the repository.".into());
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(outside());
    };
    let real = std::fs::canonicalize(parent)
        .unwrap_or_else(|_| parent.to_path_buf())
        .join(name);
    let rel = real.strip_prefix(repo).map_err(|_| outside())?;
    if rel.as_os_str().is_empty() {
        return Err(CoreError::Config(
            "The repository's own folder can't be ignored.".into(),
        ));
    }
    if rel.to_string_lossy().contains(['\n', '\r']) {
        return Err(CoreError::Config(
            "Its name has a line break, which a line of an ignore file can't hold.".into(),
        ));
    }
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    let line = crate::files::ignore_pattern(rel, is_dir);
    let file = if exclude {
        git_path(repo, "info/exclude").await?
    } else {
        repo.join(".gitignore")
    };
    tokio::task::spawn_blocking(move || crate::files::add_ignore_line(&file, &line))
        .await
        .map_err(std::io::Error::other)?
}

/// Where the repository at `repo` keeps `path` of its git folder (`git rev-parse --git-path`).
/// In a linked worktree or a submodule `.git` is a file naming the real folder, and some of its
/// files, `info/exclude` among them, are shared by every worktree of the repository.
pub async fn git_path(repo: &Path, path: &str) -> Result<PathBuf> {
    let out = git_ok(
        repo,
        &["rev-parse", "--git-path", path],
        None,
        LOCAL_TIMEOUT,
    )
    .await?;
    // Relative to `repo`, git's working folder here, unless it is absolute.
    Ok(repo.join(out.trim_end_matches(['\n', '\r'])))
}

/// Makes `dir` a new repository with `main` as its first branch.
pub async fn init(dir: &Path) -> Result<()> {
    git_ok(dir, &["init", "-q", "-b", "main"], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}
