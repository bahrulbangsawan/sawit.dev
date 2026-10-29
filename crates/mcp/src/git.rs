//! Git: status, history, diffs, branches, staging and commits (no network:
//! fetch, pull and push stay with the user).

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::git::{self, FileChange, Status};

use crate::{Sawit, existing_dir, fail, ok, tri, utc_time};

const LOG_LIMIT: usize = 30;
const MAX_LOG: usize = 500;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RepoArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LogArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(description = "Commits to show (default 30, max 500)")]
    pub limit: Option<usize>,
    #[schemars(description = "Only commits touching this path (relative to the repository)")]
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DiffArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(description = "Only this file (relative to the repository root)")]
    pub path: Option<String>,
    #[schemars(
        description = "Staged changes (index vs HEAD) instead of unstaged ones. Default false"
    )]
    #[serde(default)]
    pub staged: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ShowArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(description = "Commit hash (or any revision, e.g. HEAD~1)")]
    pub hash: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PathsArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(
        description = "Paths relative to the repository root; empty or omitted means all changes"
    )]
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CommitArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(description = "Commit message")]
    pub message: String,
    #[schemars(description = "Amend the last commit instead. Default false")]
    #[serde(default)]
    pub amend: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SwitchArgs {
    #[schemars(description = "A folder inside the repository (absolute path)")]
    pub repo: String,
    #[schemars(
        description = "Branch to switch to (a remote one like origin/x gets a local tracking branch)"
    )]
    pub branch: String,
    #[schemars(description = "Create the branch from the current commit. Default false")]
    #[serde(default)]
    pub create: bool,
}

fn change_line(f: &FileChange, staged: bool) -> String {
    let code = if staged { f.index } else { f.worktree };
    let what = match code {
        'M' => "modified",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'T' => "type changed",
        '?' => "untracked",
        _ => "changed",
    };
    match &f.from {
        Some(from) if staged => format!("  {what}: {from} → {}", f.path),
        _ => format!("  {what}: {}", f.path),
    }
}

/// `git status` as text.
pub(crate) fn status_text(s: &Status) -> String {
    let mut out = Vec::new();
    let mut head = match (&s.branch, &s.head) {
        (Some(b), _) => format!("On branch {b}"),
        (None, Some(h)) => format!("HEAD detached at {h}"),
        (None, None) => "No commits yet".to_string(),
    };
    if let Some(up) = &s.upstream {
        head.push_str(&format!(" · tracking {up}"));
        if s.ahead > 0 || s.behind > 0 {
            head.push_str(&format!(" (ahead {}, behind {})", s.ahead, s.behind));
        }
    }
    out.push(head);
    if s.merging {
        out.push("A merge is in progress.".into());
    }
    let conflicted: Vec<_> = s.conflicted().collect();
    if !conflicted.is_empty() {
        out.push("Conflicts:".into());
        out.extend(conflicted.iter().map(|f| format!("  {}", f.path)));
    }
    let staged: Vec<_> = s.staged().filter(|f| !f.conflicted).collect();
    if !staged.is_empty() {
        out.push("Staged:".into());
        out.extend(staged.iter().map(|f| change_line(f, true)));
    }
    let (untracked, unstaged): (Vec<&FileChange>, Vec<&FileChange>) = s
        .unstaged()
        .filter(|f| !f.conflicted)
        .partition(|f| f.untracked());
    if !unstaged.is_empty() {
        out.push("Not staged:".into());
        out.extend(unstaged.iter().map(|f| change_line(f, false)));
    }
    if !untracked.is_empty() {
        out.push("Untracked:".into());
        out.extend(untracked.iter().map(|f| format!("  {}", f.path)));
    }
    if s.files.is_empty() {
        out.push("Nothing to commit; the working tree is clean.".into());
    }
    out.join("\n")
}

fn safe_rev(rev: &str) -> Result<&str, String> {
    let rev = rev.trim();
    if rev.is_empty() || rev.starts_with('-') || rev.contains(char::is_whitespace) {
        Err(format!("“{rev}” isn't a commit."))
    } else {
        Ok(rev)
    }
}

impl Sawit {
    async fn repo(dir: &str) -> Result<PathBuf, String> {
        let dir = existing_dir(dir)?;
        git::discover(&dir)
            .await
            .ok_or_else(|| format!("{} is not inside a git repository.", dir.display()))
    }

    async fn git_status_tool(&self, repo: &str) -> CallToolResult {
        let repo = tri!(Self::repo(repo).await);
        match git::status(&repo).await {
            Ok(s) => ok(format!("{}\n{}", repo.display(), status_text(&s))),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_log_tool(&self, a: LogArgs) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        let limit = a.limit.unwrap_or(LOG_LIMIT).clamp(1, MAX_LOG);
        let path = a.path.as_deref().filter(|p| !p.trim().is_empty());
        match git::log(&repo, limit, path).await {
            Ok(commits) if commits.is_empty() => ok("No commits."),
            Ok(commits) => ok(commits
                .iter()
                .map(|c| {
                    let refs = if c.refs.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", c.refs.join(", "))
                    };
                    format!(
                        "{} {} {} · {}{refs}",
                        c.short(),
                        utc_time(c.time),
                        c.author,
                        c.subject
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_diff_tool(&self, a: DiffArgs) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        let status = tri!(git::status(&repo).await.map_err(|e| e.to_string()));
        let path = a.path.as_deref().map(str::trim).filter(|p| !p.is_empty());
        let files: Vec<&FileChange> = status
            .files
            .iter()
            .filter(|f| {
                if a.staged {
                    f.is_staged()
                } else {
                    f.is_unstaged()
                }
            })
            .filter(|f| path.is_none_or(|p| f.path == p || f.from.as_deref() == Some(p)))
            .collect();
        if files.is_empty() {
            return ok(match path {
                Some(p) => format!(
                    "No {} changes in {p}.",
                    if a.staged { "staged" } else { "unstaged" }
                ),
                None => format!(
                    "No {} changes.",
                    if a.staged { "staged" } else { "unstaged" }
                ),
            });
        }
        let mut out = Vec::new();
        for f in files {
            match git::diff(&repo, f, a.staged).await {
                Ok(patch) if !patch.trim().is_empty() => out.push(patch),
                Ok(_) => out.push(format!("(no textual diff for {})", f.path)),
                Err(e) => out.push(format!("{}: {e}", f.path)),
            }
        }
        ok(out.join("\n"))
    }

    async fn git_show_tool(&self, a: ShowArgs) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        let rev = tri!(safe_rev(&a.hash));
        match git::show(&repo, rev).await {
            Ok(d) => ok(format!(
                "committer: {}\n\n{}\n\n{}",
                d.committer, d.body, d.patch
            )),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_branches_tool(&self, repo: &str) -> CallToolResult {
        let repo = tri!(Self::repo(repo).await);
        match git::branches(&repo).await {
            Ok(list) if list.is_empty() => ok("No branches yet."),
            Ok(list) => ok(list
                .iter()
                .map(|b| {
                    format!(
                        "{} {}{}{}",
                        if b.current { "*" } else { " " },
                        b.name,
                        if b.remote { " (remote)" } else { "" },
                        b.upstream
                            .as_ref()
                            .map(|u| format!(" → {u}"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_stage_tool(&self, a: PathsArgs, stage: bool) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        let paths: Vec<String> = a
            .paths
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let result = match (stage, paths.is_empty()) {
            (true, true) => git::stage_all(&repo).await,
            (true, false) => git::stage(&repo, &paths).await,
            (false, true) => git::unstage_all(&repo).await,
            (false, false) => git::unstage(&repo, &paths).await,
        };
        tri!(result.map_err(|e| e.to_string()));
        match git::status(&repo).await {
            Ok(s) => ok(status_text(&s)),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_commit_tool(&self, a: CommitArgs) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        if a.message.trim().is_empty() && !a.amend {
            return fail("Write a commit message.");
        }
        let message = if a.message.trim().is_empty() {
            tri!(git::last_message(&repo).await.map_err(|e| e.to_string()))
        } else {
            a.message
        };
        match git::commit(&repo, &message, a.amend).await {
            Ok(out) => ok(if out.trim().is_empty() {
                "Committed.".to_string()
            } else {
                out.trim().to_string()
            }),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn git_switch_tool(&self, a: SwitchArgs) -> CallToolResult {
        let repo = tri!(Self::repo(&a.repo).await);
        let name = tri!(safe_rev(&a.branch)).to_string();
        if a.create {
            return match git::create_branch(&repo, &name).await {
                Ok(()) => ok(format!("Created and switched to {name}.")),
                Err(e) => fail(e.to_string()),
            };
        }
        let branches = tri!(git::branches(&repo).await.map_err(|e| e.to_string()));
        let branch = branches
            .iter()
            .find(|b| !b.remote && b.name == name)
            .or_else(|| branches.iter().find(|b| b.remote && b.name == name));
        let Some(branch) = branch else {
            return fail(format!(
                "No branch named {name}. Pass create=true to make it, or see git_branches."
            ));
        };
        match git::switch(&repo, branch).await {
            Ok(()) => ok(format!("Switched to {name}.")),
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = git_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Git status of a repository: branch, upstream ahead/behind, conflicts, staged, unstaged and untracked files"
    )]
    async fn git_status(&self, Parameters(a): Parameters<RepoArgs>) -> CallToolResult {
        self.git_status_tool(&a.repo).await
    }

    #[tool(
        description = "Recent commits (hash, date UTC, author, subject, refs), optionally for one path"
    )]
    async fn git_log(&self, Parameters(a): Parameters<LogArgs>) -> CallToolResult {
        self.git_log_tool(a).await
    }

    #[tool(
        description = "Diff of unstaged (default) or staged changes, for every changed file or one path; untracked files show as new"
    )]
    async fn git_diff(&self, Parameters(a): Parameters<DiffArgs>) -> CallToolResult {
        self.git_diff_tool(a).await
    }

    #[tool(description = "Show one commit: committer, full message and patch")]
    async fn git_show(&self, Parameters(a): Parameters<ShowArgs>) -> CallToolResult {
        self.git_show_tool(a).await
    }

    #[tool(description = "List local and remote branches, marking the current one and upstreams")]
    async fn git_branches(&self, Parameters(a): Parameters<RepoArgs>) -> CallToolResult {
        self.git_branches_tool(&a.repo).await
    }

    #[tool(description = "Stage files (git add); all changes when no paths are given")]
    async fn git_stage(&self, Parameters(a): Parameters<PathsArgs>) -> CallToolResult {
        self.git_stage_tool(a, true).await
    }

    #[tool(description = "Unstage files; all staged changes when no paths are given")]
    async fn git_unstage(&self, Parameters(a): Parameters<PathsArgs>) -> CallToolResult {
        self.git_stage_tool(a, false).await
    }

    #[tool(
        description = "Commit the staged changes with a message (amend=true rewrites the last commit; an empty message keeps its message)"
    )]
    async fn git_commit(&self, Parameters(a): Parameters<CommitArgs>) -> CallToolResult {
        self.git_commit_tool(a).await
    }

    #[tool(description = "Switch to a branch, or create one from the current commit (create=true)")]
    async fn git_switch(&self, Parameters(a): Parameters<SwitchArgs>) -> CallToolResult {
        self.git_switch_tool(a).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_as_text() {
        let raw = "# branch.oid 1234567890abcdef\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/a.rs\0\
1 A. N... 000000 100644 100644 000 ccc new.rs\0\
? notes.txt\0";
        let text = status_text(&git::parse_status(raw));
        assert!(
            text.starts_with("On branch main · tracking origin/main (ahead 2, behind 1)"),
            "{text}"
        );
        assert!(text.contains("Staged:\n  added: new.rs"));
        assert!(text.contains("Not staged:\n  modified: src/a.rs"));
        assert!(text.contains("Untracked:\n  notes.txt"));
        let clean = status_text(&git::parse_status("# branch.oid abc\0# branch.head main\0"));
        assert!(clean.contains("clean"));
    }

    #[test]
    fn revisions_cannot_be_options() {
        assert!(safe_rev("--output=/tmp/x").is_err());
        assert!(safe_rev("a b").is_err());
        assert_eq!(safe_rev(" HEAD~1 ").unwrap(), "HEAD~1");
    }
}
