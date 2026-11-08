//! Git's other actions, as the Git panel has them: throw away changes, merge a branch into the
//! current one (and abort a merge that stopped on conflicts), and make a folder a repository. The
//! same `sawit_core::git` functions the panel calls; none needs the app.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::git;

use crate::git::status_text;
use crate::{Sawit, existing_dir, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DiscardArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(
        description = "Paths relative to the repository root whose unstaged changes to throw away (an untracked file is deleted). Required: name each file"
    )]
    pub paths: Vec<String>,
    #[schemars(
        description = "Must be true: the changes are lost (git checkout / git clean), with no undo"
    )]
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MergeArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(description = "The branch to merge into the current one (local or remote)")]
    pub branch: Option<String>,
    #[schemars(
        description = "Abort the merge in progress instead (it stopped on conflicts): the working tree goes back to before it. Needs confirm"
    )]
    #[serde(default)]
    pub abort: bool,
    #[schemars(description = "Must be true with abort: what the merge left is thrown away")]
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InitArgs {
    #[schemars(description = "The folder to make a repository (absolute path); it must exist")]
    pub dir: String,
}

/// The repository's files to discard, each a change it has unstaged; or why not.
fn to_discard(status: &git::Status, paths: &[String]) -> Result<Vec<git::FileChange>, String> {
    let wanted: Vec<&str> = paths
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if wanted.is_empty() {
        return Err(
            "Name the files to discard in paths: discarding throws their changes away.".into(),
        );
    }
    wanted
        .iter()
        .map(|path| {
            status
                .files
                .iter()
                .find(|f| f.path == *path && f.worktree != '.')
                .cloned()
                .ok_or_else(|| format!("{path} has no unstaged change (see git_status)."))
        })
        .collect()
}

#[tool_router(router = git_ops_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "DESTRUCTIVE: throw away the unstaged changes to the named files, as the Git panel's Discard Changes does (a tracked file goes back to what is staged or committed; an untracked file is deleted). No undo, so it needs confirm: true. Answers the status after"
    )]
    async fn git_discard(&self, Parameters(a): Parameters<DiscardArgs>) -> CallToolResult {
        if !a.confirm {
            return fail("Discarding loses the changes for good: pass confirm: true to do it.");
        }
        let repo = tri!(Self::repo(&a.repo).await);
        let status = tri!(git::status(&repo).await.map_err(|e| e.to_string()));
        let files = tri!(to_discard(&status, &a.paths));
        for file in &files {
            tri!(git::discard(&repo, file).await.map_err(|e| e.to_string()));
        }
        match git::status(&repo).await {
            Ok(s) => ok(format!(
                "Discarded {} file(s).\n{}",
                files.len(),
                status_text(&s)
            )),
            Err(e) => fail(e.to_string()),
        }
    }

    #[tool(
        description = "Merge a branch into the current one, as the Git panel's Merge into Current does; or, with abort and confirm: true, abort a merge that stopped on conflicts. Answers git's output and the status after (conflicted files are listed: resolve them, git_stage and git_commit)"
    )]
    async fn git_merge(&self, Parameters(a): Parameters<MergeArgs>) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        if a.abort {
            if !a.confirm {
                return fail("Aborting throws away what the merge left: pass confirm: true.");
            }
            tri!(git::merge_abort(&repo).await.map_err(|e| e.to_string()));
            return match git::status(&repo).await {
                Ok(s) => ok(format!("Merge aborted.\n{}", status_text(&s))),
                Err(e) => fail(e.to_string()),
            };
        }
        let Some(branch) = a.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) else {
            return fail("Name the branch to merge (see git_branches), or pass abort.");
        };
        if branch.starts_with('-') || branch.contains(char::is_whitespace) {
            return fail(format!("“{branch}” isn't a branch."));
        }
        let out = tri!(git::merge(&repo, branch).await.map_err(|e| e.to_string()));
        let status = git::status(&repo)
            .await
            .map(|s| status_text(&s))
            .unwrap_or_default();
        let said = format!("{}{}\n{status}", out.stdout.trim(), out.stderr.trim());
        if out.ok {
            ok(said)
        } else {
            fail(format!("The merge stopped: {said}"))
        }
    }

    #[tool(
        description = "Make a folder a git repository, with main as its first branch, as the Git panel's Initialize Repository does. Answers the repository's status"
    )]
    async fn git_init(&self, Parameters(a): Parameters<InitArgs>) -> CallToolResult {
        let dir: PathBuf = tri!(existing_dir(&a.dir));
        if git::discover(&dir).await.is_some() {
            return fail(format!(
                "{} is already inside a git repository.",
                dir.display()
            ));
        }
        tri!(git::init(&dir).await.map_err(|e| e.to_string()));
        match git::status(&dir).await {
            Ok(s) => ok(format!(
                "Initialized {}.\n{}",
                dir.display(),
                status_text(&s)
            )),
            Err(e) => fail(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(files: &[(&str, char)]) -> git::Status {
        git::Status {
            files: files
                .iter()
                .map(|(path, wt)| git::FileChange {
                    path: path.to_string(),
                    from: None,
                    index: '.',
                    worktree: *wt,
                    conflicted: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn only_named_files_with_unstaged_changes_are_discarded() {
        let s = status(&[("a.rs", 'M'), ("b.rs", 'M')]);
        let files = to_discard(&s, &["a.rs".into()]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.rs");
        assert!(to_discard(&s, &[]).unwrap_err().contains("Name the files"));
        assert!(
            to_discard(&s, &["c.rs".into()])
                .unwrap_err()
                .contains("no unstaged change")
        );
    }
}
