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
}

fn settings(a: &PromptBackupArgs, root: &std::path::Path) -> BackupSettings {
    let mut s = BackupSettings::load(&root.join(prompt_sync::SETTINGS));
    if let Some(repo) = a.repo.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        s.repo = repo.to_string();
    }
    s
}

fn said(verb: &str, done: &SyncOutcome) -> String {
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
        description = "Back up the prompt library to a private GitHub repository of the user's (sawit-prompts by default, created on the first backup), one Markdown file per prompt with its name, tags and {{variables}} in the front matter, with Sawit's GitHub sign-in (only the user can sign in). Sends the prompts changed here since the last sync; prompts changed on GitHub too are listed, not overwritten, unless named in `overwrite`. Works without the app"
    )]
    async fn backup_prompts(&self, Parameters(a): Parameters<PromptBackupArgs>) -> CallToolResult {
        let token = tri!(tri!(blocking(token).await));
        let lib = tri!(self.library());
        let root = self.paths.root.clone();
        let s = settings(&a, &root);
        let state = root.join(prompt_sync::SYNC_STATE);
        let done = tri!(
            prompt_sync::back_up(&token, &lib, &s, &state, &a.overwrite)
                .await
                .map_err(|e| e.to_string())
        );
        ok(said("back up", &done))
    }

    #[tool(
        description = "Restore prompts from the user's private GitHub prompt repository (see backup_prompts): prompts changed on GitHub or only there replace or join the ones here, keeping their use counts; prompts changed here since the last sync stay; prompts changed on both sides are listed, not overwritten, unless named in `overwrite` (GitHub's version wins). Sawit's open prompt tabs reload. Works without the app"
    )]
    async fn restore_prompts(&self, Parameters(a): Parameters<PromptBackupArgs>) -> CallToolResult {
        let token = tri!(tri!(blocking(token).await));
        let mut lib = tri!(self.library());
        let root = self.paths.root.clone();
        let s = settings(&a, &root);
        let state = root.join(prompt_sync::SYNC_STATE);
        let done = tri!(
            prompt_sync::restore(&token, &mut lib, &s, &state, &a.overwrite)
                .await
                .map_err(|e| e.to_string())
        );
        if done.synced.is_empty() {
            return ok(said("restore", &done));
        }
        if let Err(e) = self.save_library(&lib).await {
            return fail(e);
        }
        ok(said("restore", &done))
    }
}
