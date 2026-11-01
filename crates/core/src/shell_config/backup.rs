//! Saving the user's own startup files to a private GitHub repository.

use std::path::{Path, PathBuf};

use super::secrets::find_secrets;
use super::{Scope, ShellConfig, detect};
use crate::{CoreError, Result, github};

/// The private repository the user's files are saved to.
pub const BACKUP_REPO: &str = "sawit-shell-config";

/// The user's own startup files (not the system-wide ones) and the path
/// each gets in the repository: its path under the home folder.
pub fn backup_files() -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    backup_files_in(&detect(), &home)
}

pub(super) fn backup_files_in(configs: &[ShellConfig], home: &Path) -> Vec<(String, PathBuf)> {
    configs
        .iter()
        .filter(|c| c.scope == Scope::User)
        .map(|c| {
            let rel = c.path.strip_prefix(home).unwrap_or(&c.path);
            let rel = rel.to_string_lossy().trim_start_matches('/').to_string();
            (rel, c.path.clone())
        })
        .collect()
}

/// What saving would do, worked out before anything is sent.
#[derive(Debug, Clone)]
pub struct BackupPlan {
    pub login: String,
    /// `None` when the repository will be created.
    pub repo: Option<github::OwnRepo>,
    /// `~/.zshrc:12 OPENAI_API_KEY`, one per line that looks like a secret.
    pub secrets: Vec<String>,
}

/// Looks up the account and the repository, and scans the files.
pub async fn plan_backup(token: &str) -> Result<BackupPlan> {
    let login = github::account(token).await?.login;
    let repo = github::own_repo(token, &login, BACKUP_REPO).await?;
    if let Some(r) = &repo
        && !r.private
    {
        return Err(CoreError::GitHub(format!(
            "github.com/{login}/{BACKUP_REPO} is public, so your shell config won't be saved there. Make it private on GitHub, then save again."
        )));
    }
    let mut secrets = Vec::new();
    for (rel, path) in backup_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line, name) in find_secrets(&text) {
            secrets.push(format!("~/{rel}:{line} {name}"));
        }
    }
    Ok(BackupPlan {
        login,
        repo,
        secrets,
    })
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub url: String,
    pub files: usize,
    /// The repository was made by this save.
    pub created: bool,
    /// `false` when GitHub already had these exact files.
    pub changed: bool,
}

/// Creates the private repository when it's missing, then commits every
/// one of the user's startup files to it.
pub async fn save_backup(token: &str, plan: &BackupPlan) -> Result<Saved> {
    let files = backup_files();
    if files.is_empty() {
        return Err(CoreError::Config(
            "There are no shell config files in your home folder to save.".into(),
        ));
    }
    let mut contents = Vec::new();
    for (rel, path) in &files {
        contents.push((rel.clone(), std::fs::read(path)?));
    }
    let (repo, created) = match &plan.repo {
        Some(r) => (r.clone(), false),
        None => (
            github::create_private_repo(
                token,
                BACKUP_REPO,
                "Shell config files (~/.zshrc and friends), saved by Sawit.",
            )
            .await?,
            true,
        ),
    };
    let host = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this Mac".into());
    let names: Vec<&str> = files.iter().map(|(rel, _)| rel.as_str()).collect();
    let message = format!("Save shell config from {host}\n\n{}", names.join("\n"));
    let changed = github::commit_files(
        token,
        &plan.login,
        BACKUP_REPO,
        &repo.default_branch,
        &contents,
        &message,
    )
    .await?;
    Ok(Saved {
        url: repo.html_url,
        files: files.len(),
        created,
        changed,
    })
}
