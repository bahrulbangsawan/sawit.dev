//! `shell_files_sync`: the Config Files panel's two-way sync of the user's shell files with their
//! private `sawit-shell-config` repository, with the app's one GitHub sign-in. `status` says how
//! each file stands; `upload` commits the files named; `download` replaces the files named with
//! GitHub's, each backed up beside itself first. Never a file's contents in the answer.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::github::{Keychain, TokenStore};
use sawit_core::instances::AppPaths;
use sawit_core::shell_config::{self, Direction, FileSync, SyncStatus};
use serde_json::json;

use crate::{Sawit, blocking, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ShellSyncArgs {
    #[schemars(
        description = "status (how each file stands), upload (commit `files` to GitHub) or download (replace `files` here with GitHub's, each backed up first)"
    )]
    pub action: String,
    #[schemars(
        description = "The files to upload or download, by their path in the repository as `status` lists them (\".zshrc\"). Required for upload and download: nothing is sent or replaced unless named"
    )]
    #[serde(default)]
    pub files: Vec<String>,
    #[schemars(
        description = "upload only: send lines that look like secrets (API keys, tokens). Without it an upload that has some is refused, listing them"
    )]
    #[serde(default)]
    pub allow_secrets: bool,
    #[schemars(
        description = "Act on files changed on both sides too (upload: this Mac's version wins; download: GitHub's wins). Without it they are refused"
    )]
    #[serde(default)]
    pub overwrite_conflicts: bool,
}

/// The sign-in the app keeps, or why there is none.
pub(crate) fn token() -> Result<String, String> {
    match Keychain::user().load() {
        Ok(Some(t)) if !t.trim().is_empty() => Ok(t),
        Ok(_) => Err("Sawit isn't signed in to GitHub. Only the user can sign in: ask them to sign in from Sawit's title bar.".into()),
        Err(e) => Err(format!("Couldn't read Sawit's GitHub sign-in from the Keychain: {e}")),
    }
}

fn state_path() -> std::path::PathBuf {
    AppPaths::default_macos()
        .root
        .join(shell_config::SYNC_STATE)
}

fn file_json(f: &FileSync) -> serde_json::Value {
    json!({ "file": f.rel, "path": f.path.display().to_string(), "state": f.state })
}

fn status_json(s: &SyncStatus) -> serde_json::Value {
    json!({
        "repo": s.backup.as_ref().map(|b| format!("{}/{}", b.login, shell_config::BACKUP_REPO)),
        "url": s.backup.as_ref().map(|b| b.repo.html_url.clone()),
        "last_sync": s.synced_at,
        "files": s.files.iter().map(file_json).collect::<Vec<_>>(),
    })
}

/// The files of `status` named `names`, each refused when it isn't listed, or when it changed on
/// both sides and `overwrite` wasn't said, or (for `fits`) when there is nothing to move.
/// The files `names` names that move the way `fits` allows ([`shell_config::pick_files`]).
fn chosen<'a>(
    status: &'a SyncStatus,
    names: &[String],
    overwrite: bool,
    way: Direction,
) -> Result<Vec<&'a FileSync>, String> {
    shell_config::pick_files(status, names, overwrite, way).map_err(|e| {
        e.replace(
            "choose a side first (overwrite)",
            "pass overwrite_conflicts to pick a side, after the user chose",
        )
        .replace(
            "as the status lists them",
            "in `files`, as `status` lists them",
        )
    })
}

impl Sawit {
    pub async fn shell_files_sync_tool(&self, a: ShellSyncArgs) -> CallToolResult {
        let token = tri!(tri!(blocking(token).await));
        let state = state_path();
        let status =
            tri!((shell_config::sync_status(&token, &state).await).map_err(|e| e.to_string()));
        match a.action.as_str() {
            "status" => {
                let v = status_json(&status);
                ok(serde_json::to_string_pretty(&v).unwrap_or_default())
            }
            "upload" => {
                let Some(backup) = &status.backup else {
                    return fail(format!(
                        "github.com/{}/{} doesn't exist yet: the user makes it with Save Shell Files to GitHub in Sawit's Config Files panel.",
                        status.login,
                        shell_config::BACKUP_REPO
                    ));
                };
                let files = tri!(chosen(
                    &status,
                    &a.files,
                    a.overwrite_conflicts,
                    Direction::Upload
                ));
                let secrets = shell_config::secret_lines(&files);
                if !secrets.is_empty() && !a.allow_secrets {
                    return fail(format!(
                        "These lines look like secrets, so nothing was sent:\n{}\nAsk the user; pass allow_secrets to send them to the private repository anyway.",
                        secrets.join("\n")
                    ));
                }
                let rels: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
                let host = shell_config::computer_name();
                let done = tri!(
                    (shell_config::upload(&token, backup, &rels, &state, &host).await)
                        .map_err(|e| e.to_string())
                );
                ok(if done.changed {
                    format!("Uploaded {} to {}.", rels.join(", "), backup.repo.html_url)
                } else {
                    format!("GitHub already had {}: nothing to upload.", rels.join(", "))
                })
            }
            "download" => {
                let Some(backup) = &status.backup else {
                    return fail("There is no shell files repository on GitHub to download from.");
                };
                let files = tri!(chosen(
                    &status,
                    &a.files,
                    a.overwrite_conflicts,
                    Direction::Download
                ));
                let owned: Vec<FileSync> = files.into_iter().cloned().collect();
                let contents = tri!(
                    (shell_config::fetch_remote(&token, backup, &owned).await)
                        .map_err(|e| e.to_string())
                );
                let known = shell_config::known_files_here();
                let commit = backup.tree.commit.clone();
                let done = tri!(
                    tri!(
                        blocking(move || shell_config::apply_download(
                            &known,
                            &contents,
                            &state,
                            &commit,
                            sawit_core::github::now()
                        ))
                        .await
                    )
                    .map_err(|e| e.to_string())
                );
                let lines: Vec<String> = done
                    .replaced
                    .iter()
                    .map(|r| match &r.backup {
                        Some(b) => {
                            format!("- {} (was backed up to {})", r.path.display(), b.display())
                        }
                        None => format!("- {} (new)", r.path.display()),
                    })
                    .collect();
                ok(format!(
                    "Replaced with GitHub's version:\n{}\nOpen shells read the new files once they run `source` on them or start again.",
                    lines.join("\n")
                ))
            }
            other => fail(format!(
                "action is status, upload or download, not {other:?}."
            )),
        }
    }
}

#[tool_router(router = shell_sync_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Sync the user's shell files (~/.zshrc, ~/.zprofile, …) with their private GitHub repository sawit-shell-config, with Sawit's GitHub sign-in (only the user can sign in). action status: each file's state against the repository since the last sync from this Mac (same, changed_here, changed_on_git_hub, changed_on_both, only_here, only_on_git_hub), as JSON. action upload: commits the files named in `files`; refused when lines look like secrets unless allow_secrets. action download: replaces the files named in `files` with GitHub's, each backed up beside itself first (~/.zshrc.sawit-backup-<time>). Files changed on both sides are refused unless overwrite_conflicts: ask the user first. Never returns file contents. Works without the app"
    )]
    async fn shell_files_sync(&self, Parameters(a): Parameters<ShellSyncArgs>) -> CallToolResult {
        self.shell_files_sync_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use sawit_core::github::FileState;

    use super::*;

    fn status() -> SyncStatus {
        let file = |rel: &str, state| FileSync {
            rel: rel.into(),
            path: format!("/home/me/{rel}").into(),
            state,
            remote: None,
        };
        SyncStatus {
            login: "me".into(),
            backup: None,
            files: vec![
                file(".zshrc", FileState::ChangedOnGitHub),
                file(".bashrc", FileState::ChangedOnBoth),
                file(".profile", FileState::ChangedHere),
            ],
            synced_at: None,
        }
    }

    /// Nothing is moved unless named; a file changed on both sides only with overwrite_conflicts;
    /// a file can't be downloaded when only this Mac changed it.
    #[test]
    fn files_are_moved_only_when_named_and_movable() {
        let s = status();
        let names = |n: &[&str]| n.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(chosen(&s, &[], false, Direction::Download).is_err());
        let picked = chosen(&s, &names(&[".zshrc"]), false, Direction::Download).unwrap();
        assert_eq!(picked[0].rel, ".zshrc");
        assert!(chosen(&s, &names(&[".bashrc"]), false, Direction::Download).is_err());
        assert!(chosen(&s, &names(&[".bashrc"]), true, Direction::Download).is_ok());
        assert!(chosen(&s, &names(&[".profile"]), false, Direction::Download).is_err());
        assert!(chosen(&s, &names(&["../.ssh/config"]), true, Direction::Download).is_err());
        assert_eq!(
            status_json(&s)["files"][1]["state"],
            serde_json::json!("changed_on_both")
        );
    }
}
