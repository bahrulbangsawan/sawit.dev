//! `backup_prompts` and `restore_prompts`: the prompt library on GitHub, in a private repository
//! of the user's (`sawit-prompts` unless they picked another), as the Prompts sidebar's Back Up
//! Now and Restore do. A prompt changed both here and on GitHub is listed, never overwritten,
//! unless the call names it.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::prompt_sync::{self, BackupSettings, SyncOutcome};

use crate::shell_sync::token;
use crate::{Sawit, blocking, fail, ok, tri};

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct PromptBackupArgs {
    #[schemars(
        description = "Repository name under the signed-in account. Default: the one chosen in Sawit, else sawit-prompts (created private on the first backup). A public repository is refused"
    )]
    pub repo: Option<String>,
    #[schemars(
        description = "Prompt ids changed both here and on GitHub whose version wins: this Mac's for backup_prompts, GitHub's for restore_prompts. Ask the user first; without them such prompts are only listed"
    )]
    #[serde(default)]
    pub overwrite: Vec<String>,
    #[schemars(
        description = "true: change nothing, only list what would be sent, brought in or deleted (and, for a backup, the lines that look like secrets). Default false"
    )]
    #[serde(default)]
    pub dry_run: bool,
    #[schemars(
        description = "backup_prompts only: send prompts whose text looks like it holds a secret (an API key or token). Without it such a backup sends nothing and lists those lines: ask the user first. Default false"
    )]
    #[serde(default)]
    pub allow_secrets: bool,
}

fn settings(a: &PromptBackupArgs, root: &std::path::Path) -> BackupSettings {
    let mut s = BackupSettings::load(&root.join(prompt_sync::SETTINGS));
    if let Some(repo) = a.repo.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        s.repo = repo.to_string();
    }
    s
}

fn said(verb: &str, done: &SyncOutcome) -> String {
    if done.held {
        return format!(
            "Nothing was backed up: these lines look like secrets and would go to {}:\n{}\nAsk the user; if they agree, call again with allow_secrets=true.",
            done.repo,
            done.secrets
                .iter()
                .map(|l| format!("- {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    if done.dry_run {
        let list = |items: &[String]| {
            if items.is_empty() {
                "none".to_string()
            } else {
                items.join(", ")
            }
        };
        let mut out = format!(
            "Dry run, nothing changed ({}). Would {verb}: {}. Would delete {}: {}.",
            done.repo,
            list(&done.synced),
            if verb == "back up" {
                "on GitHub"
            } else {
                "here"
            },
            list(&done.deleted)
        );
        if !done.secrets.is_empty() {
            out.push_str(&format!(
                " Lines that look like secrets (a backup asks first): {}.",
                done.secrets.join("; ")
            ));
        }
        if !done.conflicts.is_empty() {
            let names: Vec<String> = done.conflicts.iter().map(|c| c.title.clone()).collect();
            out.push_str(&format!(" Changed on both sides: {}.", names.join(", ")));
        }
        return out;
    }
    let mut out = if done.synced.is_empty() {
        format!("Nothing to {verb}: {} matches.", done.repo)
    } else {
        format!(
            "{} {} prompts ({}): {}.",
            if verb == "back up" {
                "Backed up"
            } else {
                "Restored"
            },
            done.synced.len(),
            done.repo,
            done.synced.join(", ")
        )
    };
    if done.created_repo {
        out = format!("Created the private repository {}. {out}", done.repo);
    }
    if !done.deleted.is_empty() {
        out.push_str(&format!(
            " Deleted {} (deleted on the other side since the last sync): {}.",
            if verb == "back up" {
                "on GitHub"
            } else {
                "here"
            },
            done.deleted.join(", ")
        ));
    }
    if !done.conflicts.is_empty() {
        let list: Vec<String> = done
            .conflicts
            .iter()
            .map(|c| format!("- {} (id {})", c.title, c.id))
            .collect();
        out.push_str(&format!(
            "\nChanged both here and on GitHub, left as they are (name them in `overwrite` once the user chose):\n{}",
            list.join("\n")
        ));
    }
    out
}

#[tool_router(router = prompt_backup_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Back up the prompt library to a private GitHub repository of the user's (sawit-prompts by default, created on the first backup), one Markdown file per prompt with its name, tags and {{variables}} in the front matter, with Sawit's GitHub sign-in (only the user can sign in). Sends the prompts changed here since the last sync and deletes on GitHub the ones deleted here since; prompts changed on GitHub too are listed, not overwritten, unless named in `overwrite`. Prompts whose text looks like a secret hold the backup back (listed) until allow_secrets. dry_run only says what would change. Works without the app"
    )]
    async fn backup_prompts(&self, Parameters(a): Parameters<PromptBackupArgs>) -> CallToolResult {
        let token = tri!(tri!(blocking(token).await));
        let lib = tri!(self.library());
        let root = self.paths.root.clone();
        let s = settings(&a, &root);
        let state = root.join(prompt_sync::SYNC_STATE);
        let done = tri!(
            prompt_sync::back_up(
                &token,
                &lib,
                &s,
                &state,
                &a.overwrite,
                prompt_sync::Options {
                    allow_secrets: a.allow_secrets,
                    dry_run: a.dry_run,
                },
            )
            .await
            .map_err(|e| e.to_string())
        );
        ok(said("back up", &done))
    }

    #[tool(
        description = "Restore prompts from the user's private GitHub prompt repository (see backup_prompts): prompts changed on GitHub or only there replace or join the ones here, keeping their use counts; prompts deleted on GitHub since the last sync are deleted here; prompts changed here since the last sync stay; prompts changed on both sides are listed, not overwritten, unless named in `overwrite` (GitHub's version wins). Sawit's open prompt tabs reload. Works without the app"
    )]
    async fn restore_prompts(&self, Parameters(a): Parameters<PromptBackupArgs>) -> CallToolResult {
        let token = tri!(tri!(blocking(token).await));
        let mut lib = tri!(self.library());
        let root = self.paths.root.clone();
        let s = settings(&a, &root);
        let state = root.join(prompt_sync::SYNC_STATE);
        let done = tri!(
            prompt_sync::restore(&token, &mut lib, &s, &state, &a.overwrite, a.dry_run)
                .await
                .map_err(|e| e.to_string())
        );
        if done.dry_run {
            return ok(said("restore", &done));
        }
        if !(done.synced.is_empty() && done.deleted.is_empty())
            && let Err(e) = self.save_library(&lib).await
        {
            return fail(e);
        }
        // The state only after the library is on disk, never ahead of it.
        if let Some(base) = &done.restored_base
            && let Err(e) = base.save(&state)
        {
            return fail(e.to_string());
        }
        ok(said("restore", &done))
    }
}
